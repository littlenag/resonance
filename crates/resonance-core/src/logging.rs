//! The daemon's log (FR-296–FR-298, MVP-044): structured records with a level and a
//! component, written to a configurable, rotated file and kept in a bounded buffer that the
//! web UI reads and follows.
//!
//! Code logs through `tracing`. [`init`] installs the process's subscriber once; a daemon
//! process runs one daemon, so there is one [`Logs`] per process.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;

use crate::error::{Error, Result};
use crate::ops::hlc;
use crate::repo::Repo;
use crate::repo::config::LogConfig;

/// One log record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub seq: u64,
    pub at: String,
    /// `error`, `warn`, `info`, `debug` or `trace`.
    pub level: String,
    /// The component: `daemon`, `watch`, `ingest`, `git`, `registry`, `store`, `ui`, ...
    pub component: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub fields: Map<String, Value>,
}

/// Which records pass: a default level and per-component levels (`info,ingest=debug`).
#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    default: Level,
    components: Vec<(String, Level)>,
    text: String,
}

fn parse_level(s: &str) -> Option<Level> {
    match s.trim().to_ascii_lowercase().as_str() {
        "error" => Some(Level::ERROR),
        "warn" | "warning" => Some(Level::WARN),
        "info" => Some(Level::INFO),
        "debug" => Some(Level::DEBUG),
        "trace" => Some(Level::TRACE),
        _ => None,
    }
}

impl Filter {
    pub fn parse(text: &str) -> Result<Filter> {
        let mut f = Filter {
            default: Level::INFO,
            components: Vec::new(),
            text: text.trim().to_string(),
        };
        for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match part.split_once('=') {
                Some((component, level)) => {
                    let level = parse_level(level).ok_or_else(|| {
                        Error::Config(format!("`{level}` is not a log level (in `{text}`)"))
                    })?;
                    f.components.push((component.trim().to_string(), level));
                }
                None => {
                    f.default = parse_level(part).ok_or_else(|| {
                        Error::Config(format!(
                            "`{part}` is not a log level; use error, warn, info, debug or trace"
                        ))
                    })?;
                }
            }
        }
        if f.text.is_empty() {
            f.text = "info".into();
        }
        Ok(f)
    }

    pub fn allows(&self, component: &str, level: &Level) -> bool {
        let max = self
            .components
            .iter()
            .rev()
            .find(|(c, _)| c == component)
            .map(|(_, l)| *l)
            .unwrap_or(self.default);
        // In `tracing`, ERROR < WARN < ... < TRACE in verbosity, ordered so that ERROR is
        // the "smallest": a record passes when it is at most as verbose as the maximum.
        level <= &max
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

/// The component of a `tracing` target: its last module, or `ui` for the web server.
pub fn component(target: &str) -> String {
    if target.starts_with("resonance_web") {
        return "ui".into();
    }
    if !target.contains("::") {
        return target.to_string();
    }
    let rest = target
        .strip_prefix("resonance_core::")
        .or_else(|| target.strip_prefix("resonance_cli::"))
        .unwrap_or(target);
    let parts: Vec<&str> = rest.split("::").collect();
    // `daemon::mod` style paths name the module; `history::fold` names `git` work.
    match parts.first().copied() {
        Some("history") => "git".into(),
        Some("daemon") => parts.get(1).copied().unwrap_or("daemon").into(),
        Some(first) => first.into(),
        None => "resonance".into(),
    }
}

/// The log file, rotated past a size.
struct LogFile {
    path: PathBuf,
    file: File,
    max_bytes: u64,
    json: bool,
}

impl LogFile {
    fn open(path: &Path, max_bytes: u64, json: bool) -> std::io::Result<LogFile> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(LogFile {
            path: path.to_path_buf(),
            file,
            max_bytes,
            json,
        })
    }

    fn write(&mut self, r: &Record) {
        let line = if self.json {
            serde_json::to_string(r).unwrap_or_default()
        } else {
            text_line(r)
        };
        let len = self.file.metadata().map(|m| m.len()).unwrap_or(0);
        if self.max_bytes > 0 && len > 0 && len + line.len() as u64 + 1 > self.max_bytes {
            let rotated = rotated_path(&self.path);
            let _ = fs::rename(&self.path, rotated);
            if let Ok(f) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
            {
                self.file = f;
            }
        }
        let _ = writeln!(self.file, "{line}");
    }
}

/// `<file>.1`, where the previous log goes.
pub fn rotated_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".1");
    PathBuf::from(s)
}

/// A record as one line of text: `time LEVEL component: message key=value ...`.
pub fn text_line(r: &Record) -> String {
    let mut line = format!(
        "{} {:<5} {}: {}",
        r.at,
        r.level.to_uppercase(),
        r.component,
        r.message
    );
    for (k, v) in &r.fields {
        let v = match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let _ = write!(line, " {k}={v}");
    }
    line
}

struct State {
    seq: u64,
    buffer: VecDeque<Record>,
    capacity: usize,
    dropped: u64,
    listeners: Vec<Sender<Value>>,
    file: Option<LogFile>,
}

/// The daemon's log: the filter, the file and the in-memory buffer.
pub struct Logs {
    filter: RwLock<Filter>,
    state: Mutex<State>,
    /// Where records go, for the diagnostics view.
    target: Mutex<String>,
}

impl Logs {
    fn new() -> Logs {
        Logs {
            filter: RwLock::new(Filter::parse("info").expect("a valid filter")),
            state: Mutex::new(State {
                seq: 0,
                buffer: VecDeque::new(),
                capacity: 5000,
                dropped: 0,
                listeners: Vec::new(),
                file: None,
            }),
            target: Mutex::new(String::new()),
        }
    }

    /// The filter in effect.
    pub fn level(&self) -> String {
        self.filter.read().expect("filter").text().to_string()
    }

    /// Change the level for the rest of the daemon's life (FR-295, FR-298).
    pub fn set_level(&self, text: &str) -> Result<String> {
        let f = Filter::parse(text)?;
        let shown = f.text().to_string();
        *self.filter.write().expect("filter") = f;
        Ok(shown)
    }

    /// The log file in use.
    pub fn file(&self) -> String {
        self.target.lock().expect("target").clone()
    }

    pub fn enabled(&self, component: &str, level: &Level) -> bool {
        self.filter.read().expect("filter").allows(component, level)
    }

    /// Keep a record, write it to the file and pass it to listeners.
    pub fn push(&self, mut r: Record) {
        let mut s = self.state.lock().expect("logs");
        s.seq += 1;
        r.seq = s.seq;
        if let Some(f) = s.file.as_mut() {
            f.write(&r);
        }
        if s.capacity > 0 {
            while s.buffer.len() >= s.capacity {
                s.buffer.pop_front();
                s.dropped += 1;
            }
            s.buffer.push_back(r.clone());
        }
        if !s.listeners.is_empty() {
            let msg = json!({"method": "log", "params": r});
            s.listeners.retain(|l| l.send(msg.clone()).is_ok());
        }
    }

    /// Records after `since`, at `min_level` or more severe, and how many were dropped.
    pub fn read(&self, since: Option<u64>, min_level: Option<&str>) -> Value {
        let min = min_level.and_then(parse_level);
        let s = self.state.lock().expect("logs");
        let records: Vec<&Record> = s
            .buffer
            .iter()
            .filter(|r| since.is_none_or(|n| r.seq > n))
            .filter(|r| min.is_none_or(|m| parse_level(&r.level).is_some_and(|l| l <= m)))
            .collect();
        json!({
            "records": records,
            "oldestSeq": s.buffer.front().map(|r| r.seq),
            "lastSeq": s.seq,
            "dropped": s.dropped,
            "level": self.level(),
            "file": self.file(),
        })
    }

    /// Records after `since`, for a reconnecting stream; `None` when some were dropped since.
    pub fn after(&self, since: u64) -> Option<Vec<Record>> {
        let s = self.state.lock().expect("logs");
        if let Some(first) = s.buffer.front()
            && first.seq > since + 1
            && s.dropped > 0
        {
            return None;
        }
        Some(s.buffer.iter().filter(|r| r.seq > since).cloned().collect())
    }

    pub fn last_seq(&self) -> u64 {
        self.state.lock().expect("logs").seq
    }

    /// Pass new records to `tx` as `{"method": "log", "params": record}`.
    pub fn listen(&self, tx: Sender<Value>) {
        self.state.lock().expect("logs").listeners.push(tx);
    }

    /// Apply the configuration: level, buffer size and file. Returns a note when the
    /// configured file was refused and the default is used instead.
    pub fn configure(&self, repo: &Repo, cfg: &LogConfig) -> Option<String> {
        let mut note = None;
        match Filter::parse(&cfg.level) {
            Ok(f) => *self.filter.write().expect("filter") = f,
            Err(e) => note = Some(format!("{e}; logging at info")),
        }
        let default = default_path(repo);
        let (path, refused) = match resolve_file(repo, &cfg.file) {
            Ok(p) => (p, None),
            Err(why) => (default.clone(), Some(why)),
        };
        let json = cfg.format.eq_ignore_ascii_case("json");
        let (file, path, open_error) = match LogFile::open(&path, cfg.max_bytes, json) {
            Ok(f) => (Some(f), path, None),
            Err(e) if path != default => (
                LogFile::open(&default, cfg.max_bytes, json).ok(),
                default.clone(),
                Some(format!("could not open {}: {e}", path.display())),
            ),
            Err(_) => (None, path, None),
        };
        {
            let mut s = self.state.lock().expect("logs");
            s.capacity = cfg.buffer_lines;
            while s.buffer.len() > s.capacity {
                s.buffer.pop_front();
                s.dropped += 1;
            }
            s.file = file;
        }
        *self.target.lock().expect("target") = path.display().to_string();
        if let Some(why) = refused.or(open_error) {
            let msg = format!("{why}; logging to {} instead", default.display());
            note = Some(match note {
                Some(n) => format!("{n}; {msg}"),
                None => msg,
            });
        }
        note
    }
}

/// `.resonance/local/daemon.log`.
pub fn default_path(repo: &Repo) -> PathBuf {
    repo.local_dir().join("daemon.log")
}

/// The configured log file: relative to the repository root, with `~` expanded. A file in
/// the working tree outside `.resonance/local/` is refused: it would be ingested and show in
/// `git status` (NFR-001).
pub fn resolve_file(repo: &Repo, configured: &str) -> std::result::Result<PathBuf, String> {
    let configured = configured.trim();
    if configured.is_empty() {
        return Ok(default_path(repo));
    }
    let path = if let Some(rest) = configured.strip_prefix("~/") {
        match std::env::var_os("HOME") {
            Some(h) => PathBuf::from(h).join(rest),
            None => return Err("log.file starts with ~ but HOME is not set".into()),
        }
    } else if Path::new(configured).is_absolute() {
        PathBuf::from(configured)
    } else {
        repo.root.join(configured)
    };
    let path = crate::repo::lexical_normalize(&path);
    let root = fs::canonicalize(&repo.root).unwrap_or_else(|_| repo.root.clone());
    let local = root.join(".resonance").join("local");
    let parent = path
        .parent()
        .map(|p| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()))
        .unwrap_or_default();
    let full = parent.join(path.file_name().unwrap_or_default());
    let inside = full.starts_with(&root) || path.starts_with(&repo.root);
    if inside && !(full.starts_with(&local) || path.starts_with(repo.local_dir())) {
        return Err(format!(
            "log.file {} is inside the working tree but outside .resonance/local/, where it would be ingested and show in git status",
            path.display()
        ));
    }
    Ok(path)
}

/// The process's log. Records reach it once [`init`] has installed the subscriber.
pub fn logs() -> Arc<Logs> {
    static LOGS: OnceLock<Arc<Logs>> = OnceLock::new();
    Arc::clone(LOGS.get_or_init(|| Arc::new(Logs::new())))
}

/// Install the subscriber (once per process) and apply `cfg`. Returns the log and a note
/// when the configuration could not be followed exactly.
pub fn init(repo: &Repo, cfg: &LogConfig) -> (Arc<Logs>, Option<String>) {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    let logs = logs();
    let note = logs.configure(repo, cfg);
    INSTALLED.get_or_init(|| {
        let layer = BufferLayer {
            logs: Arc::clone(&logs),
        };
        let _ = tracing_subscriber::registry().with(layer).try_init();
    });
    (logs, note)
}

struct BufferLayer {
    logs: Arc<Logs>,
}

impl<S: Subscriber> Layer<S> for BufferLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let component = component(meta.target());
        if !self.logs.enabled(&component, meta.level()) {
            return;
        }
        let mut visitor = Fields::default();
        event.record(&mut visitor);
        self.logs.push(Record {
            seq: 0,
            at: hlc::rfc3339(hlc::now_ms()),
            level: meta.level().as_str().to_ascii_lowercase(),
            component,
            message: visitor.message,
            fields: visitor.fields,
        });
    }
}

#[derive(Default)]
struct Fields {
    message: String,
    fields: Map<String, Value>,
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields.insert(field.name().into(), json!(value));
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields.insert(field.name().into(), json!(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields.insert(field.name().into(), json!(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields.insert(field.name().into(), json!(value));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        } else {
            self.fields
                .insert(field.name().into(), json!(format!("{value:?}")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(level: &str, component: &str, message: &str) -> Record {
        Record {
            seq: 0,
            at: "2026-10-30T00:00:00.000Z".into(),
            level: level.into(),
            component: component.into(),
            message: message.into(),
            fields: Map::new(),
        }
    }

    #[test]
    fn filters_by_level_and_component() {
        let f = Filter::parse("warn,ingest=debug").unwrap();
        assert!(f.allows("daemon", &Level::ERROR));
        assert!(!f.allows("daemon", &Level::INFO));
        assert!(f.allows("ingest", &Level::DEBUG));
        assert!(!f.allows("ingest", &Level::TRACE));
        assert!(Filter::parse("loud").is_err());
        assert_eq!(Filter::parse("").unwrap().text(), "info");
    }

    #[test]
    fn components_come_from_targets() {
        assert_eq!(component("resonance_core::daemon"), "daemon");
        assert_eq!(component("resonance_core::daemon::watch"), "watch");
        assert_eq!(component("resonance_core::ingest::reconcile"), "ingest");
        assert_eq!(component("resonance_core::history::fold"), "git");
        assert_eq!(component("resonance_web::server"), "ui");
        assert_eq!(component("registry"), "registry");
    }

    #[test]
    fn buffer_is_bounded_and_files_rotate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("d.log");
        let logs = Logs::new();
        {
            let mut s = logs.state.lock().unwrap();
            s.capacity = 3;
            s.file = Some(LogFile::open(&path, 200, false).unwrap());
        }
        for i in 0..5 {
            logs.push(record("info", "daemon", &format!("message number {i}")));
        }
        let v = logs.read(None, None);
        assert_eq!(v["records"].as_array().unwrap().len(), 3);
        assert_eq!(v["dropped"], 2);
        assert_eq!(v["oldestSeq"], 3);
        assert!(logs.after(0).is_none(), "a gap is reported");
        assert_eq!(logs.after(3).unwrap().len(), 2);
        assert!(rotated_path(&path).exists(), "the file rotated");
        let current = fs::read_to_string(&path).unwrap();
        assert!(current.contains("message number 4"));
        assert!(current.len() <= 200);
    }

    #[test]
    fn min_level_reads_only_more_severe() {
        let logs = Logs::new();
        logs.push(record("debug", "daemon", "a"));
        logs.push(record("warn", "daemon", "b"));
        logs.push(record("error", "daemon", "c"));
        let v = logs.read(None, Some("warn"));
        let msgs: Vec<&str> = v["records"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["message"].as_str().unwrap())
            .collect();
        assert_eq!(msgs, ["b", "c"]);
    }

    #[test]
    fn log_files_in_the_working_tree_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir_all(root.join(".resonance/local")).unwrap();
        let repo = Repo { root: root.clone() };
        assert_eq!(resolve_file(&repo, "").unwrap(), default_path(&repo));
        assert!(resolve_file(&repo, "logs/d.log").is_err());
        assert!(resolve_file(&repo, ".resonance/local/other.log").is_ok());
        assert!(resolve_file(&repo, "/tmp/resonance-test.log").is_ok());
    }
}
