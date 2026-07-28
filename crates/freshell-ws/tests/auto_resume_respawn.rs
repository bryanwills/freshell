//! respawn_agent_terminal spawns a resume-generation with the same
//! createRequestId and provider-native resume argv.
//!
//! Raw-WS integration against an in-process axum server on an ephemeral
//! loopback port (shared `common` harness convention). The claude CLI command
//! is overridden with a recording shim so argv is assertable — the same
//! plain-`sh` recording-script convention as `codex_session_ref_resume.rs`,
//! with the capture path baked into the script (no env-var mutation, so this
//! stays parallel-safe).

mod common;

use std::time::Duration;

use common::{connect_and_capture_inventory, next_frame_of_type};
use futures_util::SinkExt;
use tokio_tungstenite::tungstenite::Message as WsMessage;

/// A claude-shaped CLI spec whose command records its argv (one token per
/// line, atomically via tmp+mv) to `capture`, then exits 1 — so the first
/// generation "crashes" immediately and a respawn's argv overwrites the
/// capture. The crash-and-record sibling of `auto_resume_events.rs`'s
/// `exiting_cli_spec`.
fn recording_crashing_claude_spec(capture: &std::path::Path) -> freshell_platform::CliCommandSpec {
    let script_path = std::env::temp_dir().join(format!(
        "freshell-auto-resume-respawn-shim-{}.sh",
        std::process::id()
    ));
    let capture = capture.display();
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{capture}.tmp.$$\"\nmv \"{capture}.tmp.$$\" \"{capture}\"\nexit 1\n"
    );
    std::fs::write(&script_path, script).expect("write recording shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();
    }
    freshell_platform::CliCommandSpec {
        name: "claude".to_string(),
        label: "claude-label".to_string(),
        env_var: None,
        default_cmd: script_path.to_string_lossy().to_string(),
        base_args: vec![],
        base_env: std::collections::BTreeMap::new(),
        resume_args: Some(vec!["--resume".to_string(), "{{sessionId}}".to_string()]),
        // The fresh-claude preallocation path THROWS without
        // `create_session_args` (`cli_launch.rs:436-441`); same shape as
        // `common::sleeper_cli_spec`.
        create_session_args: Some(vec![
            "--session-id".to_string(),
            "{{sessionId}}".to_string(),
        ]),
        model_args: None,
        sandbox_args: None,
        permission_mode_args: None,
    }
}

/// Poll the capture file until its argv contains `--resume <session_id>` (the
/// respawn overwrites the crashed generation's `--session-id ...` capture) or
/// the deadline passes; returns the captured argv tokens for the assertion.
fn wait_for_resume_argv(path: &std::path::Path, session_id: &str) -> Vec<String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(raw) = std::fs::read_to_string(path) {
            let argv: Vec<String> = raw.lines().map(str::to_string).collect();
            if argv
                .windows(2)
                .any(|w| w[0] == "--resume" && w[1] == session_id)
            {
                return argv;
            }
            if std::time::Instant::now() >= deadline {
                return argv; // let the caller's assertion print what WAS captured
            }
        } else {
            assert!(
                std::time::Instant::now() < deadline,
                "spawned child never wrote its argv capture at {}",
                path.display()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[tokio::test(flavor = "multi_thread")]
#[cfg(unix)]
async fn respawn_spawns_resume_generation_with_same_create_request_id() {
    let capture = std::env::temp_dir().join(format!(
        "freshell-auto-resume-respawn-argv-{}.txt",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&capture);
    let (url, registry, state) =
        common::spawn_server_with_specs_and_state(vec![recording_crashing_claude_spec(&capture)])
            .await;
    let (mut ws, _inv) = connect_and_capture_inventory(&url).await;

    // Arrange: a fresh claude terminal (server-preallocated session id, so
    // `terminal.created` carries the sessionRef) that has crashed — the
    // recording shim exits 1 immediately.
    let create_request_id = "req-respawn-1";
    let create = serde_json::json!({
        "type": "terminal.create",
        "requestId": create_request_id,
        "mode": "claude",
        "shell": "system",
        "cwd": std::env::temp_dir().to_string_lossy(),
    });
    ws.send(WsMessage::Text(create.to_string()))
        .await
        .expect("send terminal.create");
    let created = next_frame_of_type(&mut ws, "terminal.created").await;
    let old_tid = created["terminalId"]
        .as_str()
        .expect("terminal.created carries terminalId")
        .to_string();
    let session_id = created["sessionRef"]["sessionId"]
        .as_str()
        .expect("fresh claude create carries a preallocated sessionRef")
        .to_string();

    // Wait for the crash: the registry row leaves Running (natural exit keeps
    // the row, status Exited) and the exit hook has retired the identity.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let status = registry
            .probe(&old_tid)
            .expect("crashed row remains")
            .status;
        if status != freshell_protocol::TerminalRunStatus::Running {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "crashed generation never exited"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let new_tid = freshell_ws::terminal::respawn_agent_terminal(
        &state,
        &freshell_ws::terminal::AgentRespawnRequest {
            mode: "claude".into(),
            provider: "claude".into(),
            session_id: session_id.clone(),
            create_request_id: create_request_id.into(),
            cwd: None,
        },
    )
    .await
    .expect("respawn");

    assert_ne!(new_tid, old_tid, "a respawn mints a new terminalId");
    // Registry row: same createRequestId, mode claude, resume id recorded.
    let probe = registry.probe(&new_tid).expect("row");
    assert_eq!(probe.mode, "claude");
    assert_eq!(
        probe.resume_session_id.as_deref(),
        Some(session_id.as_str())
    );
    assert_eq!(
        registry.probe_create_request_id(&new_tid),
        Some(create_request_id.to_string())
    );
    // Argv: the fake CLI recorded `--resume <session_id>`.
    let argv = wait_for_resume_argv(&capture, &session_id);
    assert!(
        argv.windows(2)
            .any(|w| w[0] == "--resume" && w[1] == session_id),
        "resume argv missing: {argv:?}"
    );
}

/// Kata enn3 interaction pin: an auto-resume respawn is a SERVER-initiated
/// create, and a crash-loop storm is exactly the shape the server-wide spawn
/// gate exists to bound — so `respawn_agent_terminal` must queue behind the
/// SAME gate as the WS-restore and REST doors and fail loud (no spawn) when
/// the gate's queue is full.
#[tokio::test(flavor = "multi_thread")]
#[cfg(unix)]
async fn respawn_is_rejected_loud_when_spawn_gate_queue_is_full() {
    let (_url, _registry, state) =
        common::spawn_server_with_specs_and_state(vec![common::sleeper_cli_spec("claude")]).await;

    // Saturate the gate: hold every concurrency permit (the shared test
    // harness builds `SpawnGate::new(4, 64)`)...
    let (_hold_tx, mut hold_rx) = tokio::sync::watch::channel(false);
    let mut held_permits = Vec::new();
    for _ in 0..4 {
        held_permits.push(
            state
                .spawn_gate
                .acquire(Duration::from_secs(30), &mut hold_rx)
                .await
                .expect("free permit while unsaturated"),
        );
    }
    // ...then fill the 64-deep queue with waiters (each owns a never-fired
    // cancel sender for its lifetime, the REST-door convention).
    let mut waiters = Vec::new();
    for _ in 0..64 {
        let gate = std::sync::Arc::clone(&state.spawn_gate);
        waiters.push(tokio::spawn(async move {
            let (_cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
            let _ = gate.acquire(Duration::from_secs(30), &mut cancel_rx).await;
        }));
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while state.spawn_gate.queued_total() < 64 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "gate queue never filled: queued_total={}",
            state.spawn_gate.queued_total()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let err = freshell_ws::terminal::respawn_agent_terminal(
        &state,
        &freshell_ws::terminal::AgentRespawnRequest {
            mode: "claude".into(),
            provider: "claude".into(),
            session_id: "sess-gate-pin".into(),
            create_request_id: "req-gate-pin".into(),
            cwd: None,
        },
    )
    .await
    .expect_err("a queue-full spawn gate must reject the respawn, not spawn a PTY");

    match err {
        freshell_ws::terminal::RespawnError::LaunchUnresolvable(msg) => {
            // The queue-full mapping from `spawn_gate_error_parts` — proof the
            // rejection came from the gate, not some other pre-spawn failure.
            assert_eq!(msg, "Too many terminal.create requests");
        }
        other => panic!("expected the gate's queue-full rejection, got {other:?}"),
    }
    assert_eq!(
        state.spawn_gate.queue_rejections(),
        1,
        "exactly the respawn's acquire was rejected by the gate"
    );

    // Unblock the queued waiters so the test exits promptly.
    drop(held_permits);
    for w in waiters {
        let _ = w.await;
    }
}
