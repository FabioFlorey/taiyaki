//! End-to-end MCP and authority boundary across distinct Unix UIDs.
#[cfg(target_os = "linux")]
#[test]
fn real_mcp_sqlite_authority_process_boundary() {
    use std::process::Command;
    let sudo = Command::new("sudo").arg("-n").arg("true").output();
    if !sudo.is_ok_and(|result| result.status.success()) {
        assert!(
            std::env::var_os("TAIYAKI_REQUIRE_ISOLATION_TEST").is_none(),
            "cross-UID integration is required but passwordless sudo is unavailable"
        );
        eprintln!("SKIPPED cross-UID integration: passwordless sudo unavailable for this caller");
        return;
    }
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/authority_e2e.py");
    let run = Command::new("python3")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_taiyaki-authority"))
        .arg(env!("CARGO_BIN_EXE_taiyaki-mcp"))
        .output()
        .expect("python3 required for OS isolation integration test");
    assert!(
        run.status.success(),
        "boundary test failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    println!("{}", String::from_utf8_lossy(&run.stdout));
}

#[test]
fn mcp_fails_closed_without_socket_and_secret_configuration() {
    use std::process::{Command, Stdio};
    let output = Command::new(env!("CARGO_BIN_EXE_taiyaki-mcp"))
        .env_remove("TAIYAKI_SOCKET_PATH")
        .env_remove("TAIYAKI_SECRET_FILE")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "MCP stdout must be protocol only");
    assert!(String::from_utf8_lossy(&output.stderr).contains("TAIYAKI_SOCKET_PATH"));
}

#[test]
fn authority_fails_closed_without_configuration() {
    use std::process::{Command, Stdio};
    let output = Command::new(env!("CARGO_BIN_EXE_taiyaki-authority"))
        .env_remove("TAIYAKI_SOCKET_PATH")
        .env_remove("TAIYAKI_DB_FILE")
        .env_remove("TAIYAKI_SECRET_FILE")
        .env_remove("TAIYAKI_CLIENT_UID")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("TAIYAKI_SOCKET_PATH"));
}
#[cfg(target_os = "linux")]
#[test]
fn real_three_uid_workitem_and_gate_verification() {
    use std::process::Command;
    let sudo = Command::new("sudo").arg("-n").arg("true").output();
    if !sudo.is_ok_and(|value| value.status.success()) {
        assert!(
            std::env::var_os("TAIYAKI_REQUIRE_ISOLATION_TEST").is_none(),
            "independent verifier test requires passwordless sudo"
        );
        eprintln!("SKIPPED independent verifier test: no passwordless sudo");
        return;
    }
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/workflow_e2e.py");
    let output = Command::new("python3")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_taiyaki-authority"))
        .arg(env!("CARGO_BIN_EXE_taiyaki-mcp"))
        .arg(env!("CARGO_BIN_EXE_taiyaki-verifier"))
        .output()
        .expect("Python3 available for cross-UID verifier test");
    assert!(
        output.status.success(),
        "real workflow failed\nstdout:{}\nstderr:{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
}

#[test]
fn authority_refuses_same_uid_even_when_configured() {
    use std::process::Command;
    let output = Command::new(env!("CARGO_BIN_EXE_taiyaki-authority"))
        .env("TAIYAKI_SOCKET_PATH", "/nonexistent/authority.sock")
        .env("TAIYAKI_DB_FILE", "/nonexistent/plans.sqlite")
        .env("TAIYAKI_SECRET_FILE", "/nonexistent/key")
        .env("TAIYAKI_CLIENT_UID", unsafe { libc::geteuid() }.to_string())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("distinct UIDs"));
}
