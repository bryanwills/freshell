//! Auto-resume orchestrator end-to-end (Task 5): real registry, real PTYs,
//! the hub spawned by the harness with a tiny injected backoff schedule.
//!
//! Raw-WS integration against an in-process axum server on an ephemeral
//! loopback port (shared `common` harness convention). The claude CLI command
//! is a plain-`sh` shim (the `auto_resume_respawn.rs` convention): one
//! variant crashes every generation (retry-exhaustion path), one crashes only
//! its FIRST generation (the reconcile-after-replacement pin).

mod common;

use std::time::Duration;

use common::next_frame_of_type;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message as WsMessage;

/// A claude-shaped CLI spec whose command appends one line to `count_file`
/// per invocation (O_APPEND — atomic for single lines), then exits 1: every
/// generation crashes, so the hub burns its full retry budget.
fn counting_crashing_claude_spec(
    count_file: &std::path::Path,
) -> freshell_platform::CliCommandSpec {
    let script_path = std::env::temp_dir().join(format!(
        "freshell-auto-resume-e2e-counting-shim-{}.sh",
        std::process::id()
    ));
    let script = format!(
        "#!/bin/sh\necho x >> \"{count}\"\nexit 1\n",
        count = count_file.display()
    );
    write_executable(&script_path, &script);
    claude_spec(&script_path)
}

/// A claude-shaped CLI spec that crashes ONLY its first invocation (marker
/// file absent), then survives (`exec sleep 30`) — the replacement generation
/// stays live for the reconcile pin.
fn crash_once_claude_spec(marker: &std::path::Path) -> freshell_platform::CliCommandSpec {
    let script_path = std::env::temp_dir().join(format!(
        "freshell-auto-resume-e2e-crash-once-shim-{}.sh",
        std::process::id()
    ));
    let script = format!(
        "#!/bin/sh\nif [ -e \"{marker}\" ]; then exec sleep 30; fi\n: > \"{marker}\"\nexit 1\n",
        marker = marker.display()
    );
    write_executable(&script_path, &script);
    claude_spec(&script_path)
}

fn write_executable(path: &std::path::Path, script: &str) {
    std::fs::write(path, script).expect("write shim script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
    }
}

fn claude_spec(script_path: &std::path::Path) -> freshell_platform::CliCommandSpec {
    freshell_platform::CliCommandSpec {
        name: "claude".to_string(),
        label: "claude-label".to_string(),
        env_var: None,
        default_cmd: script_path.to_string_lossy().to_string(),
        base_args: vec![],
        base_env: std::collections::BTreeMap::new(),
        resume_args: Some(vec!["--resume".to_string(), "{{sessionId}}".to_string()]),
        // The fresh-claude preallocation path THROWS without
        // `create_session_args` (`cli_launch.rs:436-441`).
        create_session_args: Some(vec![
            "--session-id".to_string(),
            "{{sessionId}}".to_string(),
        ]),
        model_args: None,
        sandbox_args: None,
        permission_mode_args: None,
    }
}

/// Send a fresh claude `terminal.create` and return
/// (old_terminal_id, session_id) from `terminal.created`.
async fn create_claude_terminal(ws: &mut common::TestWs, request_id: &str) -> (String, String) {
    let create = serde_json::json!({
        "type": "terminal.create",
        "requestId": request_id,
        "mode": "claude",
        "shell": "system",
        "cwd": std::env::temp_dir().to_string_lossy(),
    });
    ws.send(WsMessage::Text(create.to_string()))
        .await
        .expect("send terminal.create");
    let created = next_frame_of_type(ws, "terminal.created").await;
    let old_tid = created["terminalId"]
        .as_str()
        .expect("terminal.created carries terminalId")
        .to_string();
    let session_id = created["sessionRef"]["sessionId"]
        .as_str()
        .expect("fresh claude create carries a preallocated sessionRef")
        .to_string();
    (old_tid, session_id)
}

/// Read frames until `pred` matches one (returns it) or the deadline passes.
async fn wait_frame_matching(
    ws: &mut common::TestWs,
    what: &str,
    deadline: tokio::time::Instant,
    mut pred: impl FnMut(&serde_json::Value) -> bool,
) -> serde_json::Value {
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining.max(Duration::from_millis(1)), ws.next()).await {
            Ok(Some(Ok(WsMessage::Text(text)))) => {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                    if pred(&value) {
                        return value;
                    }
                }
            }
            Ok(Some(Ok(_))) => {}
            other => panic!("stream ended while waiting for {what}: {other:?}"),
        }
    }
    panic!("{what} never arrived before the deadline");
}

fn spawn_count(count_file: &std::path::Path) -> usize {
    std::fs::read_to_string(count_file)
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

/// (a) 3 spawns (1 + 2 retries), (b) `terminal.status{recovering, attempt:1}`
/// and `terminal.replaced{attempt:1}` observed on a subscribed ws client,
/// (c) the newest terminal for the createRequestId settles `exited` and no
/// further spawns occur for 500ms.
#[tokio::test(flavor = "multi_thread")]
#[cfg(unix)]
async fn crashing_agent_is_resumed_twice_then_settles_exited() {
    let count_file = std::env::temp_dir().join(format!(
        "freshell-auto-resume-e2e-count-{}.txt",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&count_file);
    let (url, registry) = common::spawn_server_with_specs_and_auto_resume_hub(
        vec![counting_crashing_claude_spec(&count_file)],
        vec![50, 100],
    )
    .await;
    let (mut ws, _inv) = common::connect_and_capture_inventory(&url).await;

    let create_request_id = "req-e2e-crashy";
    let (old_tid, _session_id) = create_claude_terminal(&mut ws, create_request_id).await;

    // (b) The broadcast recovery frames, in order: recovering attempt 1 for
    // the crashed terminal, then replaced attempt 1 naming its successor.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let recovering = wait_frame_matching(&mut ws, "terminal.status{recovering}", deadline, |v| {
        v["type"] == "terminal.status" && v["status"] == "recovering"
    })
    .await;
    assert_eq!(recovering["terminalId"], serde_json::json!(old_tid));
    assert_eq!(recovering["attempt"], serde_json::json!(1));
    // Council 7w4h/xkhx: the client renders from these typed FIELDS — the
    // reason prose below is purely presentational.
    assert_eq!(recovering["maxAttempts"], serde_json::json!(2));
    assert_eq!(recovering["exitCode"], serde_json::json!(1));
    let reason = recovering["reason"].as_str().expect("reason string");
    assert!(
        reason.contains("claude crashed") && reason.contains("attempt 1/2"),
        "unexpected reason: {reason}"
    );
    let replaced = wait_frame_matching(&mut ws, "terminal.replaced", deadline, |v| {
        v["type"] == "terminal.replaced"
    })
    .await;
    assert_eq!(replaced["oldTerminalId"], serde_json::json!(old_tid));
    assert_eq!(replaced["attempt"], serde_json::json!(1));
    assert_eq!(replaced["maxAttempts"], serde_json::json!(2));
    let first_replacement = replaced["newTerminalId"]
        .as_str()
        .expect("newTerminalId")
        .to_string();
    assert_ne!(first_replacement, old_tid);

    // (a) 3 spawns total: the original + 2 retries.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while spawn_count(&count_file) < 3 {
        assert!(
            std::time::Instant::now() < deadline,
            "expected 3 spawns, saw {} before the deadline",
            spawn_count(&count_file)
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(spawn_count(&count_file), 3);

    // (c) The newest generation for the createRequestId settles exited...
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let newest = registry
            .newest_by_create_request_id(create_request_id)
            .expect("a generation exists for the createRequestId");
        let status = registry.probe(&newest).expect("newest row remains").status;
        if status == freshell_protocol::TerminalRunStatus::Exited {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "newest generation never settled exited"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    // ...and the budget is spent: no further spawns for 500ms.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        spawn_count(&count_file),
        3,
        "settled terminal must not respawn again"
    );
}

/// MANDATORY reconcile-after-replacement pin (D-2): after the hub replaces a
/// crashed generation, a SECOND ws client reconciling the OLD terminalId (+
/// the pane's sessionRef + createRequestId) receives an attach verdict naming
/// the NEW live terminal, with `corrected` absent (same-session replacement
/// never overrides the claim, reconcile.rs `corrected_flag`).
#[tokio::test(flavor = "multi_thread")]
#[cfg(unix)]
async fn reconcile_after_replacement_attaches_to_the_new_terminal() {
    let marker = std::env::temp_dir().join(format!(
        "freshell-auto-resume-e2e-crash-once-marker-{}.txt",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&marker);
    let (url, registry) = common::spawn_server_with_specs_and_auto_resume_hub(
        vec![crash_once_claude_spec(&marker)],
        vec![50, 100],
    )
    .await;
    let (mut ws, _inv) = common::connect_and_capture_inventory(&url).await;

    let create_request_id = "req-e2e-crash-once";
    let (old_tid, session_id) = create_claude_terminal(&mut ws, create_request_id).await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let replaced = wait_frame_matching(&mut ws, "terminal.replaced", deadline, |v| {
        v["type"] == "terminal.replaced"
    })
    .await;
    assert_eq!(replaced["oldTerminalId"], serde_json::json!(old_tid));
    let new_tid = replaced["newTerminalId"]
        .as_str()
        .expect("newTerminalId")
        .to_string();

    // A SECOND client (paneReconcileV1 negotiated) presents the pane's
    // pre-crash view: the OLD terminalId + sessionRef + createRequestId.
    let (mut ws2, _resp) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("ws2 connect");
    ws2.send(WsMessage::Text(
        serde_json::json!({
            "type": "hello",
            "token": common::AUTH_TOKEN,
            "protocolVersion": freshell_protocol::WS_PROTOCOL_VERSION,
            "capabilities": { "paneReconcileV1": true },
        })
        .to_string(),
    ))
    .await
    .expect("send hello");
    // Drain the 4-frame handshake (ready → settings.updated → perf.logging →
    // terminal.inventory) before issuing the reconcile.
    let _ = next_frame_of_type(&mut ws2, "terminal.inventory").await;
    ws2.send(WsMessage::Text(
        serde_json::json!({
            "type": "pane.reconcile.request",
            "reconcileId": "rec-after-replacement",
            "panes": [{
                "paneKey": "pk-replaced",
                "kind": "terminal",
                "mode": "claude",
                "terminalId": old_tid,
                "createRequestId": create_request_id,
                "sessionRef": { "provider": "claude", "sessionId": session_id },
            }],
        })
        .to_string(),
    ))
    .await
    .expect("send pane.reconcile.request");

    let result = next_frame_of_type(&mut ws2, "pane.reconcile.result").await;
    let verdicts = result["verdicts"].as_array().expect("verdicts array");
    assert_eq!(verdicts.len(), 1);
    assert_eq!(verdicts[0]["verdict"], "attach");
    assert_eq!(
        verdicts[0]["terminalId"],
        serde_json::json!(new_tid),
        "reconcile must point the pane at the REPLACEMENT terminal"
    );
    assert!(
        verdicts[0].get("corrected").is_none_or(|v| v.is_null()),
        "same-session replacement must not set corrected: {:?}",
        verdicts[0]
    );

    // Cleanup: reap the surviving replacement PTY.
    registry.kill(&new_tid);
}
