//! Runs declared tests and records `Evidence`; pass, fail and error rules (FR-135, FR-004, MVP-017).
//!
//! - **Pass**: exit code 0. The test's `verifies` links become `verified`, with refreshed
//!   endpoint hashes.
//! - **Fail**: a non-zero exit code. Its links become (or stay) `asserted`, with the failing
//!   evidence attached.
//! - **Error**: the command could not be started, ran past the timeout, or printed the
//!   zero-tests pattern (for example a `cargo test` filter that matched nothing). An error is
//!   never a pass; link statuses are left as they were.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Bytes of output kept as evidence.
pub const OUTPUT_TAIL: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestResult {
    Pass,
    Fail,
    Error,
}

impl TestResult {
    pub fn as_str(self) -> &'static str {
        match self {
            TestResult::Pass => "pass",
            TestResult::Fail => "fail",
            TestResult::Error => "error",
        }
    }
}

/// What happened when a test command ran.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunOutcome {
    pub result: TestResult,
    pub exit_code: Option<i32>,
    /// The last [`OUTPUT_TAIL`] bytes of combined standard output and error.
    pub output_tail: String,
    pub duration_ms: u64,
    /// Why the run is an error, if it is one.
    pub error: Option<String>,
}

struct Collected {
    tail: Vec<u8>,
    carry: Vec<u8>,
    matched: bool,
}

/// Run `command` with `sh -c` in `dir`, stopping it after `timeout`.
pub fn run_command(
    command: &str,
    dir: &Path,
    timeout: Duration,
    zero_tests_pattern: &str,
) -> RunOutcome {
    let started = Instant::now();
    let mut cmd = Command::new("sh");
    // Merge standard error into standard output so the evidence keeps their order.
    cmd.arg("-c")
        .arg(format!("exec 2>&1\n{command}"))
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return RunOutcome {
                result: TestResult::Error,
                exit_code: None,
                output_tail: String::new(),
                duration_ms: started.elapsed().as_millis() as u64,
                error: Some(format!("could not start the test command: {e}")),
            };
        }
    };

    let pattern = zero_tests_pattern.as_bytes().to_vec();
    let collected = Arc::new(Mutex::new(Collected {
        tail: Vec::new(),
        carry: Vec::new(),
        matched: false,
    }));
    let reader = child.stdout.take().map(|mut out| {
        let collected = Arc::clone(&collected);
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = out.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let mut c = collected.lock().expect("output lock");
                let chunk = &buf[..n];
                if !pattern.is_empty() && !c.matched {
                    let mut window = std::mem::take(&mut c.carry);
                    window.extend_from_slice(chunk);
                    if window
                        .windows(pattern.len())
                        .any(|w| w == pattern.as_slice())
                    {
                        c.matched = true;
                    }
                    let keep = pattern.len().saturating_sub(1).min(window.len());
                    c.carry = window[window.len() - keep..].to_vec();
                }
                c.tail.extend_from_slice(chunk);
                if c.tail.len() > OUTPUT_TAIL {
                    let cut = c.tail.len() - OUTPUT_TAIL;
                    c.tail.drain(..cut);
                }
            }
        })
    });

    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                timed_out = true;
                kill_group(&mut child);
                break child.wait().ok();
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => break None,
        }
    };
    if let Some(handle) = reader {
        if timed_out {
            // Grandchildren may still hold the pipe open; do not wait for them forever.
            let deadline = Instant::now() + Duration::from_secs(2);
            while !handle.is_finished() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
        } else {
            let _ = handle.join();
        }
    }
    let c = collected.lock().expect("output lock");
    let output_tail = String::from_utf8_lossy(&c.tail).into_owned();
    let exit_code = status.and_then(|s| s.code());
    let duration_ms = started.elapsed().as_millis() as u64;

    let (result, error) = if timed_out {
        (
            TestResult::Error,
            Some(format!(
                "stopped after the {}s timeout (test.timeoutSeconds)",
                timeout.as_secs()
            )),
        )
    } else if c.matched {
        (
            TestResult::Error,
            Some(format!(
                "the output contains `{zero_tests_pattern}`, which means no test actually ran (test.zeroTestsPattern)"
            )),
        )
    } else if exit_code == Some(0) {
        (TestResult::Pass, None)
    } else if exit_code == Some(127) {
        (
            TestResult::Error,
            Some("the test command was not found (exit code 127)".into()),
        )
    } else if exit_code.is_none() {
        (
            TestResult::Error,
            Some("the test command was killed by a signal".into()),
        )
    } else {
        (TestResult::Fail, None)
    };
    RunOutcome {
        result,
        exit_code,
        output_tail,
        duration_ms,
        error,
    }
}

fn kill_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(format!("-{}", child.id()))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}

/// Fill `{name}`, `{path}` and `{address}` into a `test.command` template (MVP-017).
pub fn substitute(template: &str, name: &str, path: &str, address: &str) -> String {
    template
        .replace("{name}", name)
        .replace("{path}", path)
        .replace("{address}", address)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(cmd: &str) -> RunOutcome {
        run_command(
            cmd,
            Path::new("."),
            Duration::from_secs(10),
            "running 0 tests",
        )
    }

    #[test]
    fn pass_fail_error() {
        let pass = run("echo ok");
        assert_eq!(pass.result, TestResult::Pass);
        assert_eq!(pass.output_tail.trim(), "ok");
        let fail = run("echo boom >&2; exit 3");
        assert_eq!(fail.result, TestResult::Fail);
        assert_eq!(fail.exit_code, Some(3));
        assert!(fail.output_tail.contains("boom"), "stderr is captured");
        assert_eq!(run("printf 'running 0 tests\\n'").result, TestResult::Error);
        assert_eq!(
            run("definitely-not-a-command-xyz").result,
            TestResult::Error
        );
    }

    #[test]
    fn timeouts_are_errors() {
        let out = run_command("sleep 5", Path::new("."), Duration::from_millis(200), "");
        assert_eq!(out.result, TestResult::Error);
        assert!(out.duration_ms < 4000);
    }

    #[test]
    fn output_is_truncated_to_the_tail() {
        let out = run("i=0; while [ $i -lt 2000 ]; do echo line-$i; i=$((i+1)); done");
        assert!(out.output_tail.len() <= OUTPUT_TAIL);
        assert!(out.output_tail.trim_end().ends_with("line-1999"));
    }

    #[test]
    fn templates() {
        assert_eq!(
            substitute(
                "cargo test -- --exact {name} # {path} {address}",
                "t",
                "a.rs",
                "a.rs::t"
            ),
            "cargo test -- --exact t # a.rs a.rs::t"
        );
    }
}
