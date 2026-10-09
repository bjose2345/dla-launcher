use std::{
    io::{BufRead, BufReader, Read},
    net::TcpStream,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use dla_connector::{AccessToken, Connector, ConnectorConfig, ConnectorState};

fn config(hostname: &str) -> ConnectorConfig {
    ConnectorConfig {
        program: PathBuf::from(env!("CARGO_BIN_EXE_fake-cloudflared")),
        hostname: hostname.to_owned(),
        token: AccessToken::new("0123456789abcdef.access", "cfast_secretsecretsecret"),
    }
}

fn recording() -> (
    Arc<Mutex<Vec<ConnectorState>>>,
    impl Fn(&ConnectorState) + Send + Sync + 'static,
) {
    let states = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&states);
    (states, move |state: &ConnectorState| {
        sink.lock().unwrap().push(state.clone())
    })
}

/// Generous on purpose: on Windows, real-time scanning holds the first start
/// of each freshly built helper program, and these tests start several at
/// once. A passing test returns as soon as the condition holds.
fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn exposes_a_local_listener_with_the_token_only_in_the_environment() {
    let (states, listener) = recording();
    let connector = Connector::start(config("stay.fake"), listener);

    wait_until("listening", || {
        matches!(connector.state(), ConnectorState::Listening { .. })
    });
    let address = connector.local_address().unwrap();
    assert!(address.ip().is_loopback());
    assert_eq!((connector.dial())(), Some(address));
    let mut reply = String::new();
    TcpStream::connect(address)
        .unwrap()
        .read_to_string(&mut reply)
        .unwrap();
    assert_eq!(reply, "ok\n");
    assert_eq!(
        states.lock().unwrap()[0],
        ConnectorState::Listening {
            port: address.port()
        }
    );
}

#[test]
fn restarts_after_the_connector_exits() {
    let (states, listener) = recording();
    let connector = Connector::start(config("exit-300.fake"), listener);

    wait_until("a restart", || {
        states
            .lock()
            .unwrap()
            .iter()
            .any(|state| matches!(state, ConnectorState::Restarting { attempt: 1, .. }))
    });
    let restarting = states
        .lock()
        .unwrap()
        .iter()
        .find_map(|state| match state {
            ConnectorState::Restarting {
                reason, seconds, ..
            } => Some((reason.clone(), *seconds)),
            _ => None,
        })
        .unwrap();
    assert!(
        restarting.0.contains("ERR failed to connect"),
        "{}",
        restarting.0
    );
    assert_eq!(restarting.1, 1);
    assert_eq!(connector.local_address(), None);
    wait_until("listening again", || {
        states
            .lock()
            .unwrap()
            .iter()
            .filter(|state| matches!(state, ConnectorState::Listening { .. }))
            .count()
            >= 2
    });
}

#[test]
fn stopping_ends_the_process_and_the_listener() {
    let (states, listener) = recording();
    let mut connector = Connector::start(config("stay.fake"), listener);
    wait_until("listening", || connector.local_address().is_some());
    let address = connector.local_address().unwrap();

    connector.stop();

    assert_eq!(connector.state(), ConnectorState::Stopped);
    assert_eq!(connector.local_address(), None);
    assert!(TcpStream::connect_timeout(&address, Duration::from_millis(300)).is_err());
    assert_eq!(
        states.lock().unwrap().last(),
        Some(&ConnectorState::Stopped)
    );
}

#[test]
fn a_missing_program_is_reported_and_retried() {
    let (states, listener) = recording();
    let mut config = config("stay.fake");
    config.program = PathBuf::from("/nonexistent/cloudflared");
    let connector = Connector::start(config, listener);

    wait_until("a restart", || {
        matches!(connector.state(), ConnectorState::Restarting { .. })
    });
    let ConnectorState::Restarting { reason, .. } = connector.state() else {
        unreachable!()
    };
    assert!(reason.contains("could not start"), "{reason}");
    drop(states);
}

/// Linux relies on PR_SET_PDEATHSIG and Windows on a kill-on-close job
/// object; either way no cleanup code runs when the app is killed.
#[test]
fn the_connector_dies_when_the_app_is_killed() {
    let mut host = Command::new(env!("CARGO_BIN_EXE_connector-host"))
        .env(
            "DLA_FAKE_CLOUDFLARED",
            env!("CARGO_BIN_EXE_fake-cloudflared"),
        )
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(host.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let port: u16 = line.trim().parse().expect("the host prints its port");
    assert!(TcpStream::connect(("127.0.0.1", port)).is_ok());

    host.kill().unwrap();
    host.wait().unwrap();

    wait_until("the connector to exit with the app", || {
        TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(200))
            .is_err()
    });
}
