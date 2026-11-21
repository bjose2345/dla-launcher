use dla_application::paths::resolve_within;
use std::{
    ffi::OsStr,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use dla_application::launch::{
    LaunchClock, LaunchError, LaunchExecutionPlan, LaunchExecutionResult, LaunchExecutor,
    LaunchProcessExit, ManagedLaunchProcess,
};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use dla_domain::installation::InstallationPlatform;
use dla_domain::{
    installation::{LaunchActionKind, RelativePath},
    launch::{LaunchAdapter, PictureFilter, Sharpness, WindowScale},
};
use sha2::{Digest, Sha256};

mod enlarged;
mod locale;
mod magpie;
#[cfg(any(target_os = "windows", test))]
mod plain_drive;
mod release;
#[cfg(target_os = "windows")]
mod windows_process;

pub use locale::{LocaleRemulator, VisualCppRuntime};
pub use magpie::Magpie;

const HASH_BUFFER_BYTES: usize = 1024 * 1024;

pub struct SystemLaunchClock;

impl LaunchClock for SystemLaunchClock {
    fn now(&self) -> String {
        time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .expect("RFC 3339 timestamps are always representable")
    }
}

struct DesktopManagedProcess {
    child: Child,
}

impl ManagedLaunchProcess for DesktopManagedProcess {
    fn process_id(&self) -> u32 {
        self.child.id()
    }

    fn try_wait(&mut self) -> Result<Option<LaunchProcessExit>, LaunchError> {
        self.child
            .try_wait()
            .map(|status| {
                status.map(|status| LaunchProcessExit {
                    exit_code: status.code(),
                })
            })
            .map_err(|error| LaunchError::adapter(format!("could not monitor process: {error}")))
    }

    fn terminate(&mut self) -> Result<(), LaunchError> {
        if self
            .child
            .try_wait()
            .map_err(|error| LaunchError::adapter(format!("could not inspect process: {error}")))?
            .is_some()
        {
            return Ok(());
        }
        self.child
            .kill()
            .map_err(|error| LaunchError::adapter(format!("could not stop process: {error}")))
    }

    fn wait(&mut self) -> Result<LaunchProcessExit, LaunchError> {
        self.child
            .wait()
            .map(|status| LaunchProcessExit {
                exit_code: status.code(),
            })
            .map_err(|error| LaunchError::adapter(format!("could not reap process: {error}")))
    }
}

#[derive(Clone)]
pub struct DesktopLaunchExecutor {
    wine_binary: String,
    wine32_binary: Option<String>,
    gamescope_binary: String,
    locale: Option<LocaleRemulator>,
    magpie: Option<Magpie>,
}

impl DesktopLaunchExecutor {
    pub fn new() -> Self {
        let wine_binary = configured_program("DLA_WINE_BINARY", "wine");
        Self {
            wine32_binary: (wine_binary == "wine").then(|| "wine32".to_owned()),
            wine_binary,
            gamescope_binary: gamescope_binary(),
            locale: None,
            magpie: None,
        }
    }

    /// Runs the works the player asked for in a Japanese locale through
    /// Locale Remulator, on Windows.
    pub fn with_locale_remulator(mut self, locale: LocaleRemulator) -> Self {
        self.locale = Some(locale);
        self
    }

    /// Shows the works the player gave a Resolution enlarged through Magpie,
    /// on Windows.
    pub fn with_magpie(mut self, magpie: Magpie) -> Self {
        self.magpie = Some(magpie);
        self
    }

    #[cfg(test)]
    fn with_wine_binary(wine_binary: impl Into<String>) -> Self {
        Self {
            wine_binary: wine_binary.into(),
            wine32_binary: None,
            gamescope_binary: "gamescope".to_owned(),
            locale: None,
            magpie: None,
        }
    }
}

impl DesktopLaunchExecutor {
    /// A 32-bit game goes to the 32-bit Wine loader when the system has one
    /// beside `wine` (Ubuntu's `wine32`). `wine` there is the 64-bit loader,
    /// which runs a 32-bit game through Wine's new WoW64 mode, where its
    /// OpenGL Direct3D drew a WOLF RPG Editor game at one frame a second.
    fn wine_program(&self, target: &Path) -> &str {
        match &self.wine32_binary {
            Some(wine32) if is_32_bit_windows_program(target) && program_found(wine32) => wine32,
            _ => &self.wine_binary,
        }
    }

    fn command(
        &self,
        adapter: LaunchAdapter,
        target: &Path,
        enlargement: Option<Enlargement>,
        session: DisplaySession,
    ) -> Command {
        let (program, arguments): (&OsStr, Vec<&OsStr>) = match adapter {
            LaunchAdapter::WindowsNative | LaunchAdapter::LinuxNative => {
                (target.as_os_str(), vec![])
            }
            LaunchAdapter::LinuxWine => (
                OsStr::new(self.wine_program(target)),
                vec![target.as_os_str()],
            ),
        };
        let mut command = match enlargement {
            // gamescope gives the game a screen of its own and draws that
            // screen enlarged on the monitor, so a game that only knows how
            // to open a 640x480 window fills a 4K screen.
            Some(enlargement) if adapter != LaunchAdapter::WindowsNative => {
                // Without mangoapp gamescope would start the game with no
                // overlay and an error in its log.
                let overlay = enlargement.performance_overlay && performance_overlay_available();
                let mut command = Command::new(&self.gamescope_binary);
                if overlay && std::env::var_os("MANGOHUD_CONFIG").is_none() {
                    command.env("MANGOHUD_CONFIG", OVERLAY_CONFIG);
                }
                command
                    .args(gamescope_arguments(Enlargement {
                        performance_overlay: overlay,
                        ..enlargement
                    }))
                    .arg("--")
                    .args(["sh", "-c", GAME_SUPERVISOR])
                    .arg(program)
                    .args(&arguments);
                // gamescope's window is a Wayland window through SDL: libdecor
                // gives it a title bar with a close button (its cairo plugin;
                // GTK's cannot start inside gamescope) and the desktop gets
                // complete frames, where gamescope's X11 window on Xwayland
                // was sampled mid-copy and tore on every scroll (2026-11-08).
                // SDL's mode scaling stays off: with it the decorated window
                // never settled its size under fractional scaling.
                match session {
                    DisplaySession::Wayland => {
                        command
                            .env("SDL_VIDEODRIVER", "wayland")
                            .env("SDL_VIDEO_WAYLAND_SCALE_TO_DISPLAY", "0")
                            .env("SDL_VIDEO_WAYLAND_MODE_SCALING", "none");
                    }
                    DisplaySession::X11 => {
                        command.env("SDL_VIDEODRIVER", "x11");
                    }
                }
                #[cfg(unix)]
                {
                    use std::os::unix::process::CommandExt;
                    // gamescope 3.16 aborts while shutting down once its window
                    // is closed (a thread left joinable), and Ubuntu's crash
                    // reporter would show that to the player after every game.
                    // A core limit of exactly 1 byte makes the kernel skip the
                    // piped crash handler, so the known crash at the end stays
                    // silent. SAFETY: setrlimit is async-signal-safe and
                    // touches no Rust state.
                    unsafe {
                        command.pre_exec(|| {
                            let limit = libc::rlimit {
                                rlim_cur: 1,
                                rlim_max: 1,
                            };
                            libc::setrlimit(libc::RLIMIT_CORE, &limit);
                            Ok(())
                        });
                    }
                }
                command
            }
            _ => {
                let mut command = Command::new(program);
                command.args(&arguments);
                command
            }
        };
        if adapter == LaunchAdapter::LinuxWine {
            // Japanese games expect Japanese Windows: Wine takes the code
            // page for their file names and text from the locale, as
            // Locale Emulator or DLsite Nest set it on Windows.
            let locale = wine_locale();
            command.env("LANG", locale).env("LC_ALL", locale);
        }
        command
    }
}

/// What the player chose for a game shown through gamescope.
#[derive(Clone, Copy, Debug)]
struct Enlargement {
    scale: WindowScale,
    refresh_rate: Option<u32>,
    picture_filter: Option<PictureFilter>,
    performance_overlay: bool,
    cursor_hide_delay: Option<u32>,
}

impl Enlargement {
    fn of(plan: &LaunchExecutionPlan) -> Option<Self> {
        plan.window_scale.map(|scale| Self {
            scale,
            refresh_rate: plan.refresh_rate,
            picture_filter: plan.picture_filter,
            performance_overlay: plan.performance_overlay,
            cursor_hide_delay: plan.cursor_hide_delay,
        })
    }
}

/// Full screen with the picture fitted to the monitor, cropped to cover it or
/// stretched over it, full screen enlarged by whole numbers so every game
/// pixel keeps one size, or a window of the chosen size with the picture
/// fitted in. The picture keeps hard pixels unless the player chose another
/// filter; whole numbers always do. gamescope's pixel-art filter is a choice
/// and not the default: at full screen on integrated graphics it drew 20
/// frames a second where hard pixels drew 60 (measured 2026-11-11).
fn gamescope_arguments(enlargement: Enlargement) -> Vec<String> {
    let Enlargement {
        scale,
        refresh_rate,
        picture_filter,
        performance_overlay,
        cursor_hide_delay,
    } = enlargement;
    // The game paces itself by gamescope's nested display: at 120 Hz, with
    // every new frame shown at once, a 60 fps game no longer judders
    // against gamescope's own 60 Hz tick; 60 Hz stays a choice for games
    // that misbehave at the higher rate.
    let rate = refresh_rate.unwrap_or(DEFAULT_REFRESH_RATE).to_string();
    let pacing = ["--backend", "sdl", "-r", rate.as_str(), "--immediate-flips"];
    let (output, scaler): (Vec<String>, &str) = match scale {
        WindowScale::Fit => (vec!["-f".to_owned()], "fit"),
        WindowScale::Fill => (vec!["-f".to_owned()], "fill"),
        WindowScale::Stretch => (vec!["-f".to_owned()], "stretch"),
        WindowScale::Integer => (vec!["-f".to_owned()], "integer"),
        WindowScale::Window { width, height } => (
            vec![
                "-W".to_owned(),
                width.to_string(),
                "-H".to_owned(),
                height.to_string(),
            ],
            "fit",
        ),
    };
    let (filter, sharpness) = match (scale, picture_filter) {
        (WindowScale::Integer, _) => ("nearest", None),
        (_, None) => ("nearest", None),
        (_, Some(PictureFilter::EvenPixels)) => ("pixel", None),
        (_, Some(PictureFilter::Smooth)) => ("linear", None),
        (_, Some(PictureFilter::Fsr(sharpness))) => ("fsr", Some(sharpness)),
        (_, Some(PictureFilter::Nis(sharpness))) => ("nis", Some(sharpness)),
    };
    pacing
        .map(str::to_owned)
        .into_iter()
        .chain(output)
        .chain(["-S", scaler, "-F", filter].map(str::to_owned))
        .chain(
            sharpness
                .into_iter()
                .flat_map(|sharpness| ["--sharpness", gamescope_sharpness(sharpness)])
                .map(str::to_owned),
        )
        .chain(performance_overlay.then(|| "--mangoapp".to_owned()))
        .chain(
            cursor_hide_delay
                .into_iter()
                .flat_map(|delay| ["--hide-cursor-delay".to_owned(), delay.to_string()]),
        )
        .collect()
}

/// gamescope counts sharpness from 0, the most, to 20, the least.
fn gamescope_sharpness(sharpness: Sharpness) -> &'static str {
    match sharpness {
        Sharpness::Strong => "0",
        Sharpness::Medium => "5",
        Sharpness::Soft => "10",
    }
}

const DEFAULT_REFRESH_RATE: u32 = 120;

/// The desktop gamescope opens its window on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DisplaySession {
    Wayland,
    X11,
}

fn display_session() -> DisplaySession {
    if std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty()) {
        DisplaySession::Wayland
    } else {
        DisplaySession::X11
    }
}

/// Runs the game inside gamescope. gamescope ends only once every process of
/// its tree has gone, and Wine leaves helpers behind that outlive the game
/// (`winedevice.exe`, in a session of its own), which left gamescope and the
/// launcher's activity running after the game had closed (2026-11-08). So the
/// game runs in a systemd scope of its own, stopped as soon as the game ends;
/// without systemd it runs as it is. When gamescope's window is closed, its
/// reaper sends the supervisor SIGTERM while the game is still dying: the trap
/// keeps the shell alive until the game has gone, so the scope is still
/// stopped (a bare `trap ''` would be inherited by the game and ignored there).
const GAME_SUPERVISOR: &str = r#"unit="dla-game-$$"
if systemd-run --user --scope --quiet --collect --unit "dla-probe-$$" true 2>/dev/null; then
  trap 'true' TERM INT HUP
  systemd-run --user --scope --quiet --collect --unit "$unit" -p TimeoutStopSec=5s -- "$0" "$@"
  status=$?
  systemctl --user stop "$unit.scope" 2>/dev/null
else
  "$0" "$@"
  status=$?
fi
exit $status
"#;

fn configured_program(variable: &str, default: &str) -> String {
    std::env::var(variable)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default.to_owned())
}

fn gamescope_binary() -> String {
    configured_program("DLA_GAMESCOPE_BINARY", "gamescope")
}

/// The overlay goes top right, where MangoHud alone would put it top left.
/// `read_cfg` keeps the player's own MangoHud file in force for the rest.
const OVERLAY_CONFIG: &str = "read_cfg,position=top-right";

/// Whether gamescope can draw the performance overlay: Linux with MangoHud's
/// `mangoapp`, which gamescope starts itself.
pub fn performance_overlay_available() -> bool {
    cfg!(target_os = "linux") && program_found("mangoapp")
}

/// The program that shows a game's window enlarged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enlarger {
    Gamescope,
    Magpie,
}

impl Enlarger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gamescope => "gamescope",
            Self::Magpie => "magpie",
        }
    }
}

/// What can enlarge a game's window on this computer: gamescope on Linux
/// where it is installed, Magpie on Windows, which the launcher fetches
/// itself.
pub fn enlarger() -> Option<Enlarger> {
    if cfg!(target_os = "windows") {
        Some(Enlarger::Magpie)
    } else if cfg!(target_os = "linux") && program_found(&gamescope_binary()) {
        Some(Enlarger::Gamescope)
    } else {
        None
    }
}

fn program_found(program: &str) -> bool {
    let path = Path::new(program);
    if path.components().count() > 1 {
        return path.is_file();
    }
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| directory.join(program).is_file())
    })
}

fn is_32_bit_windows_program(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};

    const I386: u16 = 0x014c;
    let read = || -> std::io::Result<bool> {
        let mut file = std::fs::File::open(path)?;
        let mut dos = [0u8; 0x40];
        file.read_exact(&mut dos)?;
        if &dos[..2] != b"MZ" {
            return Ok(false);
        }
        let pe = u32::from_le_bytes([dos[0x3c], dos[0x3d], dos[0x3e], dos[0x3f]]);
        file.seek(SeekFrom::Start(u64::from(pe)))?;
        let mut header = [0u8; 6];
        file.read_exact(&mut header)?;
        Ok(&header[..4] == b"PE\0\0" && u16::from_le_bytes([header[4], header[5]]) == I386)
    };
    read().unwrap_or(false)
}

fn wine_missing(wine_binary: &str) -> LaunchError {
    LaunchError::adapter(format!(
        "Wine is required to launch this Windows game, but '{wine_binary}' was not found"
    ))
}

const JAPANESE_LOCALE: &str = "ja_JP.UTF-8";
const FALLBACK_LOCALE: &str = "C.UTF-8";

/// The locale Wine runs a game in: Japanese when the system has it generated.
/// A locale the system lacks makes Wine fall back to "C", where it cannot
/// open paths with non-ASCII names at all, so C.UTF-8 keeps the game
/// startable then, with Windows' code page instead of Japan's.
fn wine_locale() -> &'static str {
    static LOCALE: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    LOCALE.get_or_init(|| {
        if locale_available(JAPANESE_LOCALE) {
            JAPANESE_LOCALE
        } else {
            FALLBACK_LOCALE
        }
    })
}

#[cfg(unix)]
fn locale_available(name: &str) -> bool {
    let Ok(name) = std::ffi::CString::new(name) else {
        return false;
    };
    // SAFETY: newlocale reads the C string and returns a handle, or null when
    // the locale is not installed; the handle is freed right away.
    unsafe {
        let locale = libc::newlocale(libc::LC_ALL_MASK, name.as_ptr(), std::ptr::null_mut());
        if locale.is_null() {
            return false;
        }
        libc::freelocale(locale);
    }
    true
}

#[cfg(not(unix))]
fn locale_available(_name: &str) -> bool {
    true
}

impl Default for DesktopLaunchExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl LaunchExecutor for DesktopLaunchExecutor {
    fn execute(&self, plan: &LaunchExecutionPlan) -> Result<LaunchExecutionResult, LaunchError> {
        if plan.action != LaunchActionKind::LaunchExecutable {
            return Err(LaunchError::adapter(
                "the desktop executor accepts executable actions only",
            ));
        }
        let target = resolve_launch_target(
            &plan.root_path,
            &plan.relative_target,
            plan.expected_sha256.as_deref(),
        )?;
        let adapter = select_adapter(plan)?;
        #[cfg(target_os = "windows")]
        let (target, drive) = plain_drive::PlainDrive::for_game(target);
        let working_directory = target
            .parent()
            .ok_or_else(|| LaunchError::adapter("the executable has no valid working directory"))?;
        #[cfg(target_os = "windows")]
        if plan.japanese_locale
            && adapter == LaunchAdapter::WindowsNative
            && let Some(locale) = &self.locale
        {
            let process = start_in_japanese(locale, &target, working_directory)?;
            return Ok(LaunchExecutionResult {
                adapter,
                process: plain_drive::on_drive(self.enlarged_through_magpie(plan, process), drive),
            });
        }
        let enlarged = plan.window_scale.is_some() && adapter != LaunchAdapter::WindowsNative;
        let wine = self.wine_program(&target);
        if enlarged && adapter == LaunchAdapter::LinuxWine && !program_found(wine) {
            return Err(wine_missing(wine));
        }
        let start = {
            let executor = self.clone();
            let target = target.clone();
            let working_directory = working_directory.to_path_buf();
            let enlargement = Enlargement::of(plan);
            let session = display_session();
            move |watched: bool| {
                let mut command = executor.command(adapter, &target, enlargement, session);
                command.current_dir(&working_directory);
                if watched {
                    command.stderr(Stdio::piped());
                }
                command.spawn()
            }
        };
        let process: std::io::Result<Box<dyn ManagedLaunchProcess>> = if enlarged {
            enlarged::EnlargedProcess::start(Box::new(move || start(true)))
                .map(|process| Box::new(process) as Box<dyn ManagedLaunchProcess>)
        } else {
            start(false).map(|child| {
                Box::new(DesktopManagedProcess { child }) as Box<dyn ManagedLaunchProcess>
            })
        };
        let process = process.map_err(|error| {
            if error.kind() != std::io::ErrorKind::NotFound {
                LaunchError::adapter(format!("could not start {}: {error}", target.display()))
            } else if enlarged {
                LaunchError::adapter(format!(
                    "gamescope is required to enlarge this game's window, but '{}' was not found",
                    self.gamescope_binary
                ))
            } else if adapter == LaunchAdapter::LinuxWine {
                wine_missing(wine)
            } else {
                LaunchError::adapter(format!("could not start {}: {error}", target.display()))
            }
        })?;
        #[cfg(target_os = "windows")]
        let process = plain_drive::on_drive(self.enlarged_through_magpie(plan, process), drive);
        Ok(LaunchExecutionResult { adapter, process })
    }
}

#[cfg(target_os = "windows")]
impl DesktopLaunchExecutor {
    /// Magpie works on a window that is already there, so the game starts as
    /// it always does and is followed until its window opens.
    fn enlarged_through_magpie(
        &self,
        plan: &LaunchExecutionPlan,
        game: Box<dyn ManagedLaunchProcess>,
    ) -> Box<dyn ManagedLaunchProcess> {
        match (&self.magpie, plan.window_scale) {
            (Some(magpie), Some(scale)) => Box::new(magpie::windows::EnlargedGame::follow(
                game,
                magpie.clone(),
                magpie::settings::Shown {
                    scale,
                    picture_filter: plan.picture_filter,
                    cursor_hide_delay: plan.cursor_hide_delay,
                },
            )),
            _ => game,
        }
    }
}

/// Starts the game through LRProc, which ends once the game runs, and
/// follows the game itself.
#[cfg(target_os = "windows")]
fn start_in_japanese(
    locale: &LocaleRemulator,
    target: &Path,
    folder: &Path,
) -> Result<Box<dyn ManagedLaunchProcess>, LaunchError> {
    use std::time::{Duration, Instant};

    const LRPROC_LIMIT: Duration = Duration::from_secs(30);

    let missing = LocaleRemulator::missing_runtimes();
    if !missing.is_empty() {
        let names = missing
            .iter()
            .map(|runtime| runtime.as_str())
            .collect::<Vec<_>>()
            .join(" and ");
        return Err(LaunchError::adapter(format!(
            "Locale Remulator needs Microsoft's Visual C++ Redistributable ({names}), \
             which is not installed"
        )));
    }
    locale.install()?;
    let mut lrproc = locale.command(target, folder).spawn().map_err(|error| {
        LaunchError::adapter(format!("could not start Locale Remulator: {error}"))
    })?;
    let started = Instant::now();
    loop {
        match lrproc.try_wait() {
            Ok(Some(status)) if status.success() => break,
            // LRProc ends with 0 once it has started the game; a .NET
            // failure in it ends with 0xE0434352.
            Ok(Some(status)) => {
                return Err(LaunchError::adapter(format!(
                    "Locale Remulator failed before starting the game (exit code {:#010X})",
                    status.code().unwrap_or_default() as u32
                )));
            }
            Ok(None) if started.elapsed() < LRPROC_LIMIT => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = lrproc.kill();
                return Err(LaunchError::adapter(
                    "Locale Remulator did not start the game",
                ));
            }
            Err(error) => {
                return Err(LaunchError::adapter(format!(
                    "could not follow Locale Remulator: {error}"
                )));
            }
        }
    }
    Ok(Box::new(windows_process::GameProcess::started_by(
        lrproc.id(),
        target,
    )?))
}

fn resolve_launch_target(
    root_path: &str,
    relative_target: &RelativePath,
    expected_sha256: Option<&str>,
) -> Result<PathBuf, LaunchError> {
    let unresolved_root = Path::new(root_path);
    let root_metadata = fs::symlink_metadata(unresolved_root).map_err(|error| {
        LaunchError::adapter(format!("installation root is unavailable: {error}"))
    })?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err(LaunchError::adapter(
            "installation root must be a regular directory",
        ));
    }
    let canonical_root = unresolved_root
        .canonicalize()
        .map_err(|error| LaunchError::adapter(format!("installation root is invalid: {error}")))?;
    let unresolved_target = relative_target
        .as_str()
        .split('/')
        .fold(canonical_root.clone(), |path, segment| path.join(segment));
    reject_symlink_path(&canonical_root, relative_target)?;
    let target_metadata = fs::metadata(&unresolved_target)
        .map_err(|error| LaunchError::adapter(format!("launch target is unavailable: {error}")))?;
    if !target_metadata.is_file() {
        return Err(LaunchError::adapter("launch target is not a regular file"));
    }
    let canonical_target = resolve_within(&canonical_root, Path::new(relative_target.as_str()))
        .map_err(|error| LaunchError::adapter(format!("launch target is invalid: {error}")))?;
    if let Some(expected) = expected_sha256 {
        let actual = sha256_file(&canonical_target)?;
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(LaunchError::adapter(
                "launch target hash no longer matches the verified package",
            ));
        }
    }
    Ok(canonical_target)
}

fn reject_symlink_path(root: &Path, relative_target: &RelativePath) -> Result<(), LaunchError> {
    let mut current = root.to_path_buf();
    for segment in relative_target.as_str().split('/') {
        current.push(segment);
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            LaunchError::adapter(format!("launch target path is unavailable: {error}"))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(LaunchError::adapter(
                "launch target path must not contain symbolic links",
            ));
        }
    }
    Ok(())
}

fn select_adapter(plan: &LaunchExecutionPlan) -> Result<LaunchAdapter, LaunchError> {
    #[cfg(target_os = "windows")]
    {
        if plan
            .supported_platforms
            .contains(&InstallationPlatform::Windows)
        {
            return Ok(LaunchAdapter::WindowsNative);
        }
        return Err(LaunchError::adapter(
            "the selected executable does not support Windows",
        ));
    }

    #[cfg(target_os = "linux")]
    {
        let windows_executable = plan
            .relative_target
            .as_str()
            .to_ascii_lowercase()
            .ends_with(".exe")
            && plan
                .supported_platforms
                .contains(&InstallationPlatform::Windows);
        if windows_executable {
            return Ok(LaunchAdapter::LinuxWine);
        }
        if plan
            .supported_platforms
            .contains(&InstallationPlatform::Linux)
        {
            return Ok(LaunchAdapter::LinuxNative);
        }
        Err(LaunchError::adapter(
            "the selected executable is not supported on Linux",
        ))
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = plan;
        Err(LaunchError::adapter(
            "desktop executable launch is not supported on this platform",
        ))
    }
}

fn sha256_file(path: &Path) -> Result<String, LaunchError> {
    let mut file = File::open(path).map_err(|error| {
        LaunchError::adapter(format!("could not verify launch target: {error}"))
    })?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; HASH_BUFFER_BYTES];
    loop {
        let count = file.read(&mut buffer).map_err(|error| {
            LaunchError::adapter(format!("could not verify launch target: {error}"))
        })?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(hex::encode(digest.finalize()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    #[cfg(target_os = "linux")]
    use dla_domain::installation::{InstallationId, InstallationPlatform, LaunchActionKind};
    use tempfile::tempdir;

    use super::*;

    #[cfg(target_os = "linux")]
    fn plan(root: &Path, target: &str) -> LaunchExecutionPlan {
        LaunchExecutionPlan {
            installation_id: InstallationId("installation-1".to_owned()),
            root_path: root.to_string_lossy().into_owned(),
            action: LaunchActionKind::LaunchExecutable,
            relative_target: RelativePath::parse(target).expect("relative target"),
            supported_platforms: vec![InstallationPlatform::Windows],
            expected_sha256: None,
            japanese_locale: false,
            window_scale: None,
            refresh_rate: None,
            picture_filter: None,
            performance_overlay: false,
            cursor_hide_delay: None,
        }
    }

    #[test]
    fn resolves_a_regular_target_below_the_approved_root() {
        let directory = tempdir().expect("temporary directory");
        fs::create_dir(directory.path().join("game")).expect("game directory");
        fs::write(directory.path().join("game/Game.exe"), b"fixture").expect("fixture target");
        let target = RelativePath::parse("game/Game.exe").expect("target");

        let resolved = resolve_launch_target(
            directory.path().to_str().expect("root"),
            &target,
            Some("1e4ed5d2ea330d1a1ee2d2d2d2d6f11698b1629756d7a22cc2f2aa96c3a29c4c"),
        );

        assert!(matches!(resolved, Err(LaunchError::Adapter(_))));
        let resolved = resolve_launch_target(
            directory.path().to_str().expect("root"),
            &target,
            Some(&sha256_file(&directory.path().join("game/Game.exe")).expect("hash")),
        )
        .expect("resolved target");
        assert_eq!(
            resolved,
            directory
                .path()
                .join("game")
                .join("Game.exe")
                .canonicalize()
                .expect("canonical target")
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_symlink_even_when_it_points_back_inside_the_root() {
        let directory = tempdir().expect("temporary directory");
        fs::write(directory.path().join("Game.exe"), b"fixture").expect("fixture target");
        symlink("Game.exe", directory.path().join("Alias.exe")).expect("symlink");

        let error = resolve_launch_target(
            directory.path().to_str().expect("root"),
            &RelativePath::parse("Alias.exe").expect("target"),
            None,
        )
        .expect_err("symlink must fail");

        assert!(error.to_string().contains("symbolic links"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn routes_windows_executables_through_wine_on_linux() {
        let directory = tempdir().expect("temporary directory");
        let execution = plan(directory.path(), "Game.exe");
        assert_eq!(
            select_adapter(&execution).expect("adapter"),
            LaunchAdapter::LinuxWine
        );
        assert_eq!(
            DesktopLaunchExecutor::with_wine_binary("wine64").wine_binary,
            "wine64"
        );
    }

    fn windows_program(machine: u16) -> tempfile::NamedTempFile {
        use std::io::Write;

        let mut bytes = vec![0u8; 0x80];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        bytes[0x40..0x44].copy_from_slice(b"PE\0\0");
        bytes[0x44..0x46].copy_from_slice(&machine.to_le_bytes());
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&bytes).unwrap();
        file
    }

    #[test]
    fn sends_32_bit_games_to_the_32_bit_wine_loader_when_there_is_one() {
        let loader = tempfile::NamedTempFile::new().unwrap();
        let wine32 = loader.path().to_str().unwrap().to_owned();
        let mut executor = DesktopLaunchExecutor::with_wine_binary("wine");
        executor.wine32_binary = Some(wine32.clone());
        let game32 = windows_program(0x014c);
        let game64 = windows_program(0x8664);

        assert_eq!(executor.wine_program(game32.path()), wine32);
        assert_eq!(executor.wine_program(game64.path()), "wine");
        assert_eq!(
            executor.wine_program(Path::new("/games/Missing.exe")),
            "wine"
        );

        executor.wine32_binary = Some("/missing/wine32".to_owned());
        assert_eq!(executor.wine_program(game32.path()), "wine");
        executor.wine32_binary = None;
        assert_eq!(executor.wine_program(game32.path()), "wine");
    }

    #[test]
    fn runs_wine_games_in_japanese_and_leaves_other_games_alone() {
        let executor = DesktopLaunchExecutor::with_wine_binary("wine");
        let wine = executor.command(
            LaunchAdapter::LinuxWine,
            Path::new("/games/Game.exe"),
            None,
            DisplaySession::Wayland,
        );
        let settings = wine.get_envs().collect::<Vec<_>>();
        let locale = wine_locale();
        assert!([JAPANESE_LOCALE, FALLBACK_LOCALE].contains(&locale));
        for name in ["LANG", "LC_ALL"] {
            assert!(
                settings.contains(&(
                    std::ffi::OsStr::new(name),
                    Some(std::ffi::OsStr::new(locale))
                )),
                "{name}"
            );
        }
        let native = executor.command(
            LaunchAdapter::LinuxNative,
            Path::new("/games/game"),
            None,
            DisplaySession::Wayland,
        );
        assert_eq!(native.get_envs().count(), 0);
        assert_eq!(native.get_program(), "/games/game");
        assert_eq!(native.get_args().count(), 0);
    }

    #[test]
    fn enlarges_the_window_through_gamescope_when_asked() {
        let executor = DesktopLaunchExecutor::with_wine_binary("wine");
        let fit = executor.command(
            LaunchAdapter::LinuxWine,
            Path::new("/games/Game.exe"),
            Some(shown(WindowScale::Fit)),
            DisplaySession::Wayland,
        );
        assert_eq!(fit.get_program(), "gamescope");
        assert_eq!(
            fit.get_args().collect::<Vec<_>>(),
            [
                "--backend",
                "sdl",
                "-r",
                "120",
                "--immediate-flips",
                "-f",
                "-S",
                "fit",
                "-F",
                "nearest",
                "--",
                "sh",
                "-c",
                GAME_SUPERVISOR,
                "wine",
                "/games/Game.exe"
            ]
        );
        let settings = fit.get_envs().collect::<Vec<_>>();
        for (name, value) in [
            ("LANG", wine_locale()),
            ("SDL_VIDEODRIVER", "wayland"),
            ("SDL_VIDEO_WAYLAND_MODE_SCALING", "none"),
        ] {
            assert!(
                settings.contains(&(OsStr::new(name), Some(OsStr::new(value)))),
                "{name}"
            );
        }

        let integer = executor.command(
            LaunchAdapter::LinuxNative,
            Path::new("/games/game"),
            Some(Enlargement {
                refresh_rate: Some(60),
                ..shown(WindowScale::Integer)
            }),
            DisplaySession::X11,
        );
        assert_eq!(
            integer.get_args().collect::<Vec<_>>(),
            [
                "--backend",
                "sdl",
                "-r",
                "60",
                "--immediate-flips",
                "-f",
                "-S",
                "integer",
                "-F",
                "nearest",
                "--",
                "sh",
                "-c",
                GAME_SUPERVISOR,
                "/games/game"
            ]
        );
        assert!(integer.get_envs().any(|(name, value)| {
            name == "SDL_VIDEODRIVER" && value == Some(OsStr::new("x11"))
        }));

        let sized = executor.command(
            LaunchAdapter::LinuxWine,
            Path::new("/games/Game.exe"),
            Some(shown(WindowScale::Window {
                width: 1920,
                height: 1440,
            })),
            DisplaySession::Wayland,
        );
        assert_eq!(
            sized.get_args().take(9).collect::<Vec<_>>(),
            [
                "--backend",
                "sdl",
                "-r",
                "120",
                "--immediate-flips",
                "-W",
                "1920",
                "-H",
                "1440"
            ]
        );

        let windows = executor.command(
            LaunchAdapter::WindowsNative,
            Path::new("C:/games/Game.exe"),
            Some(shown(WindowScale::Fit)),
            DisplaySession::Wayland,
        );
        assert_eq!(windows.get_program(), "C:/games/Game.exe");
    }

    #[test]
    fn draws_the_enlarged_picture_the_way_the_player_chose() {
        let arguments = |scale, picture_filter| {
            gamescope_arguments(Enlargement {
                picture_filter,
                ..shown(scale)
            })
            .into_iter()
            .skip(5)
            .collect::<Vec<_>>()
        };
        assert_eq!(
            arguments(WindowScale::Fill, None),
            ["-f", "-S", "fill", "-F", "nearest"]
        );
        assert_eq!(
            arguments(WindowScale::Fit, Some(PictureFilter::EvenPixels)),
            ["-f", "-S", "fit", "-F", "pixel"]
        );
        assert_eq!(
            arguments(WindowScale::Stretch, Some(PictureFilter::Smooth)),
            ["-f", "-S", "stretch", "-F", "linear"]
        );
        assert_eq!(
            arguments(
                WindowScale::Fit,
                Some(PictureFilter::Fsr(Sharpness::Strong))
            ),
            ["-f", "-S", "fit", "-F", "fsr", "--sharpness", "0"]
        );
        assert_eq!(
            arguments(
                WindowScale::Window {
                    width: 1280,
                    height: 960
                },
                Some(PictureFilter::Nis(Sharpness::Soft))
            ),
            [
                "-W",
                "1280",
                "-H",
                "960",
                "-S",
                "fit",
                "-F",
                "nis",
                "--sharpness",
                "10"
            ]
        );
        // Whole steps keep hard pixels whatever was chosen.
        assert_eq!(
            arguments(
                WindowScale::Integer,
                Some(PictureFilter::Fsr(Sharpness::Medium))
            ),
            ["-f", "-S", "integer", "-F", "nearest"]
        );
    }

    #[test]
    fn adds_the_performance_overlay_and_the_cursor_delay_when_chosen() {
        let plain = gamescope_arguments(shown(WindowScale::Fit));
        assert!(!plain.iter().any(|argument| argument == "--mangoapp"));
        assert!(
            !plain
                .iter()
                .any(|argument| argument == "--hide-cursor-delay")
        );

        let chosen = gamescope_arguments(Enlargement {
            performance_overlay: true,
            cursor_hide_delay: Some(3000),
            ..shown(WindowScale::Fit)
        });
        assert_eq!(
            chosen[chosen.len() - 3..],
            ["--mangoapp", "--hide-cursor-delay", "3000"]
        );
    }

    fn shown(scale: WindowScale) -> Enlargement {
        Enlargement {
            scale,
            refresh_rate: None,
            picture_filter: None,
            performance_overlay: false,
            cursor_hide_delay: None,
        }
    }
}
