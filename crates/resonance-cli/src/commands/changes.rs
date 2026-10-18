//! The change log: `changes`, `changes show`, `changes done` (MVP-040).

use std::fmt::Write;

use resonance_core::changes::{Done, Entry};
use resonance_core::error::Result;
use serde_json::{Value, json};

use super::Ctx;
use crate::cli::ChangesCommand;

pub fn changes(ctx: &Ctx, action: Option<ChangesCommand>, since: Option<u64>) -> Result<()> {
    match action {
        None => {
            let v: Vec<Entry> = ctx.run("changes", json!({"since": since}))?;
            ctx.emit(&v, |v| list(v, since))
        }
        Some(ChangesCommand::Show { seq }) => {
            let e: Entry = ctx.run("changes_show", json!({"seq": seq}))?;
            ctx.emit(&e, show)
        }
        Some(ChangesCommand::Done { seqs, note }) => {
            let d: Done = ctx.run("changes_done", json!({"seqs": seqs, "note": note}))?;
            ctx.emit(&d, |d| {
                let mut out = String::new();
                if !d.removed.is_empty() {
                    let _ = writeln!(
                        out,
                        "Removed {} entr{} from the change log.",
                        d.removed.len(),
                        if d.removed.len() == 1 { "y" } else { "ies" }
                    );
                }
                if !d.unknown.is_empty() {
                    let n: Vec<String> = d.unknown.iter().map(u64::to_string).collect();
                    let _ = writeln!(out, "Not pending: {}.", n.join(", "));
                }
                let _ = writeln!(out, "{} pending.", d.remaining);
                out
            })
        }
    }
}

/// One line per entry: `seq  when  who  summary`.
pub fn line(e: &Entry) -> String {
    let when = e.at.get(..16).unwrap_or(&e.at).replace('T', " ");
    format!(
        "{:>4}  {when}  {:<6} {}  ({})",
        e.seq, e.origin, e.summary, e.actor
    )
}

fn list(v: &[Entry], since: Option<u64>) -> String {
    if v.is_empty() {
        return match since {
            Some(s) => format!("No change-log entries after {s}.\n"),
            None => "The change log is empty.\n".into(),
        };
    }
    let mut out = String::new();
    for e in v {
        let _ = writeln!(out, "{}", line(e));
    }
    let last = v.last().map(|e| e.seq).unwrap_or_default();
    let _ = writeln!(
        out,
        "\n{} pending. Details: `resonance changes show <seq>`; when handled: `resonance changes done <seq>…` (remember {last} as the last entry read).",
        v.len()
    );
    out
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn show(e: &Entry) -> String {
    let mut out = format!(
        "Change {} ({} via {}, {}): {}\n{} {} — {}\n",
        e.seq, e.actor, e.origin, e.at, e.summary, e.item.ntype, e.item.key, e.item.title
    );
    let before = e.before.clone().unwrap_or_default();
    let after = e.after.clone().unwrap_or_default();
    let mut keys: Vec<&String> = before.keys().chain(after.keys()).collect();
    keys.sort();
    keys.dedup();
    for k in keys {
        let (b, a) = (before.get(k), after.get(k));
        if b == a {
            continue;
        }
        let _ = writeln!(out, "  {k}:");
        if let Some(b) = b {
            let _ = writeln!(out, "    - {}", value_text(b));
        }
        if let Some(a) = a {
            let _ = writeln!(out, "    + {}", value_text(a));
        }
    }
    out
}
