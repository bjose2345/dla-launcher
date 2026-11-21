//! Following a game on Windows and showing its window through Magpie.
//!
//! Magpie is one program for the whole desktop: it reads its profiles when
//! it starts and takes no command afterwards. So one Magpie of the launcher's
//! runs while any enlarged game does, started again with a profile for each
//! of them whenever another one opens its window, and asked to quit when the
//! last one ends.

use std::{
    collections::HashSet,
    fs, mem,
    process::{Child, Command},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use dla_application::launch::{LaunchError, LaunchProcessExit, ManagedLaunchProcess};
use windows_sys::{
    Win32::{
        Foundation::{HWND, LPARAM, RECT},
        Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetClientRect, GetWindowThreadProcessId, HWND_BROADCAST,
            IsWindowVisible, PostMessageW, RegisterWindowMessageW,
        },
    },
    core::BOOL,
};

use super::{
    Magpie,
    settings::{GameWindow, Shown, Size, settings},
};
use crate::windows_process::{program_of, running_processes};

const WINDOW_LIMIT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(250);
// Some games open a first window and replace it at once.
const WINDOW_SETTLE: Duration = Duration::from_millis(750);
// Splash screens, tool windows and input-method windows are smaller.
const SMALLEST_GAME: u32 = 200;
const QUIT_LIMIT: Duration = Duration::from_secs(3);
const PROGRAM_FILE: &str = "Magpie.exe";

/// A running game whose window Magpie shows enlarged once it is up.
pub(crate) struct EnlargedGame {
    game: Box<dyn ManagedLaunchProcess>,
    ended: Arc<AtomicBool>,
}

impl EnlargedGame {
    pub(crate) fn follow(
        game: Box<dyn ManagedLaunchProcess>,
        magpie: Magpie,
        shown: Shown,
    ) -> Self {
        let ended = Arc::new(AtomicBool::new(false));
        let process = game.process_id();
        {
            let ended = Arc::clone(&ended);
            thread::spawn(move || show(process, &magpie, shown, &ended));
        }
        Self { game, ended }
    }

    fn noting<T>(&self, exit: Result<Option<T>, LaunchError>) -> Result<Option<T>, LaunchError> {
        if !matches!(exit, Ok(None)) {
            self.ended.store(true, Ordering::SeqCst);
        }
        exit
    }
}

impl ManagedLaunchProcess for EnlargedGame {
    fn process_id(&self) -> u32 {
        self.game.process_id()
    }

    fn try_wait(&mut self) -> Result<Option<LaunchProcessExit>, LaunchError> {
        let exit = self.game.try_wait();
        self.noting(exit)
    }

    fn terminate(&mut self) -> Result<(), LaunchError> {
        self.game.terminate()
    }

    fn wait(&mut self) -> Result<LaunchProcessExit, LaunchError> {
        let exit = self.game.wait();
        self.ended.store(true, Ordering::SeqCst);
        exit
    }
}

impl Drop for EnlargedGame {
    fn drop(&mut self) {
        self.ended.store(true, Ordering::SeqCst);
    }
}

/// The game always starts; whatever goes wrong here only leaves it at the
/// size it opened in.
fn show(process: u32, magpie: &Magpie, shown: Shown, ended: &AtomicBool) {
    if let Err(error) = magpie.install() {
        log::warn!(target: "dla::launch", "event=game_not_enlarged reason={error}");
        return;
    }
    let Some(window) = game_window_once_open(process, ended) else {
        if !ended.load(Ordering::SeqCst) {
            log::warn!(target: "dla::launch", "event=game_not_enlarged reason=no game window opened");
        }
        return;
    };
    let showing = match host().show(magpie, window, shown) {
        Ok(showing) => {
            log::info!(target: "dla::launch", "event=game_enlarged process_id={process}");
            showing
        }
        Err(error) => {
            log::warn!(target: "dla::launch", "event=game_not_enlarged reason={error}");
            return;
        }
    };
    while !ended.load(Ordering::SeqCst) {
        thread::sleep(POLL);
    }
    host().done(showing);
}

struct Host {
    games: Vec<(u64, GameWindow, Shown)>,
    shown_so_far: u64,
    magpie: Option<Child>,
}

static HOST: Mutex<Host> = Mutex::new(Host {
    games: Vec::new(),
    shown_so_far: 0,
    magpie: None,
});

fn host() -> std::sync::MutexGuard<'static, Host> {
    HOST.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Host {
    fn show(&mut self, magpie: &Magpie, window: GameWindow, shown: Shown) -> Result<u64, String> {
        // Magpie allows one instance for the whole desktop: beside the
        // player's own, the launcher's would only bring theirs to the front.
        let ours = self.magpie.as_ref().map(Child::id);
        if running_processes()
            .iter()
            .any(|process| process.runs(PROGRAM_FILE) && Some(process.id) != ours)
        {
            return Err("Magpie is already running with the player's own settings".to_owned());
        }
        self.quit();
        self.shown_so_far += 1;
        let showing = self.shown_so_far;
        self.games.push((showing, window, shown));
        if let Err(error) = self.start(magpie) {
            self.games.retain(|(game, _, _)| *game != showing);
            return Err(format!("Magpie could not be started: {error}"));
        }
        Ok(showing)
    }

    fn done(&mut self, showing: u64) {
        self.games.retain(|(game, _, _)| *game != showing);
        if self.games.is_empty() {
            self.quit();
        }
    }

    fn start(&mut self, magpie: &Magpie) -> std::io::Result<()> {
        let games = self
            .games
            .iter()
            .map(|(_, window, shown)| (window.clone(), *shown))
            .collect::<Vec<_>>();
        let file = magpie.settings_file();
        if let Some(folder) = file.parent() {
            fs::create_dir_all(folder)?;
        }
        fs::write(file, settings(&games))?;
        self.magpie = Some(
            Command::new(magpie.program())
                .arg("-t")
                .current_dir(magpie.folder())
                .spawn()?,
        );
        Ok(())
    }

    /// Magpie ends on its own quit message; one that does not is stopped.
    fn quit(&mut self) {
        let Some(mut magpie) = self.magpie.take() else {
            return;
        };
        ask_magpie_to_quit();
        let asked = Instant::now();
        while asked.elapsed() < QUIT_LIMIT {
            if !matches!(magpie.try_wait(), Ok(None)) {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let _ = magpie.kill();
        let _ = magpie.wait();
    }
}

fn ask_magpie_to_quit() {
    let name = "WM_MAGPIE_QUIT\0".encode_utf16().collect::<Vec<_>>();
    // SAFETY: plain Win32 calls with a NUL-terminated string that outlives them.
    unsafe {
        let message = RegisterWindowMessageW(name.as_ptr());
        if message != 0 {
            PostMessageW(HWND_BROADCAST, message, 0, 0);
        }
    }
}

fn game_window_once_open(process: u32, ended: &AtomicBool) -> Option<GameWindow> {
    let started = Instant::now();
    while started.elapsed() < WINDOW_LIMIT && !ended.load(Ordering::SeqCst) {
        if let Some(first) = game_window(process) {
            thread::sleep(WINDOW_SETTLE);
            return game_window(process).or(Some(first));
        }
        thread::sleep(POLL);
    }
    None
}

/// The largest visible window of the game or of a process it started.
fn game_window(process: u32) -> Option<GameWindow> {
    let family = family_of(process);
    let mut windows: Vec<HWND> = Vec::new();
    // SAFETY: `collect` only pushes onto the vector behind `lparam`, which
    // outlives the call.
    unsafe {
        EnumWindows(Some(collect), &mut windows as *mut Vec<HWND> as LPARAM);
    }
    windows
        .into_iter()
        .filter_map(|window| described(window, &family))
        .max_by_key(|window| u64::from(window.picture.width) * u64::from(window.picture.height))
}

unsafe extern "system" fn collect(window: HWND, windows: LPARAM) -> BOOL {
    // SAFETY: `windows` is the vector `game_window` passed to EnumWindows.
    let windows = unsafe { &mut *(windows as *mut Vec<HWND>) };
    windows.push(window);
    1
}

fn family_of(process: u32) -> HashSet<u32> {
    let running = running_processes();
    let mut family = HashSet::from([process]);
    loop {
        let before = family.len();
        for candidate in &running {
            if family.contains(&candidate.parent) {
                family.insert(candidate.id);
            }
        }
        if family.len() == before {
            return family;
        }
    }
}

fn described(window: HWND, family: &HashSet<u32>) -> Option<GameWindow> {
    // SAFETY: plain Win32 calls on a window handle that may have gone, which
    // they report by failing; every buffer is sized as passed.
    unsafe {
        if IsWindowVisible(window) == 0 {
            return None;
        }
        let mut process = 0u32;
        GetWindowThreadProcessId(window, &mut process);
        if !family.contains(&process) {
            return None;
        }
        let mut client: RECT = mem::zeroed();
        if GetClientRect(window, &mut client) == 0 {
            return None;
        }
        let picture = size_of(&client)
            .filter(|picture| picture.width >= SMALLEST_GAME && picture.height >= SMALLEST_GAME)?;
        let mut class = [0u16; 257];
        let length = GetClassNameW(window, class.as_mut_ptr(), class.len() as i32);
        if length <= 0 {
            return None;
        }
        let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = mem::zeroed();
        info.cbSize = mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        Some(GameWindow {
            program: program_of(process)?,
            class: String::from_utf16_lossy(&class[..length as usize]),
            picture,
            screen: size_of(&info.rcMonitor)?,
        })
    }
}

fn size_of(rectangle: &RECT) -> Option<Size> {
    let width = u32::try_from(rectangle.right - rectangle.left).ok()?;
    let height = u32::try_from(rectangle.bottom - rectangle.top).ok()?;
    (width > 0 && height > 0).then_some(Size { width, height })
}
