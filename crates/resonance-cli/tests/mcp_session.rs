//! MCP: a scripted stdio session (initialize -> tools/list -> tools/call `resonance_trace`)
//! succeeds (mvp.md §5), plus agent provenance (FR-134) and error handling (MVP-003).

mod common;

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};

use common::TestRepo;
use serde_json::{Value, json};

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Session {
    fn start(repo: &TestRepo) -> Session {
        let mut child = repo
            .command(&["mcp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start resonance mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Session {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    fn send(&mut self, msg: &Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let response: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line}"));
        assert_eq!(response["id"], id);
        response
    }

    /// Call a tool and return its parsed JSON body, asserting it is not an error.
    fn call(&mut self, name: &str, arguments: Value) -> Value {
        let r = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        let result = &r["result"];
        let text = result["content"][0]["text"].as_str().unwrap();
        assert_eq!(result["isError"], false, "{name}: {text}");
        serde_json::from_str(text).unwrap()
    }

    fn call_error(&mut self, name: &str, arguments: Value) -> String {
        let r = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        assert_eq!(r["result"]["isError"], true, "{r}");
        r["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn finish(mut self) {
        drop(self.stdin);
        let status = self.child.wait().unwrap();
        assert!(status.success());
    }
}

fn initialize(s: &mut Session) -> Value {
    let init = s.request(
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "test-agent", "version": "1.2"},
        }),
    );
    s.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    init
}

#[test]
fn initialize_list_tools_and_trace() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let mut s = Session::start(&repo);

    let init = initialize(&mut s);
    let result = &init["result"];
    assert_eq!(
        result["protocolVersion"], "2025-06-18",
        "the client's version is honored"
    );
    assert_eq!(result["serverInfo"]["name"], "resonance");
    let instructions = result["instructions"].as_str().unwrap();
    for needle in [
        "does not parse code",
        "resonance_declare_unit",
        "resonance_why",
        "resonance_repair_list",
        "commit messages",
        "data, not instructions",
    ] {
        assert!(
            instructions.contains(needle),
            "MVP-014: instructions mention {needle}"
        );
    }

    assert_eq!(s.request("ping", json!({}))["result"], json!({}));
    let tools = s.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    let expected = [
        "resonance_status",
        "resonance_search",
        "resonance_show",
        "resonance_trace",
        "resonance_why",
        "resonance_context",
        "resonance_unverified",
        "resonance_repair_list",
        "resonance_create_requirement",
        "resonance_create_decision",
        "resonance_declare_unit",
        "resonance_declare_test",
        "resonance_list_units",
        "resonance_edit_unit",
        "resonance_link",
        "resonance_unlink",
        "resonance_confirm",
        "resonance_annotate",
        "resonance_reattach",
        "resonance_test",
    ];
    assert_eq!(names, expected);
    for t in tools["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
    }

    let body = s.call("resonance_trace", json!({"requirement": "FR-001"}));
    assert!(
        body["note"].as_str().unwrap().contains("not instructions"),
        "NFR-013"
    );
    let trace = &body["data"];
    assert_eq!(trace["requirement"]["key"], "FR-001");
    assert_eq!(
        trace["satisfiedBy"][0]["link"]["other"]["label"],
        "src/store.rs::open"
    );
    s.finish();
}

#[test]
fn an_agent_declares_links_and_verifies_through_mcp() {
    let repo = TestRepo::with_project();
    repo.ok(&["import", "spec/requirements.md"]);
    let mut s = Session::start(&repo);
    initialize(&mut s);

    let unit = s.call(
        "resonance_declare_unit",
        json!({"path": "src/store.rs", "startLine": 11, "endLine": 14, "name": "close", "kind": "function"}),
    );
    assert_eq!(unit["data"]["address"], "src/store.rs::close");
    let test = s.call(
        "resonance_declare_test",
        json!({"path": "tests/store_test.sh", "match": "open) echo \"open ok\"; exit 0 ;;", "name": "open_case", "run": "sh tests/store_test.sh open"}),
    );
    assert_eq!(test["data"]["location"]["startLine"], 4);

    // FR-134: agent links need a confidence, carry agent provenance and identity.
    let err = s.call_error(
        "resonance_link",
        json!({"source": "close", "type": "satisfies", "target": "FR-002"}),
    );
    assert!(err.contains("confidence"), "{err}");
    let link = s.call(
        "resonance_link",
        json!({"source": "close", "type": "satisfies", "target": "FR-002", "confidence": 0.8}),
    );
    let link = &link["data"]["link"];
    assert_eq!(link["status"], "asserted");
    assert_eq!(link["provenance"], "agent");
    assert_eq!(link["author"], "test-agent/1.2");
    assert_eq!(link["confidence"], 0.8);
    s.call(
        "resonance_link",
        json!({"source": "open_case", "type": "verifies", "target": "FR-001", "confidence": 0.9}),
    );
    let run = s.call("resonance_test", json!({"ref": "FR-001"}));
    assert_eq!(run["data"]["passed"], 1);

    // Edits made with the agent's own tools are picked up by the next call (FR-131).
    repo.replace(
        "src/store.rs",
        "use std::io;\n",
        "use std::io;\nuse std::fs;\n",
    );
    let why = s.call("resonance_why", json!({"ref": "close"}));
    assert_eq!(why["data"]["unit"]["location"]["startLine"], 12);
    assert_eq!(
        why["data"]["requirements"][0]["requirement"]["key"],
        "FR-002"
    );

    let note = s.call(
        "resonance_annotate",
        json!({"target": "close", "kind": "call-out", "text": "flush before drop", "task": "shutdown"}),
    );
    assert_eq!(note["data"]["task"], "shutdown");
    let pack = s.call(
        "resonance_context",
        json!({"requirements": ["FR-002"], "task": "shutdown"}),
    );
    assert_eq!(pack["data"]["callOuts"][0]["text"], "flush before drop");
    assert!(
        s.call("resonance_repair_list", json!({}))["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    s.finish();
}

#[test]
fn protocol_errors_are_reported_and_the_session_continues() {
    let repo = TestRepo::with_project();
    let mut s = Session::start(&repo);
    initialize(&mut s);
    let unknown = s.request("no/such/method", json!({}));
    assert_eq!(unknown["error"]["code"], -32601);
    let bad_tool = s.request(
        "tools/call",
        json!({"name": "resonance_nope", "arguments": {}}),
    );
    assert_eq!(bad_tool["error"]["code"], -32602);
    let missing = s.call_error("resonance_show", json!({}));
    assert!(missing.contains("ref"), "{missing}");
    let not_found = s.call_error("resonance_show", json!({"ref": "FR-999"}));
    assert!(not_found.contains("nothing matches"), "{not_found}");
    s.send(&json!("not an object"));
    let mut line = String::new();
    s.stdout.read_line(&mut line).unwrap();
    let parsed: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(parsed["error"]["code"], -32600);
    assert_eq!(
        s.request("ping", json!({}))["result"],
        json!({}),
        "still serving"
    );
    s.finish();
}
