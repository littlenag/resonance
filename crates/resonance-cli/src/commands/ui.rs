//! `resonance ui` (FR-287, MVP-043): open the web UI the daemon serves, starting the daemon
//! if needed. The address carries this working tree's token once; the page keeps it as a
//! cookie (FR-288).

use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use resonance_core::daemon::client::Remote;
use resonance_core::daemon::runtime;
use resonance_core::error::{Error, Result};
use serde_json::json;

use super::{Ctx, exe, person_actor};

/// How long to wait for the daemon to be ready before opening the page anyway.
const READY_TIMEOUT: Duration = Duration::from_secs(30);

pub fn ui(ctx: &Ctx, key: Option<String>, no_open: bool) -> Result<()> {
    if ctx.no_daemon {
        return Err(Error::Usage(
            "the web UI is served by the daemon, so it is not available with --no-daemon".into(),
        ));
    }
    let repo = ctx.repo()?;
    let mut remote = Remote::connect(&repo, &exe()?, person_actor(&repo), ctx.cwd.clone())?;
    let deadline = Instant::now() + READY_TIMEOUT;
    let health = loop {
        let h = remote.health()?;
        if h.ready || Instant::now() >= deadline {
            break h;
        }
        thread::sleep(Duration::from_millis(50));
    };
    let Some(address) = health.ui.clone() else {
        return Err(Error::Daemon(health.ui_error.unwrap_or_else(|| {
            "the daemon does not serve the web UI; see .resonance/local/daemon.log".into()
        })));
    };
    let state = runtime::read_ui(&repo).ok_or_else(|| {
        Error::Daemon("the web UI's key (.resonance/local/ui.json) is missing; run `resonance daemon restart`".into())
    })?;
    let next = match &key {
        Some(k) => format!("/spec/{}", encode(k)),
        None => "/".into(),
    };
    let url = format!("{address}auth?token={}&next={}", state.token, encode(&next));
    let opened = if no_open {
        false
    } else {
        match open_browser(&url) {
            Ok(()) => true,
            Err(e) => {
                ctx.note(&format!(
                    "Could not open a browser ({e}); open the address below instead."
                ));
                false
            }
        }
    };
    let root = repo.root.display().to_string();
    ctx.emit(
        &json!({"url": url, "address": address, "port": state.port, "root": root, "opened": opened}),
        |_| {
            if opened {
                format!("Opened the web UI for {root} ({address}) in your browser.\n")
            } else {
                format!(
                    "The web UI for {root}:\n  {url}\nThe address carries this repository's key: open it yourself, do not share it.\n"
                )
            }
        },
    )
}

/// Percent-encode everything but unreserved characters.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Open `url` with `$BROWSER`, else the platform's opener.
fn open_browser(url: &str) -> std::io::Result<()> {
    let mut cmd = match std::env::var("BROWSER")
        .ok()
        .filter(|b| !b.trim().is_empty())
    {
        Some(browser) => {
            let mut c = Command::new("sh");
            c.arg("-c")
                .arg(format!("{browser} \"$1\""))
                .arg("sh")
                .arg(url);
            c
        }
        None if cfg!(target_os = "macos") => {
            let mut c = Command::new("open");
            c.arg(url);
            c
        }
        None => {
            let mut c = Command::new("xdg-open");
            c.arg(url);
            c
        }
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::encode;

    #[test]
    fn encodes_keys_and_paths() {
        assert_eq!(encode("FR-001"), "FR-001");
        assert_eq!(encode("/spec/FR 1"), "%2Fspec%2FFR%201");
    }
}
