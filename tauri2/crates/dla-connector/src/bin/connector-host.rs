//! Test stand-in for the app: starts a connector around fake-cloudflared,
//! prints its local port and then idles, so a test can kill this process
//! outright and check that the connector does not outlive it.

use std::{env, io::Write, path::PathBuf, thread, time::Duration};

use dla_connector::{AccessToken, Connector, ConnectorConfig};

fn main() {
    let program = PathBuf::from(env::var_os("DLA_FAKE_CLOUDFLARED").expect("DLA_FAKE_CLOUDFLARED"));
    let connector = Connector::start(
        ConnectorConfig {
            program,
            hostname: "stay.fake".to_owned(),
            token: AccessToken::new("0123456789abcdef.access", "cfast_secretsecretsecret"),
        },
        |_| {},
    );
    let port = loop {
        if let Some(address) = connector.local_address() {
            break address.port();
        }
        thread::sleep(Duration::from_millis(50));
    };
    let mut stdout = std::io::stdout();
    let _ = writeln!(stdout, "{port}");
    let _ = stdout.flush();
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}
