//! P0.3 integration: `terminal.codex.candidate.persisted` handling.
//! Campaign plan §2.3.1: four guards; reject = WARN + ignore, nothing sent back.

mod common;

use common::{
    connect_and_capture_inventory, next_frame_of_type, sleeper_cli_spec, spawn_server_with_specs,
};
use futures_util::SinkExt;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message as WsMessage;

const THREAD_A: &str = "0192aaaa-bbbb-4ccc-8ddd-eeeeffff0001";
const THREAD_B: &str = "0192aaaa-bbbb-4ccc-8ddd-eeeeffff0002";

/// Fake codex: records argv to $CODEX_ARGV_CAPTURE_PATH (atomic tmp+mv) then
/// sleeps. Copied from tests/codex_session_ref_resume.rs:85-103.
fn write_fake_codex() -> std::path::PathBuf {
    let script_path = std::env::temp_dir().join(format!(
        "freshell-codex-candidate-fake-{}.sh",
        std::process::id()
    ));
    let script = "#!/bin/sh\n\
        printf '%s\\n' \"$@\" > \"$CODEX_ARGV_CAPTURE_PATH.tmp\"\n\
        mv \"$CODEX_ARGV_CAPTURE_PATH.tmp\" \"$CODEX_ARGV_CAPTURE_PATH\"\n\
        exec sleep 300\n";
    std::fs::write(&script_path, script).expect("write fake codex script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();
    }
    script_path
}

fn codex_capture_spec() -> freshell_platform::CliCommandSpec {
    freshell_platform::CliCommandSpec {
        name: "codex".to_string(),
        label: "Codex CLI".to_string(),
        env_var: None,
        default_cmd: write_fake_codex().to_string_lossy().to_string(),
        base_args: vec![],
        base_env: std::collections::BTreeMap::new(),
        // Real codex manifest shape: resume subcommand, no createSessionArgs.
        resume_args: Some(vec!["resume".to_string(), "{{sessionId}}".to_string()]),
        create_session_args: None,
        model_args: None,
        sandbox_args: None,
        permission_mode_args: None,
    }
}

fn registry_resume_id(
    registry: &freshell_terminal::TerminalRegistry,
    terminal_id: &str,
) -> Option<String> {
    registry
        .identity_probe_rows()
        .into_iter()
        .find(|row| row.terminal_id == terminal_id)
        .unwrap_or_else(|| panic!("registry must list {terminal_id}"))
        .resume_session_id
}

async fn send_create(
    ws: &mut common::TestWs,
    request_id: &str,
    mode: &str,
    extra: serde_json::Value,
) {
    let mut msg = json!({
        "type": "terminal.create",
        "requestId": request_id,
        "mode": mode,
        "shell": "system",
        "cwd": std::env::temp_dir().to_string_lossy(),
    });
    if let (Some(obj), Some(extra_obj)) = (msg.as_object_mut(), extra.as_object()) {
        for (k, v) in extra_obj {
            obj.insert(k.clone(), v.clone());
        }
    }
    ws.send(WsMessage::Text(msg.to_string()))
        .await
        .expect("send terminal.create");
}

/// Plain send, NO sync gate. Used by the HAPPY PATH only, which proves
/// consumption by awaiting the broadcasts themselves.
async fn send_candidate(
    ws: &mut common::TestWs,
    terminal_id: &str,
    thread_id: &str,
    rollout_path: &str,
) {
    ws.send(WsMessage::Text(
        json!({
            "type": "terminal.codex.candidate.persisted",
            "terminalId": terminal_id,
            "candidateThreadId": thread_id,
            "rolloutPath": rollout_path,
            "capturedAt": 1_753_300_000_000i64,
        })
        .to_string(),
    ))
    .await
    .expect("send candidate");
}

/// Send a candidate that must be REJECTED: the ping/pong round-trip proves
/// the frame was consumed AND that nothing was sent back (silence proof --
/// precedent: pane_reconcile.rs:230-250 uses exactly this to prove nothing
/// was sent). NEVER use this on the accept path: `next_frame_of_type`
/// permanently DROPS mismatched frames (tests/common/mod.rs:327-342), and the
/// connection loop is one unbiased `tokio::select!`, so broadcasts queued
/// during candidate handling commonly hit the wire BEFORE the pong --
/// awaiting the pong first would eat the association broadcasts.
async fn send_candidate_expect_silence(
    ws: &mut common::TestWs,
    terminal_id: &str,
    thread_id: &str,
    rollout_path: &str,
) {
    send_candidate(ws, terminal_id, thread_id, rollout_path).await;
    ws.send(WsMessage::Text(json!({"type": "ping"}).to_string()))
        .await
        .expect("send ping");
    let _pong = next_frame_of_type(ws, "pong").await;
}

fn wait_for_captured_argv(path: &std::path::Path) -> Vec<String> {
    for _ in 0..100 {
        if let Ok(contents) = std::fs::read_to_string(path) {
            return contents.lines().map(str::to_string).collect();
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("fake codex never wrote argv capture at {}", path.display());
}

#[tokio::test(flavor = "multi_thread")]
#[cfg(unix)]
async fn codex_candidate_persisted_guards_and_happy_path() {
    // ---- env setup (single sequential test: this binary owns process env) ----
    let codex_home = tempfile::tempdir().expect("codex home");
    let sessions_day = codex_home
        .path()
        .join("sessions")
        .join("2026")
        .join("07")
        .join("24");
    std::fs::create_dir_all(&sessions_day).expect("sessions tree");
    std::env::set_var("CODEX_HOME", codex_home.path());
    std::env::remove_var("FRESHELL_CODEX_MANAGED_LAUNCH");
    let capture =
        std::env::temp_dir().join(format!("codex-candidate-argv-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&capture);
    std::env::set_var("CODEX_ARGV_CAPTURE_PATH", &capture);

    let (url, registry) =
        spawn_server_with_specs(vec![sleeper_cli_spec("claude"), codex_capture_spec()]).await;
    let (mut ws, _inventory) = connect_and_capture_inventory(&url).await;

    // A codex terminal with NO identity yet (fresh create, no resume).
    send_create(&mut ws, "req-codex-cand-1", "codex", json!({})).await;
    let created = next_frame_of_type(&mut ws, "terminal.created").await;
    let codex_tid = created["terminalId"]
        .as_str()
        .expect("terminalId")
        .to_string();
    assert_eq!(
        registry_resume_id(&registry, &codex_tid),
        None,
        "fresh codex must start unbound"
    );

    // A valid on-disk rollout for THREAD_A: first line is the session_meta
    // header whose payload.id is the rollout's OWN id (guard 4's contract).
    let rollout_a = sessions_day.join(format!("rollout-2026-07-24T12-00-00-{THREAD_A}.jsonl"));
    std::fs::write(
        &rollout_a,
        format!("{{\"timestamp\":\"2026-07-24T12:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{THREAD_A}\"}}}}\n"),
    )
    .unwrap();
    let rollout_a = rollout_a.to_string_lossy().to_string();

    // ---- Guard 1: unknown terminal is ignored ----
    send_candidate_expect_silence(&mut ws, "no-such-terminal", THREAD_A, &rollout_a).await;
    assert_eq!(registry_resume_id(&registry, &codex_tid), None);

    // ---- Guard 2: non-codex terminal is ignored ----
    send_create(&mut ws, "req-claude-cand-1", "claude", json!({})).await;
    let claude_created = next_frame_of_type(&mut ws, "terminal.created").await;
    let claude_tid = claude_created["terminalId"]
        .as_str()
        .expect("terminalId")
        .to_string();
    send_candidate_expect_silence(&mut ws, &claude_tid, THREAD_A, &rollout_a).await;
    assert_ne!(
        registry_resume_id(&registry, &claude_tid).as_deref(),
        Some(THREAD_A),
        "claude terminal must never adopt a codex candidate"
    );
    assert_eq!(registry_resume_id(&registry, &codex_tid), None);

    // ---- Guard 4: out-of-root rolloutPath is ignored (fails containment,
    //      so its contents are never read) ----
    let outside = std::env::temp_dir().join(format!("outside-rollout-{THREAD_A}.jsonl"));
    std::fs::write(&outside, format!("{THREAD_A}\n")).unwrap();
    send_candidate_expect_silence(&mut ws, &codex_tid, THREAD_A, &outside.to_string_lossy()).await;
    assert_eq!(registry_resume_id(&registry, &codex_tid), None);

    // ---- Guard 4: nonexistent rolloutPath is ignored ----
    let missing = sessions_day.join("rollout-nope.jsonl");
    send_candidate_expect_silence(&mut ws, &codex_tid, THREAD_A, &missing.to_string_lossy()).await;
    assert_eq!(registry_resume_id(&registry, &codex_tid), None);

    // ---- Guard 4: foreign-lineage rollout is ignored (in-root, real file,
    //      session_meta first line -- but payload.id is ANOTHER session's;
    //      the claimed id appears only as fork lineage payload.session_id) ----
    let foreign = sessions_day.join(format!("rollout-2026-07-24T11-00-00-{THREAD_B}.jsonl"));
    std::fs::write(
        &foreign,
        format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{THREAD_B}\",\"session_id\":\"{THREAD_A}\"}}}}\n"),
    )
    .unwrap();
    send_candidate_expect_silence(&mut ws, &codex_tid, THREAD_A, &foreign.to_string_lossy()).await;
    assert_eq!(registry_resume_id(&registry, &codex_tid), None);

    // ---- Happy path: binds both identity homes + broadcasts ----
    // NO ping gate here (see send_candidate_expect_silence's doc): receipt of
    // the two broadcasts IS the consumption proof. Their order is pinned by
    // the handler: associated BEFORE meta.updated.
    send_candidate(&mut ws, &codex_tid, THREAD_A, &rollout_a).await;
    let associated = next_frame_of_type(&mut ws, "terminal.session.associated").await;
    assert_eq!(associated["terminalId"], json!(codex_tid));
    assert_eq!(
        associated["sessionRef"],
        json!({ "provider": "codex", "sessionId": THREAD_A })
    );
    let meta = next_frame_of_type(&mut ws, "terminal.meta.updated").await;
    let upsert = &meta["upsert"][0];
    assert_eq!(upsert["terminalId"], json!(codex_tid));
    assert_eq!(upsert["provider"], json!("codex"));
    assert_eq!(upsert["sessionId"], json!(THREAD_A));
    assert_eq!(
        registry_resume_id(&registry, &codex_tid).as_deref(),
        Some(THREAD_A)
    );

    // ---- Guard 3b: cross-pane hijack -- THREAD_A is live-bound to codex_tid ----
    send_create(&mut ws, "req-codex-cand-2", "codex", json!({})).await;
    let created2 = next_frame_of_type(&mut ws, "terminal.created").await;
    let codex_tid2 = created2["terminalId"]
        .as_str()
        .expect("terminalId")
        .to_string();
    send_candidate_expect_silence(&mut ws, &codex_tid2, THREAD_A, &rollout_a).await;
    assert_eq!(
        registry_resume_id(&registry, &codex_tid2),
        None,
        "a sessionRef bound to a different live terminal must never be adopted"
    );

    // ---- Guard 3a: stale replayed candidate once a newer binding exists ----
    let rollout_b = sessions_day.join(format!("rollout-2026-07-24T13-00-00-{THREAD_B}.jsonl"));
    std::fs::write(
        &rollout_b,
        format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{THREAD_B}\"}}}}\n"),
    )
    .unwrap();
    send_candidate_expect_silence(&mut ws, &codex_tid, THREAD_B, &rollout_b.to_string_lossy())
        .await;
    assert_eq!(
        registry_resume_id(&registry, &codex_tid).as_deref(),
        Some(THREAD_A),
        "an already-bound terminal must keep its binding; replayed/stale candidates are ignored"
    );

    // ---- Guard 3b (retired-INCLUSIVE): dead-pane candidate replay ----
    // Kill THREAD_A's owner over the WS protocol: `handle_kill` retires the
    // identity entry SYNCHRONOUSLY in the dispatch loop
    // (`state.identity.retire(terminal_id)` in terminal.rs's handle_kill), so
    // the ping/pong gate deterministically orders retirement before the
    // replay. (A direct `registry.kill()` would leave retirement to the
    // async pty exit hook -- racy, and the red test must observe a RETIRED
    // binding to distinguish retired-inclusive from live-only guard 3b.)
    ws.send(WsMessage::Text(
        json!({"type": "terminal.kill", "terminalId": codex_tid}).to_string(),
    ))
    .await
    .expect("send terminal.kill");
    ws.send(WsMessage::Text(json!({"type": "ping"}).to_string()))
        .await
        .expect("send ping");
    let _pong = next_frame_of_type(&mut ws, "pong").await;

    send_create(&mut ws, "req-codex-cand-3", "codex", json!({})).await;
    let created3 = next_frame_of_type(&mut ws, "terminal.created").await;
    let codex_tid3 = created3["terminalId"]
        .as_str()
        .expect("terminalId")
        .to_string();
    // Replay the SAME candidate (THREAD_A, its genuine rollout) onto the
    // fresh pane: WARN + ignore, nothing sent back, tid3 stays unbound -- a
    // retired binding still blocks a DIFFERENT terminal's claim (ledger A8).
    send_candidate_expect_silence(&mut ws, &codex_tid3, THREAD_A, &rollout_a).await;
    assert_eq!(
        registry_resume_id(&registry, &codex_tid3),
        None,
        "a DEAD pane's session identity must never be claimable by a fresh terminal"
    );

    // ---- Subsequent restore create builds `codex ... resume <id>` ----
    // (codex_tid was already killed in the dead-pane phase above.)
    // Per-phase capture path (precedent: codex_session_ref_resume.rs's
    // `capture_for(phase)`): the earlier fresh codex spawns (tid2/tid3) hold
    // the ORIGINAL path in their env and may write it late -- a shared path
    // would let a stale fresh-create argv shadow the restore argv.
    let capture_restore = std::env::temp_dir().join(format!(
        "codex-candidate-argv-restore-{}.txt",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&capture_restore);
    std::env::set_var("CODEX_ARGV_CAPTURE_PATH", &capture_restore);
    send_create(
        &mut ws,
        "req-codex-cand-restore",
        "codex",
        json!({ "restore": true, "sessionRef": { "provider": "codex", "sessionId": THREAD_A } }),
    )
    .await;
    let restored = next_frame_of_type(&mut ws, "terminal.created").await;
    let restored_tid = restored["terminalId"]
        .as_str()
        .expect("terminalId")
        .to_string();
    let argv = wait_for_captured_argv(&capture_restore);
    let pos = argv.iter().position(|a| a == "resume");
    assert!(
        pos.is_some_and(|p| argv.get(p + 1).map(String::as_str) == Some(THREAD_A)),
        "restore create must spawn `codex ... resume {THREAD_A}`: {argv:?}"
    );

    registry.kill(&restored_tid);
    registry.kill(&codex_tid2);
    registry.kill(&codex_tid3);
    registry.kill(&claude_tid);
    std::env::remove_var("CODEX_HOME");
    std::env::remove_var("CODEX_ARGV_CAPTURE_PATH");
}
