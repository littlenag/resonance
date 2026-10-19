//! Human-readable output (MVP-015). `--json` prints the same views as JSON instead.

use std::fmt::Write as _;

use resonance_core::api::{Counts, StatusView, VersionInfo};
use resonance_core::ingest::ReconcileReport;
use resonance_core::query::repair::RepairItem;

fn indent(text: &str, by: &str) -> String {
    text.lines()
        .map(|l| format!("{by}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn version(v: &VersionInfo) -> String {
    format!(
        "resonance {}\nstore schema version: {}\nMCP protocol version: {} (also accepts {})\n",
        v.version,
        v.schema_version,
        v.mcp_protocol_version,
        v.mcp_protocol_versions[1..].join(", ")
    )
}

pub fn reconcile(r: &ReconcileReport) -> String {
    let mut out = format!(
        "Reconciled {}: {} added, {} changed, {} removed ({} ms).\n",
        plural(r.files_scanned, "file", "files"),
        r.files_added,
        r.files_changed,
        r.files_removed,
        r.duration_ms
    );
    let a = &r.anchors;
    if a.relocated + a.moved + a.approximate + a.orphaned + a.restored > 0 {
        let _ = writeln!(
            out,
            "Anchors: {} followed edits, {} moved, {} matched approximately, {} orphaned, {} restored.",
            a.relocated, a.moved, a.approximate, a.orphaned, a.restored
        );
    }
    let l = &r.links;
    if l.stale + l.orphaned + l.restored > 0 {
        let _ = writeln!(
            out,
            "Links: {} became stale, {} orphaned, {} restored.",
            l.stale, l.orphaned, l.restored
        );
    }
    out
}

fn counts(c: &Counts) -> String {
    let link = |s: &str| c.links.get(s).copied().unwrap_or(0);
    format!(
        "Files: {}   Requirements: {} active, {} retired   Decisions: {}\n\
         Code units: {}   Tests: {}   Annotations: {}   Test runs recorded: {}\n\
         Links: {} asserted, {} verified, {} stale, {} orphaned\n",
        c.files,
        c.requirements,
        c.requirements_retired,
        c.decisions,
        c.code_units,
        c.tests,
        c.annotations,
        c.evidence,
        link("asserted"),
        link("verified"),
        link("stale"),
        link("orphaned")
    )
}

pub fn status(s: &StatusView) -> String {
    let mode = match (s.stage.as_str(), s.participation.as_str()) {
        ("1", _) => "offline mode, stage 1".to_string(),
        (st, "user") => format!("shared graph, stage {st}; you have opted in"),
        (st, _) => format!("offline over a shared graph, stage {st}"),
    };
    let mut out = format!("Resonance in {} ({mode})\n", s.root);
    if let Some(t) = &s.last_reconcile {
        let _ = writeln!(out, "Last reconciled: {t}");
    }
    out.push('\n');
    out.push_str(&counts(&s.counts));
    if s.counts.requirements + s.counts.requirements_retired + s.counts.decisions == 0 {
        out.push_str("No spec items yet: ask your agent to bring your requirements documents into Resonance.\n");
    }
    out.push('\n');
    out.push_str(&history(&s.history));
    if !s.divergences.is_empty() {
        let _ = writeln!(
            out,
            "Divergences needing a look ({}; details with `resonance recover`):",
            s.divergences.len()
        );
        for d in &s.divergences {
            let _ = writeln!(out, "  {}", crate::commands::recover_line(d));
        }
        out.push('\n');
    }
    if s.pending_changes > 0 {
        let _ = writeln!(
            out,
            "Change log: {} pending entr{} for your agent to handle (`resonance tui`, then l, shows them); it clears them before commits, pushes and pulls.\n",
            s.pending_changes,
            if s.pending_changes == 1 { "y" } else { "ies" }
        );
    }
    if !s.agent_kit.installed {
        out.push_str(
            "Agent kit: not installed; onboard your agent with `resonance init` (FR-210).\n\n",
        );
    } else if s.agent_kit.stale {
        let _ = writeln!(
            out,
            "Agent kit: {} is stale; run `resonance init` again to refresh it.\n",
            s.agent_kit.version.as_deref().unwrap_or("?")
        );
    }
    out.push_str(&repair(&s.repair));
    out
}

pub fn history(h: &resonance_core::api::HistoryStatus) -> String {
    let mut out = String::new();
    match &h.open_delta {
        Some(p) => {
            let _ = writeln!(
                out,
                "Open delta: {p} ({} operation(s), {} file(s) changed)",
                h.open_operations, h.open_files
            );
        }
        None => out.push_str("Open delta: empty\n"),
    }
    let _ = writeln!(
        out,
        "Sealed deltas: {}{}",
        h.sealed,
        if h.snapshot { "; snapshot present" } else { "" }
    );
    if !h.detached.is_empty() {
        let _ = writeln!(
            out,
            "Detached pool: {} delta(s) whose commits were rewritten; see `resonance recover`",
            h.detached.len()
        );
    }
    if let Some(held) = &h.held {
        let _ = writeln!(
            out,
            "Action required: .resonance/ changed unexpectedly at {} ({} operation(s) would be lost). The graph was kept as it was; run `resonance rebuild` to accept the change, or `resonance recover`.",
            held.at, held.missing
        );
    }
    if h.skipped > 0 {
        let _ = writeln!(
            out,
            "{} operation(s) could not be applied by the last fold (see `resonance doctor`)",
            h.skipped
        );
    }
    out.push('\n');
    out
}

pub fn repair(items: &[RepairItem]) -> String {
    if items.is_empty() {
        return "Repair list: empty. Nothing needs attention.\n".into();
    }
    let mut out = format!("Repair list ({}):\n", items.len());
    for i in items {
        let _ = writeln!(out, "  [{}] {}", i.kind.as_str(), i.summary);
        if let Some(q) = &i.quote {
            out.push_str(&indent(q, "      | "));
            out.push('\n');
        }
        let _ = writeln!(out, "      -> {}", i.suggestion);
    }
    out
}

pub fn counts_line(c: &Counts) -> String {
    counts(c)
}

pub fn health(h: &resonance_core::daemon::Health) -> String {
    let mut out = format!(
        "Daemon for {} (pid {}): {}{}\n  endpoint: {}\n  started: {}; version {}\n  watermark: {} of {} events applied; {} pending",
        h.root,
        h.pid,
        h.state,
        if h.ready { "" } else { " (not ready)" },
        h.endpoint,
        h.started_at,
        h.version,
        h.ingest_watermark,
        h.events_received,
        h.pending_events
    );
    if h.lag_ms > 0 {
        let _ = write!(out, " ({} ms behind)", h.lag_ms);
    }
    out.push('\n');
    if let Some(t) = &h.last_ingest {
        let _ = writeln!(out, "  last ingest: {t}");
    }
    if let Some(e) = &h.last_error {
        let _ = writeln!(out, "  last error: {e}");
    }
    let _ = writeln!(
        out,
        "  {} connected, {} subscribed, {} requests served",
        h.connections, h.subscribers, h.requests_served
    );
    out
}

pub fn service_plan(verb: &str, p: &resonance_core::registry::ServicePlan) -> String {
    let commands = if verb == "install" {
        &p.load
    } else {
        &p.unload
    };
    let mut out = if verb == "install" {
        format!(
            "resonance registry install will write {}:\n\n{}\n",
            p.path.display(),
            indent(&p.contents, "    ")
        )
    } else {
        format!(
            "resonance registry uninstall will remove {}\n",
            p.path.display()
        )
    };
    out.push_str("and run:\n");
    for c in commands {
        let _ = writeln!(out, "    {}", c.join(" "));
    }
    out
}

pub fn doctor(d: &crate::commands::services::DoctorReport) -> String {
    let mut out = format!(
        "resonance {} (runtime directory {})\n",
        d.version, d.runtime_dir
    );
    if let Some(r) = &d.repository {
        if r.initialized {
            let _ = writeln!(
                out,
                "This repository: {} (schema {}, {}, daemon {})",
                r.root,
                r.schema_version
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "?".into()),
                if r.stage >= 2 {
                    "shared"
                } else if r.excluded {
                    "excluded from git"
                } else {
                    "NOT excluded from git"
                },
                if r.daemon_running {
                    "running"
                } else {
                    "not running"
                }
            );
            match &r.agent_kit {
                Some(k) if !k.installed => out.push_str(
                    "Agent kit: not installed; onboard your agent with `resonance init` (FR-210)\n",
                ),
                Some(k) if k.stale => {
                    let _ = writeln!(
                        out,
                        "Agent kit: {} is stale; run `resonance init` again to refresh it",
                        k.version.as_deref().unwrap_or("?")
                    );
                }
                Some(k) => {
                    let _ = writeln!(
                        out,
                        "Agent kit: {} installed",
                        k.version.as_deref().unwrap_or("?")
                    );
                }
                None => {}
            }
        } else {
            let _ = writeln!(out, "This repository: {} (Resonance is not set up)", r.root);
        }
    }
    let _ = writeln!(
        out,
        "Registry: {}",
        if d.registry_running {
            "running"
        } else {
            "not running (daemons are found through daemon.json)"
        }
    );
    if d.daemons.is_empty() {
        out.push_str("Daemons: none running\n");
    } else {
        out.push_str("Daemons:\n");
        for dc in &d.daemons {
            match &dc.health {
                Some(h) => {
                    let _ = writeln!(
                        out,
                        "  {} (pid {}): {}{}, {} pending, {} ms behind{}",
                        dc.root,
                        dc.pid,
                        h.state,
                        if h.ready { ", ready" } else { "" },
                        h.pending_events,
                        h.lag_ms,
                        if dc.version_skew {
                            ", different build"
                        } else {
                            ""
                        }
                    );
                }
                None => {
                    let _ = writeln!(out, "  {} (pid {}): not answering", dc.root, dc.pid);
                }
            }
        }
    }
    if d.problems.is_empty() {
        out.push_str("No problems found.\n");
    } else {
        let _ = writeln!(out, "Problems ({}):", d.problems.len());
        for p in &d.problems {
            let _ = writeln!(out, "  - {p}");
        }
    }
    out
}
