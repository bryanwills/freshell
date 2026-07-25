//! WS-level proof for restart-resilience P0.2 slice 1: the real dispatch
//! (`terminal.rs`'s `ClientMessage::FreshAgentAttach` arm) must route a claude/kilroy
//! `freshAgent.attach` to `FreshClaudeState::handle_attach` instead of swallowing it
//! via `_ => {}`. Unit-level coverage exists in `claude.rs::tests`, but -- exactly like
//! the kill/interrupt dispatch gap before it (`freshagent_claude_kill_interrupt.rs`) --
//! it is unreachable from the wire until the dispatch arm exists. Harness duplicated
//! from that file per the repo's per-test-file convention.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use freshell_ws::WsState;

const AUTH_TOKEN: &str = "s3cr3t-token-abcdef";

// ── server harness (duplicated from diag01_lifecycle_events.rs's convention, with
//    `freshAgent.enabled: true` so `freshAgent.create` actually dispatches) ──

fn test_settings_value() -> serde_json::Value {
    serde_json::json!({
        "ai": {},
        "codingCli": { "enabledProviders": [], "mcpServer": true, "providers": {} },
        "editor": { "externalEditor": "auto" },
        "extensions": { "disabled": [] },
        "freshAgent": { "defaultPlugins": [], "enabled": true, "providers": {} },
        "logging": { "debug": false },
        "network": { "configured": true, "host": "127.0.0.1" },
        "panes": { "defaultNewPane": "ask" },
        "safety": { "autoKillIdleMinutes": 15 },
        "sidebar": {
            "autoGenerateTitles": true,
            "excludeFirstChatMustStart": false,
            "excludeFirstChatSubstrings": []
        },
        "terminal": { "scrollback": 10000 }
    })
}

async fn spawn_server() -> String {
    let auth_token = Arc::new(AUTH_TOKEN.to_string());
    let broadcast_tx = Arc::new(tokio::sync::broadcast::channel::<String>(64).0);
    let settings =
        Arc::new(serde_json::from_value(test_settings_value()).expect("valid settings fixture"));

    let state = WsState {
        identity: freshell_ws::identity::TerminalIdentityRegistry::new(),
        auth_token: Arc::clone(&auth_token),
        server_instance_id: Arc::new("srv-test".to_string()),
        boot_id: Arc::new("boot-test".to_string()),
        settings,
        broadcast_tx: Arc::clone(&broadcast_tx),
        fresh_codex: freshell_freshagent::FreshCodexState::new(
            Arc::clone(&auth_token),
            Arc::clone(&broadcast_tx),
            serde_json::json!({ "freshAgent": { "enabled": true } }),
        ),
        fresh_claude: freshell_freshagent::FreshClaudeState::new(Arc::clone(&broadcast_tx)),
        fresh_opencode: freshell_freshagent::FreshOpencodeState::new(
            freshell_freshagent::FreshAgentState::new(
                Arc::clone(&auth_token),
                Arc::clone(&broadcast_tx),
            ),
        ),
        registry: freshell_terminal::TerminalRegistry::new(),
        tabs: freshell_ws::tabs::TabsRegistry::new(),
        screenshots: freshell_ws::screenshot::ScreenshotBroker::new(Arc::clone(&broadcast_tx)),
        terminals_revision: Arc::new(std::sync::atomic::AtomicI64::new(0)),
        sessions_revision: Arc::new(std::sync::atomic::AtomicI64::new(0)),
        cli_commands: Arc::new(Vec::new()),
        shutdown: Arc::new(tokio::sync::Notify::new()),
        ping_interval_ms: 30_000,
        hello_timeout_ms: 5_000,
        allowed_origins: Arc::new(freshell_ws::origin::default_allowed_origins()),
        ws_max_payload_bytes: 16 * 1024 * 1024,
        term09: freshell_ws::backpressure::Term09Config::default(),
        config_fallback: None,
        opencode_locator: None,
        activity: None,
        session_existence: std::sync::Arc::new(freshell_ws::existence::NoIndexProbe::default()),
    };

    let router = freshell_ws::router(state);
    // Ephemeral loopback port only -- NEVER the self-hosted 3001/3002 ports.
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral loopback port");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    format!("ws://{addr}/ws")
}

type TestWs =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect_and_complete_handshake(url: &str) -> TestWs {
    let (mut ws, _resp) = tokio_tungstenite::connect_async(url)
        .await
        .expect("ws connect");
    ws.send(WsMessage::Text(
        serde_json::json!({
            "type": "hello",
            "token": AUTH_TOKEN,
            "protocolVersion": freshell_protocol::WS_PROTOCOL_VERSION,
        })
        .to_string(),
    ))
    .await
    .expect("send hello");

    // Drain the handshake frames (ready + whatever else precedes it) until `ready`.
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("handshake message within timeout")
            .expect("stream not ended")
            .expect("no ws error");
        let WsMessage::Text(text) = msg else {
            continue;
        };
        let value: Value = serde_json::from_str(&text).unwrap();
        if value["type"] == "ready" {
            break;
        }
    }
    ws
}

async fn send_json(ws: &mut TestWs, value: &Value) {
    ws.send(WsMessage::Text(value.to_string()))
        .await
        .expect("send frame");
}

/// Drain frames until one matching `predicate` arrives (or the budget expires).
async fn await_frame(
    ws: &mut TestWs,
    budget: Duration,
    predicate: impl Fn(&Value) -> bool,
) -> Value {
    tokio::time::timeout(budget, async {
        loop {
            let msg = ws
                .next()
                .await
                .expect("stream not ended")
                .expect("no ws error");
            let WsMessage::Text(text) = msg else {
                continue;
            };
            let value: Value = serde_json::from_str(&text).unwrap();
            if predicate(&value) {
                return value;
            }
        }
    })
    .await
    .expect("expected frame did not arrive within budget")
}

/// A claude `freshAgent.attach` for a session id this server process does not track
/// (the always-true case right after a server restart) must produce the
/// `freshAgent.error{code:'INVALID_SESSION_ID'}` lost-session frame on the wire --
/// the frame the frozen client folds into `markSessionLost` -> `triggerRecovery`.
/// Before the fix the dispatch swallowed the message and NO frame ever arrived
/// (this test then fails with `await_frame` panicking on its timeout budget).
#[tokio::test]
async fn claude_attach_for_untracked_session_emits_lost_session_frame_over_ws() {
    let url = spawn_server().await;
    let mut ws = connect_and_complete_handshake(&url).await;

    send_json(
        &mut ws,
        &serde_json::json!({
            "type": "freshAgent.attach",
            "provider": "claude",
            "sessionId": "restarted-away",
            "sessionType": "freshclaude",
        }),
    )
    .await;

    let frame = await_frame(&mut ws, Duration::from_secs(10), |v| {
        v["type"] == "freshAgent.event" && v["sessionId"] == "restarted-away"
    })
    .await;

    assert_eq!(frame["provider"], "claude");
    assert_eq!(frame["sessionType"], "freshclaude");
    assert_eq!(frame["event"]["type"], "freshAgent.error");
    assert_eq!(frame["event"]["code"], "INVALID_SESSION_ID");
}

/// Kilroy panes ride the same claude provider arm with `sessionType: "kilroy"`; the
/// envelope must echo it (through the real serde parse of `ClientMessage`, which the
/// unit tests bypass) or the client builds the wrong session locator.
#[tokio::test]
async fn kilroy_attach_for_untracked_session_emits_lost_session_frame_over_ws() {
    let url = spawn_server().await;
    let mut ws = connect_and_complete_handshake(&url).await;

    send_json(
        &mut ws,
        &serde_json::json!({
            "type": "freshAgent.attach",
            "provider": "claude",
            "sessionId": "kilroy-was-here",
            "sessionType": "kilroy",
        }),
    )
    .await;

    let frame = await_frame(&mut ws, Duration::from_secs(10), |v| {
        v["type"] == "freshAgent.event" && v["sessionId"] == "kilroy-was-here"
    })
    .await;

    assert_eq!(frame["provider"], "claude");
    assert_eq!(frame["sessionType"], "kilroy");
    assert_eq!(frame["event"]["code"], "INVALID_SESSION_ID");
}
