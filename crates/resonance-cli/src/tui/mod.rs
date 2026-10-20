//! `resonance tui`: a deliberately simple terminal UI over spec items (FR-270–FR-273, FR-279,
//! MVP-041). It lists, shows, edits, adds and retires requirements, decisions, designs and
//! constraints, and shows the change log. It is a client of the daemon and reloads when the
//! daemon reports a change (FR-104); its changes carry the person's identity and
//! `origin = tui`.

pub mod app;
pub mod ui;

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyEventKind};
use resonance_core::daemon::client::Remote;
use resonance_core::error::{Error, Result};

use crate::commands::{Ctx, exe, person_actor};
use app::{App, BackendClient, Effect};

/// The editor: `$VISUAL`, then `$EDITOR`, then `vi`.
pub fn editor() -> String {
    std::env::var("VISUAL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::env::var("EDITOR")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or_else(|| "vi".into())
}

/// The command that opens `path` at `line` in `editor` (which may carry arguments, such as
/// `code -w`): `sh -c '<editor> "$@"' sh +<line> <path>`.
pub fn editor_command(editor: &str, path: &Path, line: Option<usize>) -> Command {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(format!("{editor} \"$@\"")).arg("sh");
    if let Some(l) = line {
        cmd.arg(format!("+{l}"));
    }
    cmd.arg(path);
    cmd
}

fn run_editor(path: &Path, line: Option<usize>) -> Result<()> {
    let status = editor_command(&editor(), path, line)
        .status()
        .map_err(|e| Error::io("starting the editor", e))?;
    if !status.success() {
        return Err(Error::Invalid(format!("the editor exited with {status}")));
    }
    Ok(())
}

/// Carry out an effect outside the terminal UI, then hand the result back to the app.
fn perform(app: &mut App, effect: Effect) -> Result<()> {
    match effect {
        Effect::None => Ok(()),
        Effect::EditText { field, text } => {
            let dir = tempfile_dir()?;
            let path = dir.join(format!("resonance-{field}.md"));
            fs::write(&path, &text).map_err(|e| Error::io("writing a temporary file", e))?;
            let result = run_editor(&path, None);
            let edited = fs::read_to_string(&path).unwrap_or(text);
            let _ = fs::remove_file(&path);
            result?;
            app.finish_text(&field, &edited);
            Ok(())
        }
    }
}

fn tempfile_dir() -> Result<std::path::PathBuf> {
    let d = std::env::temp_dir().join(format!("resonance-tui-{}", std::process::id()));
    fs::create_dir_all(&d).map_err(|e| Error::io("creating a temporary directory", e))?;
    Ok(d)
}

/// Daemon change notifications, as a channel the UI loop polls (FR-104).
fn subscribe(ctx: &Ctx) -> Option<mpsc::Receiver<()>> {
    if ctx.no_daemon {
        return None;
    }
    let repo = ctx.repo().ok()?;
    let remote = Remote::connect(&repo, &exe().ok()?, person_actor(&repo), ctx.cwd.clone()).ok()?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = remote.subscribe(|_| tx.send(()).is_ok());
    });
    Some(rx)
}

pub fn run(ctx: &Ctx) -> Result<()> {
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return Err(Error::Usage("`resonance tui` needs a terminal".into()));
    }
    // The TUI is the person, even when the agent kit set RESONANCE_ACTOR (MVP-040).
    let backend = ctx.open_backend(person_actor(&ctx.repo()?))?;
    let mut app = App::new(Box::new(BackendClient(backend)))?;
    let changes = subscribe(ctx);
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        let mut ticks = 0u32;
        while !app.quit {
            terminal
                .draw(|f| ui::draw(f, &mut app))
                .map_err(|e| Error::io("drawing the terminal", e))?;
            if event::poll(Duration::from_millis(250)).map_err(|e| Error::io("reading keys", e))? {
                if let Event::Key(key) = event::read().map_err(|e| Error::io("reading keys", e))?
                    && key.kind == KeyEventKind::Press
                {
                    let effect = app.on_key(key);
                    if effect != Effect::None {
                        ratatui::restore();
                        let r = perform(&mut app, effect);
                        terminal = ratatui::init();
                        if let Err(e) = r {
                            app.message = Some((e.to_string(), true));
                        }
                    }
                }
                continue;
            }
            // Stay current: daemon notifications, or a periodic reload in direct mode.
            ticks += 1;
            let changed = match &changes {
                Some(rx) => rx.try_iter().count() > 0,
                None => ticks.is_multiple_of(8),
            };
            if changed && app.form.is_none() && app.input.is_none() {
                app.refresh();
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}

#[cfg(test)]
mod tests;
