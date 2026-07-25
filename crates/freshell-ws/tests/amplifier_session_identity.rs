//! Launcher-assigned amplifier session identity — wire + disk + argv proof.
//!
//! ONE test fn (env vars are process-global; mirrors
//! `codex_session_ref_resume.rs`'s phase discipline). Fake `amplifier` is a
//! recording sh script installed via AMPLIFIER_CMD; FRESHELL_AMPLIFIER_HOME
//! is the shared isolated harness home (the broker's single home resolution
//! — Task 2's retarget — never consults AMPLIFIER_HOME; validated F1).

mod common;
use common::*;

use futures_util::SinkExt;
use serde_json::json;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

/// The shipped amplifier CLI spec shape (extensions/amplifier/freshell.json):
/// `resume_args: ["resume", "{{sessionId}}"]` and an HONORED `AMPLIFIER_CMD`
/// override. The shared harness's sleeper spec keeps `env_var: None` (so
/// ambient dev-shell env never leaks into other tests), which would leave the
/// recording fake below unreachable — this test registers its own spec via
/// `spawn_server_with_specs` (the `codex_session_ref_resume.rs` discipline).
fn amplifier_cli_spec() -> freshell_platform::CliCommandSpec {
    freshell_platform::CliCommandSpec {
        name: "amplifier".to_string(),
        label: "Amplifier CLI".to_string(),
        env_var: Some("AMPLIFIER_CMD".to_string()),
        default_cmd: "amplifier".to_string(),
        resume_args: Some(vec!["resume".to_string(), "{{sessionId}}".to_string()]),
        ..Default::default()
    }
}

fn write_fake_amplifier() -> std::path::PathBuf {
    let script_path = std::env::temp_dir().join(format!(
        "freshell-fake-amplifier-{}.sh",
        std::process::id()
    ));
    let script = "#!/bin/sh\n\
        printf '%s\\n' \"$@\" > \"$AMPLIFIER_ARGV_CAPTURE_PATH.tmp\"\n\
        mv \"$AMPLIFIER_ARGV_CAPTURE_PATH.tmp\" \"$AMPLIFIER_ARGV_CAPTURE_PATH\"\n\
        exec sleep 300\n";
    std::fs::write(&script_path, script).expect("write fake amplifier");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();
    }
    script_path
}

fn wait_for_captured_argv(path: &std::path::Path) -> Vec<String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if let Ok(raw) = std::fs::read_to_string(path) {
            return raw.lines().map(str::to_string).collect();
        }
        assert!(std::time::Instant::now() < deadline, "argv capture never appeared: {path:?}");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Send a terminal.create and return the first terminal.created OR error
/// frame whose requestId matches.
async fn create_amplifier_terminal(
    ws: &mut TestWs,
    request_id: &str,
    extra: serde_json::Value,
) -> serde_json::Value {
    let mut msg = json!({
        "type": "terminal.create",
        "requestId": request_id,
        "mode": "amplifier",
        "shell": "system",
    });
    if let (Some(base), Some(extra)) = (msg.as_object_mut(), extra.as_object()) {
        for (k, v) in extra {
            base.insert(k.clone(), v.clone());
        }
    }
    ws.send(WsMessage::Text(msg.to_string()))
        .await
        .expect("send terminal.create");
    for _ in 0..40 {
        let frame = next_frame_of_type_or_error(ws).await;
        let matches_req = frame["requestId"] == json!(request_id);
        let is_terminal = frame["type"] == json!("terminal.created") || frame["type"] == json!("error");
        if matches_req && is_terminal {
            return frame;
        }
    }
    panic!("no terminal.created/error for {request_id}");
}

// Helper: like common::next_frame_of_type but returns ANY frame so error
// frames are observable. Add to this file (common's next_frame_of_type
// panics on unmatched types after 20 frames).
async fn next_frame_of_type_or_error(ws: &mut TestWs) -> serde_json::Value {
    use futures_util::StreamExt;
    loop {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(20), ws.next())
            .await
            .expect("frame timeout")
            .expect("stream open")
            .expect("ws ok");
        if let WsMessage::Text(text) = msg {
            return serde_json::from_str(&text).expect("json frame");
        }
    }
}

fn session_dir_for(home: &std::path::Path, session_id: &str) -> Option<std::path::PathBuf> {
    let projects = home.join("projects");
    for entry in std::fs::read_dir(projects).ok()?.flatten() {
        let candidate = entry.path().join("sessions").join(session_id);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

#[tokio::test(flavor = "multi_thread")]
#[cfg(unix)]
async fn amplifier_creates_carry_launcher_assigned_identity() {
    let home = isolate_amplifier_home().to_path_buf();
    let fake = write_fake_amplifier();
    std::env::set_var("AMPLIFIER_CMD", &fake);

    let (ws_url, registry) = spawn_server_with_specs(vec![amplifier_cli_spec()]).await;
    let (mut ws, _inventory) = connect_and_capture_inventory(&ws_url).await;

    // ── Phase 1: FRESH create → server-minted identity, stub on disk, resume argv.
    let cwd = std::env::temp_dir().join(format!("amp-id-cwd-{}", std::process::id()));
    std::fs::create_dir_all(&cwd).unwrap();
    let canonical_cwd = std::fs::canonicalize(&cwd).unwrap();
    let capture = std::env::temp_dir().join(format!(
        "freshell-amp-argv-fresh-{}.txt",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&capture);
    std::env::set_var("AMPLIFIER_ARGV_CAPTURE_PATH", &capture);

    let created = create_amplifier_terminal(
        &mut ws,
        "req-amp-fresh",
        json!({ "cwd": cwd.to_string_lossy() }),
    )
    .await;
    assert_eq!(created["type"], json!("terminal.created"), "{created}");
    let terminal_id = created["terminalId"].as_str().unwrap().to_string();
    let session_ref = session_ref_of(&created)
        .unwrap_or_else(|| panic!("fresh amplifier terminal.created must carry sessionRef: {created}"));
    assert_eq!(session_ref["provider"], json!("amplifier"));
    let session_id = session_ref["sessionId"].as_str().unwrap().to_string();
    // Server-minted UUID shape (the client sent nothing).
    assert_eq!(session_id.len(), 36, "uuid shape: {session_id}");
    assert_eq!(session_id.chars().filter(|c| *c == '-').count(), 4);

    // Stub exists BEFORE/at spawn, under the canonical cwd's slug.
    let expected_slug =
        freshell_sessions::amplifier_stub::cwd_slug(&canonical_cwd.to_string_lossy());
    let dir = session_dir_for(&home, &session_id).expect("stub dir on disk");
    assert_eq!(
        dir.parent().unwrap().parent().unwrap().file_name().unwrap().to_str().unwrap(),
        expected_slug,
        "HARD INVARIANT: stub slug must be the spawn cwd's slug"
    );
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("metadata.json")).unwrap()).unwrap();
    assert_eq!(meta["session_id"], json!(session_id));
    assert_eq!(meta["working_dir"], json!(canonical_cwd.to_string_lossy()));
    assert_eq!(meta["freshell_terminal_id"], json!(terminal_id));
    assert_eq!(std::fs::metadata(dir.join("transcript.jsonl")).unwrap().len(), 0);
    assert!(dir.join("events.jsonl").is_file());

    // Spawned argv is `resume <uuid>` (manifest resumeArgs template).
    let argv = wait_for_captured_argv(&capture);
    assert_eq!(argv, vec!["resume".to_string(), session_id.clone()], "argv: {argv:?}");

    // Registry meta records the resume id (restore-across-restart identity).
    let row = registry
        .identity_probe_rows()
        .into_iter()
        .find(|r| r.terminal_id == terminal_id)
        .expect("registry row");
    assert_eq!(row.resume_session_id.as_deref(), Some(session_id.as_str()));

    registry.kill(&terminal_id);
    std::env::remove_var("AMPLIFIER_ARGV_CAPTURE_PATH");
    std::env::remove_var("AMPLIFIER_CMD");
}
