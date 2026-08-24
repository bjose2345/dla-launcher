use std::{
    ffi::OsString,
    sync::{Arc, Condvar, Mutex},
};

use tauri::{AppHandle, Manager};
#[cfg(any(desktop, target_os = "android"))]
use tauri::{Emitter, Runtime};

const READ_ONLY_DEEP_LINK_EVENT: &str = "read-only-deep-link";
const WORK_LINK_PREFIX: &str = "dla-launcher://works/";

#[derive(Clone)]
pub struct ReadOnlyNavigationState {
    inner: Arc<ReadOnlyNavigationInner>,
}

struct ReadOnlyNavigationInner {
    current: Mutex<Vec<String>>,
    ready: Condvar,
    runtime_ready: Mutex<bool>,
}

impl ReadOnlyNavigationState {
    pub fn from_cli_arguments(arguments: &[OsString]) -> Self {
        Self::new(validated_os_cli_link(arguments).into_iter().collect())
    }

    fn new(current: Vec<String>) -> Self {
        Self {
            inner: Arc::new(ReadOnlyNavigationInner {
                current: Mutex::new(current),
                ready: Condvar::new(),
                runtime_ready: Mutex::new(false),
            }),
        }
    }

    fn replace(&self, links: Vec<String>) {
        *self
            .inner
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = links;
    }

    fn signal_runtime_ready(&self) {
        *self
            .inner
            .runtime_ready
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
        self.inner.ready.notify_all();
    }

    fn read_when_runtime_ready(&self) -> Vec<String> {
        let mut runtime_ready = self
            .inner
            .runtime_ready
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while !*runtime_ready {
            runtime_ready = self
                .inner
                .ready
                .wait(runtime_ready)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        self.inner
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

#[tauri::command]
pub async fn read_current_read_only_deep_links(app: AppHandle) -> Result<Vec<String>, String> {
    let state = app.state::<ReadOnlyNavigationState>().inner().clone();
    match tauri::async_runtime::spawn_blocking(move || state.read_when_runtime_ready()).await {
        Ok(links) => {
            log::info!(
                target: "dla::navigation",
                "event=read_only_deep_link_frontend_read count={}",
                links.len()
            );
            Ok(links)
        }
        Err(error) => {
            log::warn!(target: "dla::navigation", "event=deep_link_runtime_wait_failed error={error}");
            Err(format!("deep-link runtime readiness failed: {error}"))
        }
    }
}

#[cfg(desktop)]
pub fn deliver_cli_arguments<R: Runtime>(app: &AppHandle<R>, arguments: &[String]) {
    if let Some(link) = validated_cli_link(arguments) {
        deliver(app, vec![link]);
    }
}

#[cfg(any(target_os = "android", target_os = "macos"))]
pub fn install_open_url_listener(app: &tauri::App) {
    use tauri_plugin_deep_link::DeepLinkExt;

    let handle = app.handle().clone();
    app.deep_link().on_open_url(move |event| {
        if let Some(link) = validated_url_link(event.urls().iter().map(|url| url.as_str())) {
            deliver(&handle, vec![link]);
        }
    });

    #[cfg(target_os = "android")]
    match app.deep_link().get_current() {
        Ok(Some(urls)) => {
            if let Some(link) = validated_url_link(urls.iter().map(|url| url.as_str())) {
                deliver(app.handle(), vec![link]);
            }
        }
        Ok(None) => {}
        Err(error) => {
            log::warn!(target: "dla::navigation", "event=deep_link_initial_read_failed error={error}");
        }
    }
}

pub fn signal_runtime_ready(app: &AppHandle) {
    app.state::<ReadOnlyNavigationState>()
        .signal_runtime_ready();
}

#[cfg(any(desktop, target_os = "android"))]
fn deliver<R: Runtime>(app: &AppHandle<R>, links: Vec<String>) {
    log::info!(
        target: "dla::navigation",
        "event=read_only_deep_link_delivered count={}",
        links.len()
    );
    app.state::<ReadOnlyNavigationState>()
        .replace(links.clone());
    if let Err(error) = app.emit(READ_ONLY_DEEP_LINK_EVENT, links) {
        log::warn!(target: "dla::navigation", "event=deep_link_delivery_failed error={error}");
    }
}

#[cfg(any(test, target_os = "android", target_os = "macos"))]
fn validated_url_link<I, S>(values: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut values = values.into_iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    parse_exact_work_link(value.as_ref())
}

#[cfg(any(test, desktop))]
fn validated_cli_link(arguments: &[String]) -> Option<String> {
    let [_, value] = arguments else {
        return None;
    };
    parse_exact_work_link(value)
}

fn validated_os_cli_link(arguments: &[OsString]) -> Option<String> {
    let [_, value] = arguments else {
        return None;
    };
    parse_exact_work_link(value.to_str()?)
}

fn parse_exact_work_link(value: &str) -> Option<String> {
    let route_prefix = value.get(..WORK_LINK_PREFIX.len())?;
    if !route_prefix.eq_ignore_ascii_case(WORK_LINK_PREFIX) {
        return None;
    }

    let code = value.get(WORK_LINK_PREFIX.len()..)?;
    let (prefix, digits) = code.split_at_checked(2)?;
    if !(prefix.eq_ignore_ascii_case("RJ")
        || prefix.eq_ignore_ascii_case("BJ")
        || prefix.eq_ignore_ascii_case("VJ"))
        || !(5..=10).contains(&digits.len())
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }

    Some(format!(
        "{WORK_LINK_PREFIX}{}{}",
        prefix.to_ascii_uppercase(),
        digits
    ))
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::ffi::OsString;
    use std::{sync::mpsc, time::Duration};

    use super::{
        ReadOnlyNavigationState, parse_exact_work_link, validated_cli_link, validated_os_cli_link,
        validated_url_link,
    };

    #[test]
    fn current_links_wait_for_runtime_readiness() {
        let state = ReadOnlyNavigationState::new(Vec::new());
        let reader = state.clone();
        let (sender, receiver) = mpsc::channel();
        let task = std::thread::spawn(move || {
            sender
                .send(reader.read_when_runtime_ready())
                .expect("reader should still be connected");
        });

        assert!(receiver.recv_timeout(Duration::from_millis(25)).is_err());
        state.replace(vec!["dla-launcher://works/RJ01326398".to_owned()]);
        state.signal_runtime_ready();
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("reader should resume when the runtime is ready"),
            ["dla-launcher://works/RJ01326398"]
        );
        task.join().expect("reader should finish cleanly");
    }

    #[test]
    fn accepts_only_the_exact_read_only_work_route() {
        for (value, expected) in [
            (
                "dla-launcher://works/RJ01326398",
                "dla-launcher://works/RJ01326398",
            ),
            (
                "DLA-LAUNCHER://WORKS/bj12345",
                "dla-launcher://works/BJ12345",
            ),
            (
                "dla-launcher://works/vj1234567890",
                "dla-launcher://works/VJ1234567890",
            ),
        ] {
            assert_eq!(parse_exact_work_link(value).as_deref(), Some(expected));
        }
    }

    #[test]
    fn rejects_ambiguous_or_write_capable_routes_before_url_parsing() {
        for value in [
            "https://works/RJ01326398",
            "dla-launcher://scanner/RJ01326398",
            "dla-launcher://import/RJ01326398",
            "dla-launcher://launch/RJ01326398",
            "dla-launcher://works.example/RJ01326398",
            "dla-launcher://user@works/RJ01326398",
            "dla-launcher://works:443/RJ01326398",
            "dla-launcher://works/RJ01326398?launch=true",
            "dla-launcher://works/RJ01326398?",
            "dla-launcher://works/RJ01326398#section",
            "dla-launcher://works/RJ01326398#",
            "dla-launcher://works/RJ01326398/extra",
            "dla-launcher://works/RJ01326398/",
            "dla-launcher://works/%52J01326398",
            "dla-launcher://works/%2e/RJ01326398",
            " dla-launcher://works/RJ01326398",
            "dla-launcher://works/RJ01326398 ",
            "dla-launcher://works/RJ1234",
            "dla-launcher://works/RJ12345678901",
            "dla-launcher://works/RJ12345.exe",
            "dla-launcher://workſ/RJ01326398",
            "dla-launcher://works/🧙12345",
            "🧙dla-launcher://works/RJ01326398",
        ] {
            assert_eq!(parse_exact_work_link(value), None, "accepted {value}");
        }
    }

    #[test]
    fn requires_one_and_only_one_locator_argument() {
        let executable = "/opt/dla-launcher".to_owned();
        let valid = "dla-launcher://works/RJ01326398".to_owned();
        assert_eq!(
            validated_cli_link(&[executable.clone(), valid.clone()]).as_deref(),
            Some(valid.as_str())
        );
        assert_eq!(validated_cli_link(std::slice::from_ref(&executable)), None);
        assert_eq!(
            validated_cli_link(&[executable, valid, "unexpected".to_owned()]),
            None
        );
    }

    #[test]
    fn requires_one_and_only_one_open_url() {
        let valid = "dla-launcher://works/RJ01326398";
        assert_eq!(validated_url_link([valid]).as_deref(), Some(valid));
        assert_eq!(validated_url_link(std::iter::empty::<&str>()), None);
        assert_eq!(
            validated_url_link([valid, "dla-launcher://works/BJ12345"]),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_utf8_platform_arguments_without_panicking() {
        use std::os::unix::ffi::OsStringExt;

        let arguments = [
            OsString::from("/opt/dla-launcher"),
            OsString::from_vec(vec![0xff]),
        ];
        assert_eq!(validated_os_cli_link(&arguments), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn packaged_linux_launchers_forward_exactly_one_url() {
        let template = include_str!("../linux/dla-launcher.desktop.hbs");
        assert!(template.contains("Exec={{exec}} %u"));
        assert!(template.contains("MimeType=x-scheme-handler/dla-launcher;"));
        assert!(!template.contains("%U"));
    }
}
