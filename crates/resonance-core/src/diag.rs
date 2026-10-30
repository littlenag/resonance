//! What the daemon has done since it started (FR-293, FR-294, MVP-044): structured events in a
//! bounded buffer, counters per method, and the clients connected now. Nothing here outlives
//! the daemon. Events carry identifiers, keys, paths, counts and durations, never item text
//! or file content, so that an export can go into a bug report.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::ops::hlc;

/// One thing the daemon did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    pub at: String,
    /// `request`, `client`, `reconcile`, `git`, `changeset`, `changelog`, `divergence`,
    /// `delta`, `registry`, `state`, `action` or `ui`.
    pub kind: String,
    pub summary: String,
    #[serde(default)]
    pub fields: Value,
}

/// How many durations per method are kept for the percentiles.
const SAMPLES: usize = 1000;

#[derive(Debug, Default)]
struct MethodStats {
    count: u64,
    errors: u64,
    max_ms: u64,
    total_ms: u64,
    recent: VecDeque<u64>,
}

/// A connected client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Client {
    pub id: u64,
    /// `mcp`, `tui`, `ui`, `cli`, or `socket` until its first request says which.
    pub kind: String,
    pub actor: Option<String>,
    pub connected_at: String,
    pub requests: u64,
}

struct Inner {
    seq: u64,
    events: VecDeque<Event>,
    capacity: usize,
    dropped: u64,
    methods: BTreeMap<String, MethodStats>,
    totals: BTreeMap<String, u64>,
    clients: BTreeMap<u64, Client>,
    next_client: u64,
    listeners: Vec<Sender<Value>>,
}

/// The daemon's diagnostics.
pub struct Diagnostics {
    inner: Mutex<Inner>,
}

fn now() -> String {
    hlc::rfc3339(hlc::now_ms())
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let i = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

impl Diagnostics {
    pub fn new(capacity: usize) -> Diagnostics {
        Diagnostics {
            inner: Mutex::new(Inner {
                seq: 0,
                events: VecDeque::new(),
                capacity: capacity.max(1),
                dropped: 0,
                methods: BTreeMap::new(),
                totals: BTreeMap::new(),
                clients: BTreeMap::new(),
                next_client: 0,
                listeners: Vec::new(),
            }),
        }
    }

    /// Record an event, and pass it to listeners as `{"method": "event", "params": event}`.
    pub fn record(&self, kind: &str, summary: impl Into<String>, fields: Value) {
        let mut s = self.inner.lock().expect("diagnostics");
        s.seq += 1;
        let event = Event {
            seq: s.seq,
            at: now(),
            kind: kind.to_string(),
            summary: summary.into(),
            fields,
        };
        *s.totals.entry(kind.to_string()).or_default() += 1;
        while s.events.len() >= s.capacity {
            s.events.pop_front();
            s.dropped += 1;
        }
        s.events.push_back(event.clone());
        if !s.listeners.is_empty() {
            let msg = json!({"method": "event", "params": event});
            s.listeners.retain(|l| l.send(msg.clone()).is_ok());
        }
    }

    /// A request served: counted per method, and recorded as a `request` event.
    #[allow(clippy::too_many_arguments)]
    pub fn request(
        &self,
        method: &str,
        actor: Option<&str>,
        origin: Option<&str>,
        client: Option<u64>,
        waited_ms: u64,
        duration_ms: u64,
        error: Option<&str>,
    ) {
        {
            let mut s = self.inner.lock().expect("diagnostics");
            let m = s.methods.entry(method.to_string()).or_default();
            m.count += 1;
            if error.is_some() {
                m.errors += 1;
            }
            let total = waited_ms + duration_ms;
            m.max_ms = m.max_ms.max(total);
            m.total_ms += total;
            if m.recent.len() >= SAMPLES {
                m.recent.pop_front();
            }
            m.recent.push_back(total);
            if let Some(c) = client.and_then(|id| s.clients.get_mut(&id)) {
                c.requests += 1;
                if let Some(o) = origin {
                    c.kind = o.to_string();
                }
                if let Some(a) = actor {
                    c.actor = Some(a.to_string());
                }
            }
        }
        let outcome = if error.is_some() { "failed" } else { "ok" };
        self.record(
            "request",
            format!("{method} {outcome} in {} ms", waited_ms + duration_ms),
            json!({
                "method": method,
                "actor": actor,
                "origin": origin,
                "client": client,
                "waitedMs": waited_ms,
                "durationMs": duration_ms,
                "outcome": outcome,
                "error": error,
            }),
        );
    }

    /// A client connected; returns its ID.
    pub fn connected(&self, kind: &str) -> u64 {
        let id = {
            let mut s = self.inner.lock().expect("diagnostics");
            s.next_client += 1;
            let id = s.next_client;
            s.clients.insert(
                id,
                Client {
                    id,
                    kind: kind.to_string(),
                    actor: None,
                    connected_at: now(),
                    requests: 0,
                },
            );
            id
        };
        self.record(
            "client",
            format!("client {id} connected ({kind})"),
            json!({"client": id, "change": "connected", "kind": kind}),
        );
        id
    }

    /// Name a client's kind and actor once they are known.
    pub fn identify(&self, id: u64, kind: &str, actor: Option<&str>) {
        let mut s = self.inner.lock().expect("diagnostics");
        if let Some(c) = s.clients.get_mut(&id) {
            c.kind = kind.to_string();
            if let Some(a) = actor {
                c.actor = Some(a.to_string());
            }
        }
    }

    pub fn disconnected(&self, id: u64) {
        let gone = self.inner.lock().expect("diagnostics").clients.remove(&id);
        if let Some(c) = gone {
            self.record(
                "client",
                format!(
                    "client {id} disconnected ({}, {} requests)",
                    c.kind, c.requests
                ),
                json!({"client": id, "change": "disconnected", "kind": c.kind, "requests": c.requests}),
            );
        }
    }

    /// Pass new events to `tx`.
    pub fn listen(&self, tx: Sender<Value>) {
        self.inner.lock().expect("diagnostics").listeners.push(tx);
    }

    pub fn last_seq(&self) -> u64 {
        self.inner.lock().expect("diagnostics").seq
    }

    /// Events after `since`, for a reconnecting stream; `None` when some were dropped since.
    pub fn after(&self, since: u64) -> Option<Vec<Event>> {
        let s = self.inner.lock().expect("diagnostics");
        if let Some(first) = s.events.front()
            && first.seq > since + 1
            && s.dropped > 0
        {
            return None;
        }
        Some(s.events.iter().filter(|e| e.seq > since).cloned().collect())
    }

    /// Events (after `since`, of the given kinds), counters and clients.
    pub fn snapshot(&self, since: Option<u64>, kinds: &[String]) -> Value {
        let s = self.inner.lock().expect("diagnostics");
        let events: Vec<&Event> = s
            .events
            .iter()
            .filter(|e| since.is_none_or(|n| e.seq > n))
            .filter(|e| kinds.is_empty() || kinds.contains(&e.kind))
            .collect();
        let methods: Vec<Value> = s
            .methods
            .iter()
            .map(|(name, m)| {
                let mut sorted: Vec<u64> = m.recent.iter().copied().collect();
                sorted.sort_unstable();
                json!({
                    "method": name,
                    "count": m.count,
                    "errors": m.errors,
                    "medianMs": percentile(&sorted, 0.5),
                    "p95Ms": percentile(&sorted, 0.95),
                    "maxMs": m.max_ms,
                    "totalMs": m.total_ms,
                })
            })
            .collect();
        json!({
            "events": events,
            "oldestSeq": s.events.front().map(|e| e.seq),
            "lastSeq": s.seq,
            "dropped": s.dropped,
            "capacity": s.capacity,
            "counters": {"methods": methods, "events": s.totals},
            "clients": s.clients.values().collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_bounded_and_counters_survive() {
        let d = Diagnostics::new(2);
        let c = d.connected("socket");
        d.identify(c, "mcp", Some("agent:claude"));
        for i in 0..3 {
            d.request(
                "show",
                Some("agent:claude"),
                Some("mcp"),
                Some(c),
                0,
                i,
                None,
            );
        }
        d.request("show", None, None, Some(c), 1, 1, Some("no such item"));
        let v = d.snapshot(None, &[]);
        assert_eq!(v["events"].as_array().unwrap().len(), 2);
        assert_eq!(v["dropped"], 3);
        let show = &v["counters"]["methods"][0];
        assert_eq!(show["count"], 4);
        assert_eq!(show["errors"], 1);
        assert_eq!(show["maxMs"], 2);
        assert_eq!(v["clients"][0]["requests"], 4);
        assert_eq!(v["clients"][0]["kind"], "mcp");
        assert!(d.after(0).is_none());
        d.disconnected(c);
        assert!(
            d.snapshot(None, &[])["clients"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn kinds_filter_and_listeners_receive() {
        let d = Diagnostics::new(10);
        let (tx, rx) = std::sync::mpsc::channel();
        d.listen(tx);
        d.record("reconcile", "applied 2 paths", json!({"paths": 2}));
        d.record("state", "ready", json!({}));
        let v = d.snapshot(None, &["state".to_string()]);
        assert_eq!(v["events"].as_array().unwrap().len(), 1);
        assert_eq!(rx.try_iter().count(), 2);
    }
}
