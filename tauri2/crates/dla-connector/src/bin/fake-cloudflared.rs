//! Test stand-in for `cloudflared access tcp`. Behaviour is chosen by the
//! hostname: `stay.fake` keeps listening, `exit-<ms>.fake` exits with an ERR
//! line after that many milliseconds. It refuses to start if the service token
//! is missing from its environment or appears on its command line.

use std::{
    env,
    io::Write,
    net::TcpListener,
    process::exit,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn value(arguments: &[String], flag: &str) -> String {
    arguments
        .iter()
        .position(|argument| argument == flag)
        .and_then(|index| arguments.get(index + 1))
        .cloned()
        .unwrap_or_default()
}

fn main() {
    let arguments: Vec<String> = env::args().collect();
    let (Ok(id), Ok(secret)) = (
        env::var("TUNNEL_SERVICE_TOKEN_ID"),
        env::var("TUNNEL_SERVICE_TOKEN_SECRET"),
    ) else {
        eprintln!("ERR service token missing from the environment");
        exit(4);
    };
    if arguments
        .iter()
        .any(|argument| argument.contains(&id) || argument.contains(&secret))
    {
        eprintln!("ERR service token passed on the command line");
        exit(3);
    }
    if env::var("PATH").is_ok_and(|path| !path.is_empty()) {
        eprintln!("ERR PATH would allow a browser fallback");
        exit(5);
    }
    let url = value(&arguments, "--url");
    let hostname = value(&arguments, "--hostname");
    let listener = TcpListener::bind(&url).unwrap_or_else(|_| exit(6));
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    eprintln!("{stamp} INF Start Websocket listener host={url}");
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.write_all(b"ok\n");
        }
    });
    match hostname
        .strip_prefix("exit-")
        .and_then(|rest| rest.strip_suffix(".fake"))
    {
        Some(milliseconds) => {
            thread::sleep(Duration::from_millis(milliseconds.parse().unwrap_or(300)));
            eprintln!("{stamp} ERR failed to connect to the Cloudflare edge");
            exit(1);
        }
        None => loop {
            thread::sleep(Duration::from_secs(1));
        },
    }
}
