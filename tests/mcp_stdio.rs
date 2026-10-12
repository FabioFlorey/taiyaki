//! End-to-end MCP framing against the actual compiled stdio process.
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::fs::PermissionsExt,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};
use tempfile::TempDir;

struct McpProcess {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl McpProcess {
    fn spawn(state: &TempDir) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_taiyaki-mcp"))
            .env("TAIYAKI_PLAN_FILE", state.path().join("plan.json"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            input,
            output,
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        writeln!(
            self.input,
            "{}",
            json!({"jsonrpc":"2.0","method":method,"params":params})
        )
        .unwrap();
        self.input.flush().unwrap();
    }

    fn call(&mut self, id: u64, method: &str, params: Value) -> Value {
        let request = json!({
            "jsonrpc":"2.0",
            "id":id,
            "method":method,
            "params":params
        });
        writeln!(self.input, "{request}").unwrap();
        self.input.flush().unwrap();
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        assert!(
            !line.is_empty(),
            "server closed before responding to {request}"
        );
        let response: Value = serde_json::from_str(&line).expect("one JSON response per line");
        assert_eq!(response["id"], id);
        response
    }

    fn tool(&mut self, id: u64, name: &str, arguments: Value) -> Value {
        self.call(id, "tools/call", json!({"name":name,"arguments":arguments}))["result"].clone()
    }

    fn initialize(&mut self) {
        let answer = self.call(
            1,
            "initialize",
            json!({
                "protocolVersion":"2025-06-18",
                "capabilities":{},
                "clientInfo":{"name":"taiyaki-test","version":"1"}
            }),
        );
        assert_eq!(answer["result"]["serverInfo"]["name"], "taiyaki");
        self.notify("notifications/initialized", json!({}));
    }
}

impl Drop for McpProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn private_dir() -> TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    root
}

#[test]
fn real_sdk_stdio_lifecycle_and_persistence() {
    let root = private_dir();
    {
        let mut server = McpProcess::spawn(&root);
        server.initialize();
        let list = server.call(2, "tools/list", json!({}));
        let tools = list["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 4);
        let names: Vec<_> = tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            ["plan_create", "plan_edit", "plan_get", "plan_review"]
        );
        let missing = server.tool(3, "plan_get", json!({}));
        assert_eq!(missing["isError"], true);
        assert_eq!(missing["structuredContent"]["code"], "not_found");

        let created = server.tool(
            4,
            "plan_create",
            json!({"title":"Local planning", "description":"MCP smoke test"}),
        );
        assert_eq!(created["isError"], false);
        assert_eq!(created["structuredContent"]["revision"], 1);
        assert_eq!(created["structuredContent"]["review_state"], "draft");

        // Path and actor identity are not fields in any tool schema.
        let extra = server.tool(
            5,
            "plan_create",
            json!({"title":"Other plan", "path":"/etc/passwd"}),
        );
        assert!(extra["isError"] == true || extra.is_null());

        let added = server.tool(
            6,
            "plan_edit",
            json!({
                "expected_revision":1,
                "edit":{"action":"add_item","item":{
                    "title":"Test gated work",
                    "description":"Definition only",
                    "disposition":"ready",
                    "acceptance_criteria":["Reject stale edits"],
                    "required_gate_ids":["cargo-test"]
                }}
            }),
        );
        assert_eq!(added["isError"], false);
        assert_eq!(added["structuredContent"]["revision"], 2);
        assert_eq!(
            added["structuredContent"]["work_items"][0]["required_gate_ids"][0],
            "cargo-test"
        );

        let stale = server.tool(
            7,
            "plan_edit",
            json!({"expected_revision":1,"edit":{"action":"set_details","title":"Stale","description":""}}),
        );
        assert_eq!(stale["isError"], true);
        assert_eq!(stale["structuredContent"]["code"], "revision_conflict");

        let submitted = server.tool(
            8,
            "plan_review",
            json!({"expected_revision":2,"action":"submit"}),
        );
        assert_eq!(submitted["structuredContent"]["review_state"], "submitted");
        assert_eq!(submitted["structuredContent"]["revision"], 3);
        let illegal = server.call(
            9,
            "tools/call",
            json!({"name":"approve","arguments":{"plan_id":"anything"}}),
        );
        assert!(illegal["error"].is_object() || illegal["result"]["isError"] == true);
    }

    // The plan survives process termination. No state relies on MCP session ID.
    let mut restarted = McpProcess::spawn(&root);
    restarted.initialize();
    let loaded = restarted.tool(11, "plan_get", json!({}));
    assert_eq!(loaded["structuredContent"]["revision"], 3);
    assert_eq!(loaded["structuredContent"]["review_state"], "submitted");
    let blocked = restarted.tool(
        12,
        "plan_edit",
        json!({"expected_revision":3,"edit":{"action":"set_details","title":"bypass","description":""}}),
    );
    assert_eq!(blocked["isError"], true);
    assert_eq!(blocked["structuredContent"]["code"], "review_locked");
    let draft = restarted.tool(
        13,
        "plan_review",
        json!({"expected_revision":3,"action":"return_to_draft"}),
    );
    assert_eq!(draft["structuredContent"]["review_state"], "draft");
    assert_eq!(draft["structuredContent"]["revision"], 4);
}

#[test]
fn server_rejects_missing_plan_file_configuration() {
    let output = Command::new(env!("CARGO_BIN_EXE_taiyaki-mcp"))
        .env_remove("TAIYAKI_PLAN_FILE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "errors must not pollute MCP stdout"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("TAIYAKI_PLAN_FILE"));
}
