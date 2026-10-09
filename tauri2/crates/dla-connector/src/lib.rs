use std::{
    collections::VecDeque,
    fmt,
    io::{BufRead, BufReader, Read},
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU16, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde::Serialize;
use zeroize::Zeroizing;

#[cfg(windows)]
mod windows_job;

const BACKOFF_SECONDS: [u64; 5] = [1, 2, 5, 10, 30];
const START_TIMEOUT: Duration = Duration::from_secs(15);
const LISTENING_MARK: &str = "Start Websocket listener";
const LOG_LINES: usize = 20;

#[derive(Clone)]
pub struct AccessToken {
    client_id: Zeroizing<String>,
    client_secret: Zeroizing<String>,
}

impl AccessToken {
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self {
            client_id: Zeroizing::new(client_id.into()),
            client_secret: Zeroizing::new(client_secret.into()),
        }
    }

    fn appears_in(&self, line: &str) -> bool {
        line.contains(self.client_id.as_str()) || line.contains(self.client_secret.as_str())
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccessToken(<hidden>)")
    }
}

#[derive(Debug)]
pub struct ConnectorConfig {
    pub program: PathBuf,
    pub hostname: String,
    pub token: AccessToken,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConnectorState {
    Starting,
    Listening {
        port: u16,
    },
    Restarting {
        attempt: u32,
        seconds: u64,
        reason: String,
    },
    Stopped,
}

type Listener = Box<dyn Fn(&ConnectorState) + Send + Sync>;

struct Shared {
    port: AtomicU16,
    stop: AtomicBool,
    child: Mutex<Option<Child>>,
    state: Mutex<ConnectorState>,
    listener: Listener,
    /// Kills the connector if the app dies without stopping it.
    #[cfg(windows)]
    job: Option<windows_job::Job>,
}

impl Shared {
    fn set(&self, state: ConnectorState) {
        *self.state.lock().expect("connector state lock") = state.clone();
        (self.listener)(&state);
    }

    fn stopping(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

/// Keeps one `cloudflared access tcp` process listening on a loopback port and
/// restarts it with backoff. The service token reaches the process only through
/// its environment, never its command line.
pub struct Connector {
    shared: Arc<Shared>,
    supervisor: Option<JoinHandle<()>>,
}

impl Connector {
    pub fn start(
        config: ConnectorConfig,
        listener: impl Fn(&ConnectorState) + Send + Sync + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            port: AtomicU16::new(0),
            stop: AtomicBool::new(false),
            child: Mutex::new(None),
            state: Mutex::new(ConnectorState::Starting),
            listener: Box::new(listener),
            #[cfg(windows)]
            job: windows_job::Job::new().ok(),
        });
        let supervisor = {
            let shared = Arc::clone(&shared);
            thread::spawn(move || supervise(&shared, &config))
        };
        Self {
            shared,
            supervisor: Some(supervisor),
        }
    }

    pub fn local_address(&self) -> Option<SocketAddr> {
        local_address(&self.shared)
    }

    pub fn dial(&self) -> Arc<dyn Fn() -> Option<SocketAddr> + Send + Sync> {
        let shared = Arc::clone(&self.shared);
        Arc::new(move || local_address(&shared))
    }

    pub fn state(&self) -> ConnectorState {
        self.shared
            .state
            .lock()
            .expect("connector state lock")
            .clone()
    }

    pub fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        kill(&self.shared);
        if let Some(supervisor) = self.supervisor.take() {
            let _ = supervisor.join();
        }
    }
}

impl Drop for Connector {
    fn drop(&mut self) {
        self.stop();
    }
}

fn local_address(shared: &Shared) -> Option<SocketAddr> {
    match shared.port.load(Ordering::SeqCst) {
        0 => None,
        port => Some(SocketAddr::from(([127, 0, 0, 1], port))),
    }
}

fn kill(shared: &Shared) {
    if let Some(mut child) = shared.child.lock().expect("connector child lock").take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn supervise(shared: &Shared, config: &ConnectorConfig) {
    let mut attempt: u32 = 0;
    while !shared.stopping() {
        let reason = run_once(shared, config, &mut attempt);
        shared.port.store(0, Ordering::SeqCst);
        if shared.stopping() {
            break;
        }
        let seconds = BACKOFF_SECONDS[(attempt as usize).min(BACKOFF_SECONDS.len() - 1)];
        attempt += 1;
        shared.set(ConnectorState::Restarting {
            attempt,
            seconds,
            reason,
        });
        let until = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < until && !shared.stopping() {
            thread::sleep(Duration::from_millis(100));
        }
    }
    shared.set(ConnectorState::Stopped);
}

/// Runs one connector process until it exits; returns why it stopped.
fn run_once(shared: &Shared, config: &ConnectorConfig, attempt: &mut u32) -> String {
    let port = match free_port() {
        Ok(port) => port,
        Err(error) => return format!("no free local port: {error}"),
    };
    let mut child = match command(config, port).spawn() {
        Ok(child) => child,
        Err(error) => return format!("the private connector could not start: {error}"),
    };
    #[cfg(windows)]
    match windows_job::release(shared.job.as_ref(), &child) {
        Ok(true) => {}
        // Running, but it could outlive a crash of the app.
        Ok(false) => eprintln!("DLA Collab: the private connector is not tied to the app"),
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return format!("the private connector could not start: {error}");
        }
    }
    let log = Arc::new(Mutex::new(VecDeque::new()));
    let (marked, listening) = mpsc::channel();
    for stream in [
        child
            .stdout
            .take()
            .map(|out| Box::new(out) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|err| Box::new(err) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        drain(stream, Arc::clone(&log), marked.clone(), &config.token);
    }
    *shared.child.lock().expect("connector child lock") = Some(child);
    if shared.stopping() {
        kill(shared);
        return "stopped".into();
    }

    if wait_listening(shared, port, &listening) {
        *attempt = 0;
        shared.port.store(port, Ordering::SeqCst);
        shared.set(ConnectorState::Listening { port });
    }
    let status = loop {
        let mut guard = shared.child.lock().expect("connector child lock");
        let Some(child) = guard.as_mut() else {
            break None;
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                guard.take();
                break Some(status);
            }
            Ok(None) => {}
            Err(_) => break None,
        }
        drop(guard);
        thread::sleep(Duration::from_millis(200));
    };
    let last = log
        .lock()
        .expect("connector log lock")
        .iter()
        .rev()
        .find(|line| line.contains(" ERR "))
        .cloned();
    match (last, status) {
        (Some(line), _) => line,
        (None, Some(status)) => format!("the private connector exited ({status})"),
        (None, None) => "the private connector stopped".into(),
    }
}

fn wait_listening(shared: &Shared, port: u16, listening: &mpsc::Receiver<()>) -> bool {
    let deadline = Instant::now() + START_TIMEOUT;
    while Instant::now() < deadline && !shared.stopping() {
        if listening.recv_timeout(Duration::from_millis(100)).is_ok() {
            return true;
        }
        let exited = shared
            .child
            .lock()
            .expect("connector child lock")
            .as_mut()
            .is_none_or(|child| !matches!(child.try_wait(), Ok(None)));
        if exited {
            return false;
        }
    }
    // Older releases may word the start-up line differently; a listening
    // socket is the authoritative signal.
    TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_secs(1),
    )
    .is_ok()
}

fn drain(
    stream: Box<dyn Read + Send>,
    log: Arc<Mutex<VecDeque<String>>>,
    listening: mpsc::Sender<()>,
    token: &AccessToken,
) {
    let secrets = AccessToken::new(token.client_id.as_str(), token.client_secret.as_str());
    thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            if line.contains(LISTENING_MARK) {
                let _ = listening.send(());
            }
            let line = if secrets.appears_in(&line) {
                "<a line containing the access token was hidden>".to_owned()
            } else {
                line
            };
            let mut log = log.lock().expect("connector log lock");
            if log.len() == LOG_LINES {
                log.pop_front();
            }
            log.push_back(line);
        }
    });
}

fn command(config: &ConnectorConfig, port: u16) -> Command {
    let mut command = Command::new(&config.program);
    command
        .args([
            "access",
            "tcp",
            "--hostname",
            &config.hostname,
            "--url",
            &format!("127.0.0.1:{port}"),
        ])
        .env("TUNNEL_SERVICE_TOKEN_ID", config.token.client_id.as_str())
        .env(
            "TUNNEL_SERVICE_TOKEN_SECRET",
            config.token.client_secret.as_str(),
        )
        .env("NO_AUTOUPDATE", "true")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Without a usable token cloudflared falls back to opening a browser login;
    // an empty PATH keeps that from ever happening on the contributor's desktop
    // (it finds neither xdg-open nor rundll32).
    command.env("PATH", "");
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: prctl is async-signal-safe and touches no Rust state. The
        // connector dies with the app instead of outliving a crash.
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // Suspended until it has joined the app's job object.
        command.creation_flags(CREATE_NO_WINDOW | windows_job::CREATE_SUSPENDED);
    }
    command
}

fn free_port() -> std::io::Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}
