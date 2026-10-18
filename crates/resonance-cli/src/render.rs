//! Human-readable output (MVP-015). `--json` prints the same views as JSON instead.

use std::fmt::Write as _;

use resonance_core::api::{
    Counts, ImportView, LinkResult, StatusView, TestReport, TestRun, VersionInfo,
};
use resonance_core::ingest::ReconcileReport;
use resonance_core::intent::IntentReport;
use resonance_core::query::context::ContextPack;
use resonance_core::query::repair::RepairItem;
use resonance_core::query::search::{SearchHit, Unverified};
use resonance_core::query::show::ShowView;
use resonance_core::query::trace::{DecisionLink, TraceEntry, TraceView};
use resonance_core::query::why::WhyView;
use resonance_core::query::{
    AnnotationView, DecisionView, EvidenceView, LinkView, RequirementView, UnitView,
};
use resonance_core::verify::TestResult;
use serde_json::Value;

/// The first line of `text`, at most `max` characters.
pub fn first_line(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    let more = text.lines().nth(1).is_some();
    if line.chars().count() > max {
        format!("{}…", line.chars().take(max).collect::<String>())
    } else if more {
        format!("{line} …")
    } else {
        line.to_string()
    }
}

fn indent(text: &str, by: &str) -> String {
    text.lines()
        .map(|l| format!("{by}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn short_id(id: &str) -> &str {
    if id.len() == 26 { &id[..10] } else { id }
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
    out.push_str(&intent_changes(&r.intent));
    out
}

fn intent_changes(i: &IntentReport) -> String {
    let mut out = String::new();
    if !i.scanned {
        return out;
    }
    let list = |keys: &[String]| -> String {
        if keys.len() <= 8 {
            keys.join(", ")
        } else {
            format!("{}, and {} more", keys[..8].join(", "), keys.len() - 8)
        }
    };
    if !i.added.is_empty() {
        let _ = writeln!(out, "Requirements added: {}", list(&i.added));
    }
    if !i.changed.is_empty() {
        let _ = writeln!(
            out,
            "Requirements changed (their links are now stale): {}",
            list(&i.changed)
        );
    }
    if !i.retired.is_empty() {
        let _ = writeln!(
            out,
            "Requirements retired (no longer in any source): {}",
            list(&i.retired)
        );
    }
    if !i.reactivated.is_empty() {
        let _ = writeln!(
            out,
            "Requirements back in a source: {}",
            list(&i.reactivated)
        );
    }
    for u in &i.unreadable {
        let _ = writeln!(
            out,
            "Warning: {} could not be read ({}); its requirements are kept as they were.",
            u.path, u.reason
        );
    }
    for c in &i.conflicts {
        let _ = writeln!(
            out,
            "Warning: {} in {}:{} was already created with `req add`; the document's version is ignored.",
            c.key, c.path, c.line
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
    if s.intent_sources.is_empty() {
        out.push_str(
            "Intent sources: none (import requirements with `resonance import <file.md>`)\n",
        );
    } else {
        let _ = writeln!(out, "Intent sources: {}", s.intent_sources.join(", "));
    }
    out.push('\n');
    out.push_str(&counts(&s.counts));
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
            "Change log: {} pending entr{} (`resonance changes`); agents clear them before commits, pushes and pulls.\n",
            s.pending_changes,
            if s.pending_changes == 1 { "y" } else { "ies" }
        );
    }
    if !s.agent_kit.installed {
        out.push_str("Agent kit: not installed; onboard your agent with `resonance agent install` (FR-210).\n\n");
    } else if s.agent_kit.stale {
        let _ = writeln!(
            out,
            "Agent kit: {} is stale; `resonance agent update` refreshes it.\n",
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

pub fn import(v: &ImportView) -> String {
    let mut out = String::new();
    if !v.added_sources.is_empty() {
        let _ = writeln!(out, "Now importing from: {}", v.added_sources.join(", "));
    }
    let _ = writeln!(
        out,
        "{} defined in {}.",
        plural(v.intent.total, "requirement", "requirements"),
        plural(v.sources.len(), "source", "sources")
    );
    out.push_str(&intent_changes(&v.intent));
    out.push_str("Changes to these files are picked up automatically by every command.\n");
    out
}

pub fn requirement(r: &RequirementView) -> String {
    let mut tags = vec![r.status.clone(), r.kind.clone()];
    if let Some(s) = &r.spec_status {
        tags.push(s.clone());
    }
    let mut out = format!(
        "{} [{}]\n{}\n",
        r.key,
        tags.join(", "),
        indent(&r.text, "  ")
    );
    if let Some(src) = &r.source {
        let _ = writeln!(
            out,
            "  source: {}:{}",
            src.get("path").and_then(Value::as_str).unwrap_or("?"),
            src.get("line").map(|l| l.to_string()).unwrap_or_default()
        );
    } else {
        let _ = writeln!(out, "  origin: {}", r.origin);
    }
    out
}

pub fn decision(d: &DecisionView) -> String {
    let mut out = format!(
        "Decision: {} [{}]\n  id: {}\n  decision: {}\n",
        d.title, d.status, d.id, d.decision
    );
    if let Some(c) = &d.context {
        let _ = writeln!(out, "  context: {c}");
    }
    if let Some(c) = &d.consequences {
        let _ = writeln!(out, "  consequences: {c}");
    }
    out
}

fn location(u: &UnitView) -> String {
    match &u.location {
        Some(l) => format!(
            "{l} [{}]",
            if u.removed {
                "removed".into()
            } else {
                format!("{:?}", l.state).to_lowercase()
            }
        ),
        None => "(no location)".into(),
    }
}

pub fn unit(u: &UnitView) -> String {
    let what = if u.ntype.as_str() == "Test" {
        "Test"
    } else {
        "Code unit"
    };
    let mut out = format!(
        "{what} {} ({})\n  at {}\n  id: {}\n",
        u.address,
        u.kind,
        location(u),
        u.id
    );
    if let Some(l) = &u.location
        && l.confidence < 1.0
    {
        let _ = writeln!(out, "  anchor confidence: {:.2}", l.confidence);
    }
    if let Some(run) = &u.run {
        let _ = writeln!(out, "  run: {run}");
    }
    out
}

pub fn units(list: &[UnitView]) -> String {
    if list.is_empty() {
        return "No code units or tests declared here yet (see `resonance unit add`).\n".into();
    }
    let mut out = String::new();
    for u in list {
        let loc = u
            .location
            .as_ref()
            .map(|l| l.to_string())
            .unwrap_or_default();
        let state = match &u.location {
            _ if u.removed => "removed".to_string(),
            Some(l) => format!("{:?}", l.state).to_lowercase(),
            None => "-".into(),
        };
        let what = if u.ntype.as_str() == "Test" {
            "test"
        } else {
            "unit"
        };
        let _ = writeln!(
            out,
            "{loc:<32} {:<40} {what:<5} {:<10} {state}",
            u.address, u.kind
        );
    }
    out
}

fn evidence_brief(e: &EvidenceView) -> String {
    let commit = e
        .commit
        .as_deref()
        .map(|c| format!(", commit {}", &c[..c.len().min(10)]))
        .unwrap_or_default();
    format!("last run {} at {}{commit}", e.result, e.timestamp)
}

fn link_line(l: &LinkView) -> String {
    let mut out = format!(
        "[{}] {} {} ({})",
        l.status, l.relation, l.other.label, l.other.ntype
    );
    if let Some(e) = &l.latest_evidence {
        let _ = write!(out, "; {}", evidence_brief(e));
    }
    if let (Some("agent"), Some(c)) = (l.provenance.as_deref(), l.confidence) {
        let _ = write!(out, "; agent confidence {c:.2}");
    }
    let _ = write!(out, "  link {}", short_id(&l.id));
    out
}

/// `source relation target [status]` and the link ID.
pub fn link_change(verb: &str, r: &LinkResult) -> String {
    format!(
        "{verb}: {} {}\n  link id: {}\n",
        r.source.label,
        link_line(&r.link),
        r.link.id
    )
}

pub fn link_result(r: &LinkResult) -> String {
    link_change(
        if r.created {
            "Linked"
        } else {
            "Already linked"
        },
        r,
    )
}

pub fn annotation(a: &AnnotationView) -> String {
    let on =
        a.on.as_ref()
            .map(|n| n.label.clone())
            .unwrap_or_else(|| "(nothing)".into());
    let task = a
        .task
        .as_deref()
        .map(|t| format!(" for task `{t}`"))
        .unwrap_or_default();
    let status = a.link_status.map(|s| format!(" [{s}]")).unwrap_or_default();
    format!(
        "{}{task} on {on}{status}\n{}\n  id: {}\n",
        a.kind,
        indent(&a.text, "  "),
        a.id
    )
}

fn annotations(list: &[AnnotationView]) -> String {
    let mut out = String::new();
    for a in list {
        let on = a.on.as_ref().map(|n| n.label.clone()).unwrap_or_default();
        let task = a
            .task
            .as_deref()
            .map(|t| format!(" (task {t})"))
            .unwrap_or_default();
        let via = if a.inherited {
            format!(" (on enclosing {on})")
        } else {
            String::new()
        };
        let _ = writeln!(out, "  {}{task}{via}: {}", a.kind, first_line(&a.text, 100));
        for l in a.text.lines().skip(1) {
            let _ = writeln!(out, "      {l}");
        }
    }
    out
}

pub fn show(v: &ShowView) -> String {
    match v {
        ShowView::Node(n) => {
            let mut out = format!(
                "{} {} [{}]\n  id: {}\n",
                n.node.ntype, n.node.label, n.node.status, n.node.id
            );
            if let Some(l) = &n.location {
                let _ = writeln!(out, "  at {l} (anchor confidence {:.2})", l.confidence);
            }
            if let Some(obj) = n.properties.as_object() {
                for (k, v) in obj {
                    if matches!(k.as_str(), "path" | "key" | "name") || v.is_null() {
                        continue;
                    }
                    let text = match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    if text.contains('\n') {
                        let _ = writeln!(out, "  {k}:\n{}", indent(&text, "    "));
                    } else {
                        let _ = writeln!(out, "  {k}: {text}");
                    }
                }
            }
            if !n.links.is_empty() {
                out.push_str("Links:\n");
                for l in &n.links {
                    let _ = writeln!(out, "  {}", link_line(l));
                }
            }
            if !n.annotations.is_empty() {
                out.push_str("Annotations:\n");
                out.push_str(&annotations(&n.annotations));
            }
            if !n.contained_by.is_empty() {
                let names: Vec<&str> = n.contained_by.iter().map(|c| c.label.as_str()).collect();
                let _ = writeln!(out, "Inside: {}", names.join(", "));
            }
            if !n.contains.is_empty() {
                let names: Vec<&str> = n.contains.iter().map(|c| c.label.as_str()).collect();
                let _ = writeln!(out, "Contains: {}", names.join(", "));
            }
            out
        }
        ShowView::Link(l) => {
            let mut out = format!(
                "Link {} [{}{}]\n  {} {} {}\n",
                l.link.id,
                l.link.status,
                if l.removed { ", removed" } else { "" },
                l.src.label,
                l.link.etype,
                l.dst.label
            );
            if let Some(obj) = l.properties.as_object() {
                for (k, v) in obj {
                    if !v.is_null() {
                        let _ = writeln!(
                            out,
                            "  {k}: {}",
                            v.as_str()
                                .map(String::from)
                                .unwrap_or_else(|| v.to_string())
                        );
                    }
                }
            }
            for e in &l.evidence {
                let _ = writeln!(out, "  evidence: {}", evidence_brief(e));
            }
            out
        }
    }
}

fn trace_entry(e: &TraceEntry) -> String {
    let where_ = e
        .unit
        .as_ref()
        .and_then(|u| u.location.as_ref())
        .map(|l| format!(" at {l}"))
        .unwrap_or_default();
    let mut out = format!("  [{}] {}{where_}", e.link.status, e.link.other.label);
    if let Some(ev) = &e.link.latest_evidence {
        let _ = write!(out, "; {}", evidence_brief(ev));
    }
    let _ = write!(out, "  link {}", short_id(&e.link.id));
    out
}

fn decisions(list: &[DecisionLink]) -> String {
    let mut out = String::new();
    for d in list {
        let _ = writeln!(
            out,
            "  [{}] {}: {}",
            d.link.status,
            d.decision.title,
            first_line(&d.decision.decision, 90)
        );
    }
    out
}

pub fn trace(t: &TraceView) -> String {
    let mut out = requirement(&t.requirement);
    out.push_str("Satisfied by:\n");
    if t.satisfied_by.is_empty() {
        out.push_str("  (nothing yet)\n");
    }
    for e in &t.satisfied_by {
        let _ = writeln!(out, "{}", trace_entry(e));
    }
    out.push_str("Verified by:\n");
    if t.verified_by.is_empty() {
        out.push_str("  (no tests yet)\n");
    }
    for e in &t.verified_by {
        let _ = writeln!(out, "{}", trace_entry(e));
    }
    if !t.constrained_by.is_empty() {
        out.push_str("Constrained by:\n");
        out.push_str(&decisions(&t.constrained_by));
    }
    if !t.annotations.is_empty() {
        out.push_str("Annotations:\n");
        out.push_str(&annotations(&t.annotations));
    }
    out
}

pub fn why(w: &WhyView) -> String {
    let mut out = match &w.unit {
        Some(u) => unit(u),
        None => format!("{} {}\n", w.node.ntype, w.node.label),
    };
    if !w.enclosed_by.is_empty() {
        let names: Vec<&str> = w.enclosed_by.iter().map(|n| n.label.as_str()).collect();
        let _ = writeln!(out, "  inside: {}", names.join(" < "));
    }
    out.push_str("Satisfies:\n");
    if w.requirements.is_empty() {
        out.push_str("  (no requirements linked)\n");
    }
    for r in &w.requirements {
        let _ = writeln!(
            out,
            "  [{}] {}: {}  link {}",
            r.link.status,
            r.requirement.key,
            first_line(&r.requirement.text, 90),
            short_id(&r.link.id)
        );
    }
    if !w.decisions.is_empty() {
        out.push_str("Constrained by:\n");
        for d in &w.decisions {
            let via = d
                .via
                .as_ref()
                .map(|v| format!(" (via {})", v.label))
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "  [{}] {}{via}: {}",
                d.decision.link.status,
                d.decision.decision.title,
                first_line(&d.decision.decision.decision, 90)
            );
        }
    }
    out.push_str("Covered by tests:\n");
    if w.tests.is_empty() {
        out.push_str("  (none)\n");
    }
    for t in &w.tests {
        let _ = writeln!(out, "  {}", link_line(&t.link));
    }
    if !w.verifies.is_empty() {
        out.push_str("Verifies:\n");
        for l in &w.verifies {
            let _ = writeln!(out, "  {}", link_line(l));
        }
    }
    out.push_str("Annotations:\n");
    if w.annotations.is_empty() {
        out.push_str("  (none)\n");
    }
    out.push_str(&annotations(&w.annotations));
    out
}

pub fn search(hits: &[SearchHit]) -> String {
    if hits.is_empty() {
        return "No matches.\n".into();
    }
    let mut out = String::new();
    for h in hits {
        let snippet = h
            .snippet
            .as_deref()
            .filter(|s| *s != h.node.label)
            .map(|s| format!("  {}", first_line(s, 90)))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "{:<11} {:<40} {:<9}{snippet}",
            h.node.ntype.as_str(),
            h.node.label,
            h.node.status
        );
    }
    out
}

pub fn unverified(list: &[Unverified]) -> String {
    if list.is_empty() {
        return "Every active requirement has a verified test.\n".into();
    }
    let mut out = format!(
        "{} with no verified test:\n",
        plural(list.len(), "requirement", "requirements")
    );
    for u in list {
        let tests = if u.tests.is_empty() {
            "no tests".to_string()
        } else {
            u.tests
                .iter()
                .map(|t| format!("{} ({})", t.test.label, t.status))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let _ = writeln!(
            out,
            "  {:<9} {}  [{}; {} satisfying]",
            u.requirement.key,
            first_line(&u.requirement.text, 70),
            tests,
            u.satisfied_by
        );
    }
    out
}

pub fn context(p: &ContextPack) -> String {
    let mut out = String::new();
    if let Some(t) = &p.task {
        let _ = writeln!(out, "Context pack for task `{t}`\n");
    }
    out.push_str("Requirements:\n");
    for r in &p.requirements {
        out.push_str(&indent(&requirement(r), "  "));
        out.push('\n');
    }
    if !p.decisions.is_empty() {
        out.push_str("Decisions:\n");
        out.push_str(&decisions(&p.decisions));
    }
    out.push_str("Code:\n");
    if p.code_units.is_empty() {
        out.push_str("  (no code linked yet)\n");
    }
    for u in &p.code_units {
        let links: Vec<String> = u
            .links
            .iter()
            .map(|l| format!("{} {} [{}]", l.etype, l.requirement, l.status))
            .collect();
        let _ = writeln!(
            out,
            "  {} at {}  ({})",
            u.unit.address,
            location(&u.unit),
            links.join(", ")
        );
    }
    out.push_str("Tests:\n");
    if p.tests.is_empty() {
        out.push_str("  (no tests linked yet)\n");
    }
    for t in &p.tests {
        let ev = t
            .latest_evidence
            .as_ref()
            .map(|e| format!("; {}", evidence_brief(e)))
            .unwrap_or_default();
        let _ = writeln!(out, "  {} at {}{ev}", t.unit.address, location(&t.unit));
        if let Some(run) = &t.unit.run {
            let _ = writeln!(out, "      run: {run}");
        }
    }
    if !p.annotations.is_empty() {
        out.push_str("Annotations in scope:\n");
        out.push_str(&annotations(&p.annotations));
    }
    if !p.call_outs.is_empty() {
        out.push_str("Call-outs for this task:\n");
        out.push_str(&annotations(&p.call_outs));
    }
    out
}

fn run(r: &TestRun) -> String {
    let label = match r.outcome.result {
        TestResult::Pass => "PASS ",
        TestResult::Fail => "FAIL ",
        TestResult::Error => "ERROR",
    };
    let mut out = format!(
        "{label} {} ({} ms)\n",
        r.test.address, r.outcome.duration_ms
    );
    if let Some(cmd) = &r.command {
        let _ = writeln!(out, "      $ {cmd}");
    }
    if let Some(e) = &r.outcome.error {
        let _ = writeln!(out, "      {e}");
    }
    if r.outcome.result != TestResult::Pass && !r.outcome.output_tail.trim().is_empty() {
        let lines: Vec<&str> = r.outcome.output_tail.lines().collect();
        let start = lines.len().saturating_sub(15);
        for l in &lines[start..] {
            let _ = writeln!(out, "      | {l}");
        }
    }
    for l in &r.links {
        let _ = writeln!(
            out,
            "      {} {} -> {}",
            l.relation, l.other.label, l.status
        );
    }
    out
}

pub fn test_report(t: &TestReport) -> String {
    if t.runs.is_empty() {
        return format!(
            "No tests are linked to {} (link one with `resonance link <test> verifies {}`).\n",
            t.target, t.target
        );
    }
    let mut out = String::new();
    for r in &t.runs {
        out.push_str(&run(r));
    }
    let _ = writeln!(
        out,
        "{} passed, {} failed, {} errors.",
        t.passed, t.failed, t.errors
    );
    out
}

pub fn counts_line(c: &Counts) -> String {
    counts(c)
}

pub fn identity_change(verb: &str, c: &resonance_core::api::IdentityChange) -> String {
    let from: Vec<&str> = c.retired.iter().map(|u| u.address.as_str()).collect();
    let into: Vec<&str> = c.into.iter().map(|u| u.address.as_str()).collect();
    format!(
        "{verb} {} into {}; {} moved. The old unit{} {} retired and kept for history.\n",
        from.join(", "),
        into.join(", "),
        plural(c.links_moved, "link or annotation", "links and annotations"),
        if from.len() == 1 { "" } else { "s" },
        if from.len() == 1 { "is" } else { "are" },
    )
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
                    "Agent kit: not installed; onboard your agent with `resonance agent install` (FR-210)\n",
                ),
                Some(k) if k.stale => {
                    let _ = writeln!(
                        out,
                        "Agent kit: {} is stale; `resonance agent update` refreshes it",
                        k.version.as_deref().unwrap_or("?")
                    );
                }
                Some(k) => {
                    let _ = writeln!(out, "Agent kit: {} installed", k.version.as_deref().unwrap_or("?"));
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

pub fn spec_item(v: &resonance_core::query::SpecItemView) -> String {
    let mut out = String::new();
    let head = match &v.key {
        Some(k) => format!("{} {k}", v.ntype),
        None => v.ntype.to_string(),
    };
    let _ = writeln!(out, "{head}: {}", v.title);
    let _ = writeln!(out, "  status: {}", v.status);
    if let Some(p) = &v.priority {
        let _ = writeln!(out, "  priority: {p}");
    }
    if let Some(k) = &v.kind {
        let _ = writeln!(out, "  kind: {k}");
    }
    if v.origin == "import"
        && let Some(src) = &v.source
    {
        let _ = writeln!(
            out,
            "  source: {}:{} (edit it there)",
            src["path"].as_str().unwrap_or("?"),
            src["line"]
        );
    }
    if !v.text.is_empty() && v.text != v.title {
        let _ = writeln!(out, "  text: {}", v.text);
    }
    if let Some(scope) = v.props["scope"].as_str() {
        let _ = writeln!(out, "  scope: {scope}");
    }
    let _ = writeln!(out, "  id: {}", v.id);
    out
}

pub fn spec_list(items: &[resonance_core::query::SpecItemView]) -> String {
    if items.is_empty() {
        return "No spec items match.\n".into();
    }
    let mut out = String::new();
    for v in items {
        let key = v
            .key
            .clone()
            .unwrap_or_else(|| v.id[..v.id.len().min(10)].to_string());
        let _ = writeln!(
            out,
            "{:<11} {:<12} {:<10} {}",
            v.ntype.to_string(),
            key,
            v.status,
            first_line(&v.title, 80)
        );
    }
    out
}
