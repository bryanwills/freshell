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

    // ── Phase 2 (plan §10): `terminal:`-prefixed sessionRef is the old
    // correlation bug's poisoned persisted state — reject instead of
    // spawning a doomed `amplifier resume terminal:<hex>`.
    let rejected = create_amplifier_terminal(
        &mut ws,
        "req-amp-poisoned",
        json!({
            "cwd": cwd.to_string_lossy(),
            "sessionRef": { "provider": "amplifier", "sessionId": "terminal:deadbeef" },
        }),
    )
    .await;
    assert_eq!(rejected["type"], json!("error"), "{rejected}");
    assert!(
        rejected["message"].as_str().unwrap_or_default().contains("terminal:"),
        "reject names the synthetic id: {rejected}"
    );

    // ── Phase 3 (plan §11): same-id double-resume guard. First resume-create
    // of X succeeds (ensure-stub writes the dir); a second concurrent one is
    // rejected while the first is live.
    let resumed_id = "99999999-8888-7777-6666-555555555555";
    let capture2 = std::env::temp_dir().join(format!(
        "freshell-amp-argv-resume-{}.txt",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&capture2);
    std::env::set_var("AMPLIFIER_ARGV_CAPTURE_PATH", &capture2);
    let first = create_amplifier_terminal(
        &mut ws,
        "req-amp-resume-1",
        json!({
            "cwd": cwd.to_string_lossy(),
            "sessionRef": { "provider": "amplifier", "sessionId": resumed_id },
        }),
    )
    .await;
    assert_eq!(first["type"], json!("terminal.created"), "{first}");
    let first_tid = first["terminalId"].as_str().unwrap().to_string();
    // ensure-stub created the dir for the requested id.
    assert!(session_dir_for(&home, resumed_id).is_some());
    let dup = create_amplifier_terminal(
        &mut ws,
        "req-amp-resume-2",
        json!({
            "cwd": cwd.to_string_lossy(),
            "sessionRef": { "provider": "amplifier", "sessionId": resumed_id },
        }),
    )
    .await;
    assert_eq!(dup["type"], json!("error"), "double-resume must be rejected: {dup}");
    assert!(dup["message"].as_str().unwrap_or_default().contains(resumed_id));
    registry.kill(&first_tid);

    // ── Phase 4 (plan §8): stub GC. Phase 3's `first` terminal was a
    // zero-turn CREATED stub — killing it must delete the dir.
    {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while session_dir_for(&home, resumed_id).is_some() {
            assert!(
                std::time::Instant::now() < deadline,
                "never-used stub must be GC'd on exit"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    // ── Phase 5 (plan §8 tolerance + used-session survival): a USED session
    // survives exit. Create fresh, stamp the "used" signature, kill.
    let capture3 = std::env::temp_dir().join(format!(
        "freshell-amp-argv-used-{}.txt",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&capture3);
    std::env::set_var("AMPLIFIER_ARGV_CAPTURE_PATH", &capture3);
    let used = create_amplifier_terminal(
        &mut ws,
        "req-amp-used",
        json!({ "cwd": cwd.to_string_lossy() }),
    )
    .await;
    assert_eq!(used["type"], json!("terminal.created"));
    let used_tid = used["terminalId"].as_str().unwrap().to_string();
    let used_sid = session_ref_of(&used).unwrap()["sessionId"].as_str().unwrap().to_string();
    let used_dir = session_dir_for(&home, &used_sid).expect("used stub dir");
    // Simulate amplifier's first-turn save (the real-CLI contract test pins
    // that a real turn writes turn_count).
    let mut meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(used_dir.join("metadata.json")).unwrap())
            .unwrap();
    meta["turn_count"] = json!(1);
    std::fs::write(used_dir.join("metadata.json"), meta.to_string()).unwrap();
    std::fs::write(used_dir.join("transcript.jsonl"), "{\"role\":\"user\"}\n").unwrap();
    registry.kill(&used_tid);
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(
        session_dir_for(&home, &used_sid).is_some(),
        "used sessions must survive exit"
    );

    // ── Phase 6 (ensure-after-GC): resuming the Phase-3 id (whose stub was
    // GC'd in Phase 4) re-stubs it under the same id — restore keeps working
    // for never-used panes across restarts.
    let capture4 = std::env::temp_dir().join(format!(
        "freshell-amp-argv-regc-{}.txt",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&capture4);
    std::env::set_var("AMPLIFIER_ARGV_CAPTURE_PATH", &capture4);
    let restored = create_amplifier_terminal(
        &mut ws,
        "req-amp-restore-after-gc",
        json!({
            "cwd": cwd.to_string_lossy(),
            "sessionRef": { "provider": "amplifier", "sessionId": resumed_id },
        }),
    )
    .await;
    assert_eq!(restored["type"], json!("terminal.created"), "{restored}");
    assert!(session_dir_for(&home, resumed_id).is_some(), "re-stubbed after GC");
    let argv4 = wait_for_captured_argv(&capture4);
    assert_eq!(argv4, vec!["resume".to_string(), resumed_id.to_string()]);
    registry.kill(restored["terminalId"].as_str().unwrap());

    // ── Phase 7: spawn-failure WORDING discrimination. A launcher-minted
    // FRESH create carries a resume id but is not a restore — its spawn
    // failure must keep the legacy "Could not start" wording (pinned e2e by
    // term28-path-shadow-rust.spec.ts); only a genuine user-requested
    // restore (sessionRef) reads as "Could not restore".
    std::env::set_var("AMPLIFIER_CMD", "totally-missing-amplifier-cli-identity-test");
    let fresh_fail = create_amplifier_terminal(
        &mut ws,
        "req-amp-fresh-spawn-fail",
        json!({ "cwd": cwd.to_string_lossy() }),
    )
    .await;
    assert_eq!(fresh_fail["type"], json!("error"), "{fresh_fail}");
    let fresh_msg = fresh_fail["message"].as_str().unwrap_or_default();
    assert!(
        fresh_msg.starts_with("Could not start Amplifier CLI:"),
        "launcher-minted fresh spawn failure must read as start: {fresh_msg}"
    );
    let restore_fail = create_amplifier_terminal(
        &mut ws,
        "req-amp-restore-spawn-fail",
        json!({
            "cwd": cwd.to_string_lossy(),
            "sessionRef": { "provider": "amplifier", "sessionId": "44444444-3333-2222-1111-000000000000" },
        }),
    )
    .await;
    assert_eq!(restore_fail["type"], json!("error"), "{restore_fail}");
    let restore_msg = restore_fail["message"].as_str().unwrap_or_default();
    assert!(
        restore_msg.starts_with("Could not restore Amplifier CLI:"),
        "genuine restore spawn failure must read as restore: {restore_msg}"
    );

    std::env::remove_var("AMPLIFIER_ARGV_CAPTURE_PATH");
    std::env::remove_var("AMPLIFIER_CMD");
}
