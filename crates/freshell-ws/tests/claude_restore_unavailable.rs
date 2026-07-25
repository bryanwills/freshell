//! P0.4 integration (campaign plan §2.2): a claude `restore:true` create must
//! resume when an id is resolvable (client-supplied OR server-side lineage)
//! and fail LOUD -- error{RESTORE_UNAVAILABLE}, no pty -- when it is not.
//! Never a silent bare `claude` with neither --session-id nor --resume.

mod common;

use common::{
    connect_and_capture_inventory, next_frame_of_type, session_ref_of, sleeper_cli_spec,
    spawn_server, spawn_server_with_specs,
};
use futures_util::SinkExt;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message as WsMessage;

const KNOWN_ID: &str = "11111111-2222-4333-8444-555566667777";

async fn send_create(ws: &mut common::TestWs, body: serde_json::Value) {
    ws.send(WsMessage::Text(body.to_string()))
        .await
        .expect("send terminal.create");
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

/// A fake claude that exits promptly after start -- a NATURAL pty exit, which
/// RETAINS the registry row (`finish_pty_exit`; an explicit `registry.kill()`
/// would REMOVE the row and destroy the lineage). Mirrors `sleeper_cli_spec`'s
/// CliCommandSpec shape exactly, swapping only the command.
fn fast_exit_claude_spec() -> freshell_platform::CliCommandSpec {
    let script_path = std::env::temp_dir().join(format!(
        "freshell-claude-fastexit-{}.sh",
        std::process::id()
    ));
    std::fs::write(&script_path, "#!/bin/sh\nsleep 0.2\nexit 0\n").expect("write fast-exit script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();
    }
    let mut spec = sleeper_cli_spec("claude");
    spec.default_cmd = script_path.to_string_lossy().to_string();
    spec
}

/// Poll until a terminal's registry row reports Exited -- the natural pty
/// exit has been observed and the row RETAINED (registry.rs `finish_pty_exit`
/// sets status; no freshell-ws test awaits a `terminal.exit` frame for this
/// because that frame goes only to ATTACHED subscribers, and this test never
/// attaches). Bounded-poll shape mirrors Task 2's `wait_for_captured_argv`.
fn wait_for_exited(registry: &freshell_terminal::TerminalRegistry, terminal_id: &str) {
    for _ in 0..100 {
        if let Some(row) = registry.probe(terminal_id) {
            if row.status == freshell_protocol::TerminalRunStatus::Exited {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("terminal {terminal_id} never reached Exited");
}

/// Pins EXISTING behavior: restore:true + sessionRef resumes with that id.
#[tokio::test]
async fn claude_restore_with_session_ref_resumes() {
    let (url, registry) = spawn_server().await;
    let (mut ws, _inv) = connect_and_capture_inventory(&url).await;

    send_create(
        &mut ws,
        json!({
            "type": "terminal.create",
            "requestId": "req-restore-ref-1",
            "mode": "claude",
            "shell": "system",
            "cwd": std::env::temp_dir().to_string_lossy(),
            "restore": true,
            "sessionRef": { "provider": "claude", "sessionId": KNOWN_ID },
        }),
    )
    .await;

    let created = next_frame_of_type(&mut ws, "terminal.created").await;
    let tid = created["terminalId"]
        .as_str()
        .expect("terminalId")
        .to_string();
    assert_eq!(
        session_ref_of(&created),
        Some(json!({ "provider": "claude", "sessionId": KNOWN_ID })),
        "restore-with-ref must carry the client's identity: {created}"
    );
    assert_eq!(
        registry_resume_id(&registry, &tid).as_deref(),
        Some(KNOWN_ID)
    );
    registry.kill(&tid);
}

/// The server's own resolution ladder (this slice: in-process identity
/// registry via createRequestId lineage). A restore:true create with NO
/// client id, whose requestId lineage has a NATURALLY-EXITED generation with
/// a retained identity, resumes it automatically -- no error, no user
/// interaction. Uses a fast-exiting fake claude: only a NATURAL pty exit
/// retains the registry row (`finish_pty_exit`) -- `registry.kill()` REMOVES
/// the row entirely, and with it the lineage. (An explicitly user-killed
/// terminal therefore loses its lineage BY DESIGN: a restore after user-kill
/// fails loud, which is correct under "never silently wrong".)
#[tokio::test(flavor = "multi_thread")]
async fn claude_restore_without_id_resolves_from_request_lineage() {
    let (url, registry) =
        spawn_server_with_specs(vec![sleeper_cli_spec("amplifier"), fast_exit_claude_spec()]).await;
    let (mut ws, _inv) = connect_and_capture_inventory(&url).await;

    // Generation 1: fresh claude (server preallocates a --session-id UUID).
    // Preallocation is CREATE-TIME (before the pty runs), so terminal.created
    // still carries the sessionRef even though the fake exits ~200ms later.
    send_create(
        &mut ws,
        json!({
            "type": "terminal.create",
            "requestId": "req-lineage-1",
            "mode": "claude",
            "shell": "system",
            "cwd": std::env::temp_dir().to_string_lossy(),
        }),
    )
    .await;
    let created = next_frame_of_type(&mut ws, "terminal.created").await;
    let tid1 = created["terminalId"]
        .as_str()
        .expect("terminalId")
        .to_string();
    let preallocated = session_ref_of(&created).expect("fresh claude carries sessionRef")
        ["sessionId"]
        .as_str()
        .expect("sessionId")
        .to_string();

    // Let generation 1 exit NATURALLY, and wait until the registry row shows
    // Exited (row retained; identity entry retired-not-removed by the exit
    // hook). Only then is the lineage rung -- and the A13 not-Running gate --
    // satisfied for generation 2.
    wait_for_exited(&registry, &tid1);

    // Generation 2: same requestId, restore:true, identity LOST client-side.
    send_create(
        &mut ws,
        json!({
            "type": "terminal.create",
            "requestId": "req-lineage-1",
            "mode": "claude",
            "shell": "system",
            "cwd": std::env::temp_dir().to_string_lossy(),
            "restore": true,
        }),
    )
    .await;
    let restored = next_frame_of_type(&mut ws, "terminal.created").await;
    let tid2 = restored["terminalId"]
        .as_str()
        .expect("terminalId")
        .to_string();
    assert_eq!(
        session_ref_of(&restored),
        Some(json!({ "provider": "claude", "sessionId": preallocated })),
        "server must auto-resume the lineage identity: {restored}"
    );
    assert_eq!(
        registry_resume_id(&registry, &tid2).as_deref(),
        Some(preallocated.as_str())
    );
    // No cleanup kill needed: generation 2 is the same fast-exit fake.
}

/// A13 gate (ledger): while the lineage's newest generation is still RUNNING,
/// the ladder must NOT auto-resume -- a second live claude resuming the same
/// session would be silently wrong. (Capability-on clients get live adoption
/// via the pane_reconcile dedupe instead; this harness sends no capabilities,
/// so the create falls through to the ladder and must fail loud.)
#[tokio::test]
async fn claude_restore_while_lineage_still_running_is_rejected() {
    let (url, registry) = spawn_server().await; // sleeper claude: stays Running
    let (mut ws, _inv) = connect_and_capture_inventory(&url).await;

    // Generation 1: fresh claude, still running.
    send_create(
        &mut ws,
        json!({
            "type": "terminal.create",
            "requestId": "req-live-1",
            "mode": "claude",
            "shell": "system",
            "cwd": std::env::temp_dir().to_string_lossy(),
        }),
    )
    .await;
    let created = next_frame_of_type(&mut ws, "terminal.created").await;
    let tid1 = created["terminalId"]
        .as_str()
        .expect("terminalId")
        .to_string();

    // Same requestId, restore:true, no id -- while generation 1 is Running.
    send_create(
        &mut ws,
        json!({
            "type": "terminal.create",
            "requestId": "req-live-1",
            "mode": "claude",
            "shell": "system",
            "cwd": std::env::temp_dir().to_string_lossy(),
            "restore": true,
        }),
    )
    .await;
    let err = next_frame_of_type(&mut ws, "error").await;
    assert_eq!(
        err["code"],
        json!("RESTORE_UNAVAILABLE"),
        "exact wire code: {err}"
    );
    assert_eq!(
        err["requestId"],
        json!("req-live-1"),
        "reject must correlate: {err}"
    );
    let rows = registry.identity_probe_rows();
    assert_eq!(
        rows.len(),
        1,
        "only the original terminal may exist: {rows:?}"
    );
    assert_eq!(rows[0].terminal_id, tid1);
    registry.kill(&tid1);
}

/// Genuinely unresolvable: error frame with the EXACT code + message the
/// frozen client handles (generic in-flight-create error handler,
/// TerminalView.tsx:3995; Node parity ws-handler.ts:2130-2159), and NO pty.
#[tokio::test]
async fn claude_restore_without_any_identity_is_rejected_loudly() {
    let (url, registry) = spawn_server().await;
    let (mut ws, _inv) = connect_and_capture_inventory(&url).await;

    send_create(
        &mut ws,
        json!({
            "type": "terminal.create",
            "requestId": "req-lost-1",
            "mode": "claude",
            "shell": "system",
            "cwd": std::env::temp_dir().to_string_lossy(),
            "restore": true,
        }),
    )
    .await;

    let err = next_frame_of_type(&mut ws, "error").await;
    assert_eq!(
        err["code"],
        json!("RESTORE_UNAVAILABLE"),
        "exact wire code: {err}"
    );
    assert_eq!(
        err["message"],
        json!("Restore requires a canonical session reference."),
        "Node-parity message: {err}"
    );
    assert_eq!(
        err["requestId"],
        json!("req-lost-1"),
        "reject must correlate: {err}"
    );
    assert!(
        registry.identity_probe_rows().is_empty(),
        "NO pty may be spawned for an unresolvable restore"
    );

    // Provider-mismatched sessionRef is equally unresolvable (Node parity).
    send_create(
        &mut ws,
        json!({
            "type": "terminal.create",
            "requestId": "req-lost-2",
            "mode": "claude",
            "shell": "system",
            "cwd": std::env::temp_dir().to_string_lossy(),
            "restore": true,
            "sessionRef": { "provider": "codex", "sessionId": KNOWN_ID },
        }),
    )
    .await;
    let err2 = next_frame_of_type(&mut ws, "error").await;
    assert_eq!(err2["code"], json!("RESTORE_UNAVAILABLE"));
    assert_eq!(err2["requestId"], json!("req-lost-2"));
    assert!(registry.identity_probe_rows().is_empty());

    // Non-canonical claude session id is equally unresolvable (full Node
    // reject-predicate parity, ws-handler.ts:2130-2139: `m.mode === 'claude'
    // && !isValidClaudeSessionId(...)`; canonical shape at
    // shared/session-contract.ts:34,44-46).
    send_create(
        &mut ws,
        json!({
            "type": "terminal.create",
            "requestId": "req-lost-3",
            "mode": "claude",
            "shell": "system",
            "cwd": std::env::temp_dir().to_string_lossy(),
            "restore": true,
            "sessionRef": { "provider": "claude", "sessionId": "not-a-canonical-uuid" },
        }),
    )
    .await;
    let err3 = next_frame_of_type(&mut ws, "error").await;
    assert_eq!(err3["code"], json!("RESTORE_UNAVAILABLE"));
    assert_eq!(err3["requestId"], json!("req-lost-3"));
    assert!(
        registry.identity_probe_rows().is_empty(),
        "NO pty for a non-canonical claude id"
    );
}
