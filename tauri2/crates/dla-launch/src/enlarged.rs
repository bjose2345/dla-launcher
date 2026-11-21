//! A game started inside gamescope, started again when gamescope's window
//! fails to open.
//!
//! gamescope 3.16 creates its window hidden and shows it from one thread
//! while another already presents the game's first frame. When the frame
//! arrives first the desktop refuses the window (`wl_surface already has a
//! buffer committed`): the game runs unseen and gamescope rebuilds its
//! swapchain without end, which also starves the game. Measured on
//! 2026-11-11 at about one start in fifteen, more on a busy computer.
//! Nothing recovers such a start, so it is stopped and repeated.

use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Write},
    process::Child,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use dla_application::launch::{LaunchError, LaunchProcessExit, ManagedLaunchProcess};

const SWAPCHAIN_LINE: &str = "Creating Gamescope nested swapchain";
// A good start logs up to five of these; a stuck one about eighteen a second.
const STUCK_REBUILDS: usize = 20;
const STUCK_WITHIN: Duration = Duration::from_secs(3);
// Later bursts are the player resizing the window, which must never cost
// them a running game.
const OPENING: Duration = Duration::from_secs(45);
const RESTARTS: u8 = 2;
// The game's own processes end a moment after gamescope does.
const SETTLE: Duration = Duration::from_millis(1500);

type Start = Box<dyn FnMut() -> std::io::Result<Child> + Send>;

pub(crate) struct EnlargedProcess {
    start: Start,
    child: Child,
    // The activity is known by the first start's process.
    process_id: u32,
    stuck: Arc<AtomicBool>,
    restarts_left: u8,
    stopping: bool,
}

impl EnlargedProcess {
    /// `start` must give gamescope a piped standard error, which is read
    /// here and passed on.
    pub(crate) fn start(mut start: Start) -> std::io::Result<Self> {
        let mut child = start()?;
        Ok(Self {
            process_id: child.id(),
            stuck: watch(&mut child),
            child,
            start,
            restarts_left: RESTARTS,
            stopping: false,
        })
    }

    fn start_again(&mut self) -> Result<(), LaunchError> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        thread::sleep(SETTLE);
        self.restarts_left -= 1;
        let mut child = (self.start)().map_err(|error| {
            LaunchError::adapter(format!("could not start gamescope again: {error}"))
        })?;
        self.stuck = watch(&mut child);
        self.child = child;
        Ok(())
    }
}

impl ManagedLaunchProcess for EnlargedProcess {
    fn process_id(&self) -> u32 {
        self.process_id
    }

    fn try_wait(&mut self) -> Result<Option<LaunchProcessExit>, LaunchError> {
        if !self.stopping && self.restarts_left > 0 && self.stuck.load(Ordering::SeqCst) {
            self.start_again()?;
            return Ok(None);
        }
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
        self.stopping = true;
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

/// Reads gamescope's log on a thread of its own until gamescope ends, passes
/// every line on to the launcher's, and raises the flag once the window is
/// stuck.
fn watch(child: &mut Child) -> Arc<AtomicBool> {
    let stuck = Arc::new(AtomicBool::new(false));
    if let Some(log) = child.stderr.take() {
        let stuck = Arc::clone(&stuck);
        thread::spawn(move || {
            let mut window = StuckWindow::new(Instant::now());
            for line in BufReader::new(log).split(b'\n') {
                let Ok(line) = line else { break };
                let mut output = std::io::stderr().lock();
                let _ = output.write_all(&line);
                let _ = output.write_all(b"\n");
                if window.reads(&String::from_utf8_lossy(&line), Instant::now()) {
                    stuck.store(true, Ordering::SeqCst);
                }
            }
        });
    }
    stuck
}

struct StuckWindow {
    opened: Instant,
    rebuilds: VecDeque<Instant>,
}

impl StuckWindow {
    fn new(opened: Instant) -> Self {
        Self {
            opened,
            rebuilds: VecDeque::new(),
        }
    }

    /// Whether this line of gamescope's log shows the window stuck.
    fn reads(&mut self, line: &str, now: Instant) -> bool {
        if !line.contains(SWAPCHAIN_LINE) || now.duration_since(self.opened) > OPENING {
            return false;
        }
        self.rebuilds.push_back(now);
        while self
            .rebuilds
            .front()
            .is_some_and(|first| now.duration_since(*first) > STUCK_WITHIN)
        {
            self.rebuilds.pop_front();
        }
        self.rebuilds.len() >= STUCK_REBUILDS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REBUILD: &str = "[gamescope] [Info]  vulkan: Creating Gamescope nested swapchain with format 64 and colorspace 0";

    #[test]
    fn a_good_start_is_left_alone() {
        let opened = Instant::now();
        let mut window = StuckWindow::new(opened);
        for step in 0..5 {
            assert!(!window.reads(REBUILD, opened + Duration::from_millis(400 * step)));
        }
        assert!(!window.reads(
            "[gamescope] [Info]  xwm: got the same buffer committed twice, ignoring.",
            opened + Duration::from_secs(3)
        ));
    }

    #[test]
    fn a_window_that_keeps_rebuilding_while_it_opens_is_stuck() {
        let opened = Instant::now();
        let mut window = StuckWindow::new(opened);
        let stuck_at = (0..40).find(|step| {
            window.reads(
                REBUILD,
                opened + Duration::from_secs(2) + Duration::from_millis(55 * step),
            )
        });
        assert_eq!(stuck_at, Some(19));
    }

    #[test]
    fn slow_rebuilds_and_a_later_resize_are_not_a_stuck_window() {
        let opened = Instant::now();
        let mut window = StuckWindow::new(opened);
        for step in 0..40 {
            assert!(!window.reads(REBUILD, opened + Duration::from_millis(500 * step)));
        }

        let mut window = StuckWindow::new(opened);
        for step in 0..60 {
            assert!(!window.reads(
                REBUILD,
                opened + Duration::from_secs(46) + Duration::from_millis(20 * step)
            ));
        }
    }
}
