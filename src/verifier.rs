//! Separately deployed verifier. It reads a verifier-owned, operator-approved
//! catalog and runs only its fixed executable + fixed arguments (never MCP code).
//! The catalog approval must include a human review of semantic test relevance.
use crate::{
    authority::{Client, Request},
    planning::Plan,
    workflow::{GateObservation, Phase, TestGate, Workflow},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub source_root: PathBuf,
    pub cases: Vec<Case>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub test_id: String,
    /// Exact relative source filename. Must match TestGate.test_code.
    pub test_code: String,
    /// SHA256 of the source file reviewed by the operator.
    pub test_code_sha256: String,
    /// Digest of the full semantic Test Gate definition approved by an operator.
    pub test_definition_sha256: String,
    /// Absolute, administrator-approved executable (never from MCP).
    pub executable: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    /// Required in actual test output (prevents zero-test pass).
    pub success_marker: String,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}
fn default_timeout() -> u64 {
    30
}
fn bad(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

pub fn read_catalog(path: &Path) -> io::Result<Catalog> {
    if !path.is_absolute() {
        return Err(bad("catalog filename must be absolute"));
    }
    let parent = fs::canonicalize(path.parent().ok_or_else(|| bad("catalog parent missing"))?)?;
    let parent_meta = fs::metadata(&parent)?;
    if !parent_meta.is_dir()
        || parent_meta.uid() != unsafe { libc::geteuid() }
        || parent_meta.mode() & 0o077 != 0
    {
        return Err(bad(
            "catalog directory must be verifier-owned and mode 0700",
        ));
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file()
        || meta.nlink() != 1
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o077 != 0
    {
        return Err(bad("catalog must be an owner-private regular file"));
    }
    if meta.len() > 262_144 {
        return Err(bad("catalog too large"));
    }
    let catalog: Catalog = serde_json::from_slice(&fs::read(path)?)?;
    if !catalog.source_root.is_absolute()
        || !catalog.source_root.is_dir()
        || catalog.cases.len() > 128
    {
        return Err(bad("invalid source root or catalog size"));
    }
    let root_metadata = fs::metadata(&catalog.source_root)?;
    if root_metadata.uid() != unsafe { libc::geteuid() } || root_metadata.mode() & 0o022 != 0 {
        return Err(bad(
            "approved test checkout must be owned by verifier, not writable by agents",
        ));
    }
    let mut unique = std::collections::HashSet::new();
    for case in &catalog.cases {
        if !unique.insert(&case.test_id)
            || !case.executable.is_absolute()
            || case.args.len() > 32
            || case.args.iter().any(|a| a.len() > 1024)
            || case.success_marker.trim().is_empty()
            || case.success_marker.len() > 128
            || case.timeout_seconds == 0
            || case.timeout_seconds > 120
            || case.test_definition_sha256.len() != 64
            || !case
                .test_definition_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
            || case.test_code_sha256.len() != 64
            || !case.test_code_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(bad("invalid catalog case"));
        }
        let source = Path::new(&case.test_code);
        if source.is_absolute()
            || source
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(bad("test source path must be relative and canonical"));
        }
        let exe = fs::canonicalize(&case.executable)?;
        let executable = fs::metadata(&exe)?;
        if !executable.is_file()
            || executable.mode() & 0o022 != 0
            || !matches!(executable.uid(), 0) && executable.uid() != unsafe { libc::geteuid() }
        {
            return Err(bad(
                "catalog executable is not a trusted private/system executable",
            ));
        }
    }
    Ok(catalog)
}
fn digest(content: &[u8]) -> String {
    hex::encode(Sha256::digest(content))
}
fn git_output(root: &Path, args: &[&str]) -> io::Result<String> {
    let result = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()?;
    if !result.status.success() {
        return Err(bad("Git cannot attest the source tree"));
    }
    String::from_utf8(result.stdout).map_err(io::Error::other)
}
fn tree_digest(root: &Path) -> io::Result<String> {
    if !git_output(root, &["status", "--porcelain"])?
        .trim()
        .is_empty()
    {
        return Err(bad("source checkout is dirty"));
    }
    let sha = git_output(root, &["rev-parse", "HEAD^{tree}"])?;
    Ok(digest(sha.trim().as_bytes()))
}
fn source_digest(root: &Path, source: &str) -> io::Result<String> {
    let relative = root.join(source);
    let resolved = relative.canonicalize()?;
    let canonical_root = root.canonicalize()?;
    if !resolved.starts_with(&canonical_root) || !resolved.is_file() {
        return Err(bad("test source resolves outside approved root"));
    }
    let meta = fs::metadata(&resolved)?;
    if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o022 != 0 {
        return Err(bad(
            "test source is not an immutable verifier-owned snapshot",
        ));
    }
    if meta.len() > 1_048_576 {
        return Err(bad("test source too large"));
    }
    Ok(digest(&fs::read(&resolved)?))
}
fn collect_limited(mut stream: impl Read) -> String {
    let mut collected = Vec::new();
    let mut block = [0u8; 4096];
    loop {
        match stream.read(&mut block) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if collected.len() < 65536 {
                    let remaining = 65536 - collected.len();
                    collected.extend_from_slice(&block[..n.min(remaining)]);
                }
            }
        }
    }
    String::from_utf8_lossy(&collected).into_owned()
}
fn execute_case(root: &Path, case: &Case) -> bool {
    let mut child = match Command::new(&case.executable)
        .args(&case.args)
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(value) => value,
        Err(_) => return false,
    };
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out = thread::spawn(move || collect_limited(stdout));
    let err = thread::spawn(move || collect_limited(stderr));
    let deadline = Instant::now() + Duration::from_secs(case.timeout_seconds);
    let successful = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let combined = out.join().unwrap_or_default() + &err.join().unwrap_or_default();
    successful && combined.contains(&case.success_marker)
}
fn run_one(root: &Path, case: &Case, gate: &TestGate) -> GateObservation {
    let mut definition = gate.clone();
    definition.test_status = crate::workflow::TestStatus::NotRun;
    let definition_digest = serde_json::to_value(definition)
        .ok()
        .and_then(|v| serde_json::to_vec(&v).ok())
        .map(|data| digest(&data));
    let checked = gate.test_code == case.test_code
        && case.test_id == gate.test_id
        && definition_digest.as_deref() == Some(&case.test_definition_sha256)
        && source_digest(root, &case.test_code).ok().as_deref() == Some(&case.test_code_sha256);
    let passed = checked
        && execute_case(root, case)
        && source_digest(root, &case.test_code).ok().as_deref() == Some(&case.test_code_sha256);
    GateObservation {
        test_id: gate.test_id.clone(),
        test_code_sha256: case.test_code_sha256.clone(),
        code_valid: checked,
        passed,
    }
}
pub fn verify_pending(client: &Client, catalog: &Catalog) -> io::Result<usize> {
    let root = &catalog.source_root;
    let state = client
        .request(Request::VerifyFetch)
        .map_err(|(code, msg)| bad(&format!("{code}: {msg}")))?;
    let plan: Plan = serde_json::from_value(state["plan"].clone())?;
    let workflow: Workflow = serde_json::from_value(state["workflow"].clone())?;
    let baseline_status = tree_digest(root).ok();
    // A source change revokes older PASS receipts before new executions.
    let state = client
        .request(Request::VerifyRefresh {
            expected_revision: workflow.revision,
            source_tree_sha256: baseline_status.clone(),
        })
        .map_err(|(code, msg)| bad(&format!("{code}: {msg}")))?;
    let workflow: Workflow = serde_json::from_value(state)?;
    let baseline = baseline_status.ok_or_else(|| bad("source checkout is not clean"))?;
    let mut count = 0;
    for item in plan.work_items() {
        if workflow
            .work
            .get(&item.id)
            .is_none_or(|r| r.phase != Phase::Verifying)
        {
            continue;
        }
        let mut observations = Vec::new();
        for id in &item.required_gate_ids {
            let def = workflow
                .gates
                .get(id)
                .ok_or_else(|| bad("missing Test Gate"))?;
            let observation = if let Some(case) = catalog.cases.iter().find(|c| c.test_id == *id) {
                run_one(root, case, def)
            } else {
                GateObservation {
                    test_id: id.clone(),
                    test_code_sha256: "0".repeat(64),
                    code_valid: false,
                    passed: false,
                }
            };
            observations.push(observation);
        }
        // Reject all outcomes if the source changed during execution.
        let unchanged = tree_digest(root).is_ok_and(|after| after == baseline);
        if !unchanged {
            for row in &mut observations {
                row.passed = false;
            }
        }
        client
            .request(Request::VerifyCommit {
                expected_revision: workflow.revision + count as u64,
                item_id: item.id,
                source_tree_sha256: baseline.clone(),
                results: observations,
            })
            .map_err(|(code, msg)| bad(&format!("{code}: {msg}")))?;
        count += 1;
    }
    Ok(count)
}
