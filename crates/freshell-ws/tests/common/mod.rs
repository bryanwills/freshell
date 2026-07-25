//! Shared integration-test harness for `freshell-ws` WS tests.
//!
//! Extracted verbatim from `attach_viewport_resize.rs` and
//! `session_identity_frames.rs`, whose harness sections were byte-identical
//! copies. Compiled into each test binary that declares `mod common;` —
//! helpers unused by a given binary are expected, hence the file-level
//! `dead_code` allow (the idiomatic pattern for `tests/common/mod.rs`).
#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use freshell_ws::WsState;

pub const AUTH_TOKEN: &str = "s3cr3t-token-abcdef";

/// Process-wide isolated FRESHELL_AMPLIFIER_HOME (the broker's own
/// amplifier-home override — validated F1: the broker never consults the
/// CLI's cache-only AMPLIFIER_HOME). The amplifier pre-create path
/// (launcher-assigned identity plan) writes stub session dirs at terminal
/// create time — without this, any shared-harness test that creates an
/// amplifier terminal would litter the developer's real ~/.amplifier.
/// OnceLock ⇒ a single `set_var` per process with one stable value, safe
/// under parallel tests (mirrors the CODEX_CMD env discipline in
/// `codex_session_ref_resume.rs`). Edition-2021 note: `set_var` is a safe
/// fn today; an edition-2024 bump makes it unsafe — revisit this helper then.
pub fn isolate_amplifier_home() -> &'static std::path::Path {
    use std::sync::OnceLock;
    static HOME: OnceLock<std::path::PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!(
            "freshell-ws-test-amplifier-home-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create isolated FRESHELL_AMPLIFIER_HOME");
        std::env::set_var("FRESHELL_AMPLIFIER_HOME", &dir);
        dir
    })
}

pub fn test_settings_value() -> serde_json::Value {
    serde_json::json!({
        "ai": {},
        "codingCli": { "enabledProviders": [], "mcpServer": true, "providers": {} },
        "editor": { "externalEditor": "auto" },
        "extensions": { "disabled": [] },
        "freshAgent": { "defaultPlugins": [], "enabled": false, "providers": {} },
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

/// A minimal always-present CLI spec (`/bin/sh` sleeper script) so a
/// `mode:"amplifier"` create genuinely spawns — the same recording-script
/// convention as `freshell-freshagent`'s Slice 3a tests, minus the argv file
/// (these tests assert on wire frames, not argv).
pub fn sleeper_cli_spec(name: &str) -> freshell_platform::CliCommandSpec {
    let script_path = std::env::temp_dir().join(format!(
        "freshell-identity-frames-sleeper-{name}-{}.sh",
        std::process::id()
    ));
    std::fs::write(&script_path, "#!/bin/sh\nexec sleep 30\n").expect("write sleeper script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();
    }
    freshell_platform::CliCommandSpec {
        name: name.to_string(),
        label: format!("{name}-label"),
        env_var: None,
        default_cmd: script_path.to_string_lossy().to_string(),
        base_args: vec![],
        base_env: std::collections::BTreeMap::new(),
        // Manifest-true resume args per provider: amplifier is
        // `["resume", "{{sessionId}}"]` (extensions/amplifier/freshell.json),
        // claude-shaped specs keep `["--resume", "{{sessionId}}"]`.
        resume_args: Some(if name == "amplifier" {
            vec!["resume".to_string(), "{{sessionId}}".to_string()]
        } else {
            vec!["--resume".to_string(), "{{sessionId}}".to_string()]
        }),
        // Required for the fresh-claude preallocation path: `LaunchIntent::Start`
        // THROWS without `create_session_args` (`cli_launch.rs:436-441`), same
        // shape as the real claude spec (`cli_launch_goldens.rs:50`).
        create_session_args: Some(vec![
            "--session-id".to_string(),
            "{{sessionId}}".to_string(),
        ]),
        model_args: None,
        sandbox_args: None,
        permission_mode_args: None,
    }
}

/// Real axum server on an ephemeral loopback port, with an `amplifier` CLI
/// spec registered so resume creates spawn a real (sleeper) PTY. Returns the
/// ws URL + the shared registry (for cleanup kills).
pub async fn spawn_server() -> (String, freshell_terminal::TerminalRegistry) {
    spawn_server_with_specs(vec![
        sleeper_cli_spec("amplifier"),
        sleeper_cli_spec("claude"),
    ])
    .await
}

/// Same real-axum server, caller-chosen CLI specs — for spec-sensitive tests
/// (e.g. an `env_var: Some("AMPLIFIER_CMD")` amplifier spec; the shared
/// sleeper specs keep `env_var: None` on purpose so ambient dev-shell env
/// never leaks into unrelated tests). `spawn_server()` delegates here.
pub async fn spawn_server_with_specs(
    specs: Vec<freshell_platform::CliCommandSpec>,
) -> (String, freshell_terminal::TerminalRegistry) {
    isolate_amplifier_home();
    let auth_token = Arc::new(AUTH_TOKEN.to_string());
    let broadcast_tx = Arc::new(tokio::sync::broadcast::channel::<String>(64).0);
    let settings =
        Arc::new(serde_json::from_value(test_settings_value()).expect("valid settings fixture"));
    let registry = freshell_terminal::TerminalRegistry::new();

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
            serde_json::json!({ "freshAgent": { "enabled": false } }),
        ),
        fresh_claude: freshell_freshagent::FreshClaudeState::new(Arc::clone(&broadcast_tx)),
        fresh_opencode: freshell_freshagent::FreshOpencodeState::new(
            freshell_freshagent::FreshAgentState::new(
                Arc::clone(&auth_token),
                Arc::clone(&broadcast_tx),
            ),
        ),
        registry: registry.clone(),
        tabs: freshell_ws::tabs::TabsRegistry::new(),
        screenshots: freshell_ws::screenshot::ScreenshotBroker::new(Arc::clone(&broadcast_tx)),
        terminals_revision: Arc::new(std::sync::atomic::AtomicI64::new(0)),
        sessions_revision: Arc::new(std::sync::atomic::AtomicI64::new(0)),
        cli_commands: Arc::new(specs),
        shutdown: Arc::new(tokio::sync::Notify::new()),
        ping_interval_ms: 30_000,
        hello_timeout_ms: 5_000,
        allowed_origins: Arc::new(freshell_ws::origin::default_allowed_origins()),
        ws_max_payload_bytes: 16 * 1024 * 1024,
        term09: freshell_ws::backpressure::Term09Config::default(),
        config_fallback: None,
        amplifier_locator: None,
        opencode_locator: None,
        activity: None,
        session_existence: std::sync::Arc::new(freshell_ws::existence::NoIndexProbe::default()),
    };

    let router = freshell_ws::router(state);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral loopback port");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    (format!("ws://{addr}/ws", addr = addr), registry)
}

pub type TestWs =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Connect + hello, returning the socket AND the parsed `terminal.inventory`
/// handshake frame (the 4th handshake message; `config_fallback` is None in
/// this harness, so the handshake is exactly 4 frames).
pub async fn connect_and_capture_inventory(url: &str) -> (TestWs, serde_json::Value) {
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

    let mut inventory = serde_json::Value::Null;
    for _ in 0..4u8 {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("handshake message within timeout")
            .expect("stream not ended")
            .expect("no ws error");
        if let WsMessage::Text(text) = &msg {
            let value: serde_json::Value = serde_json::from_str(text).expect("json frame");
            if value["type"] == serde_json::json!("terminal.inventory") {
                inventory = value;
            }
        }
    }
    assert!(
        !inventory.is_null(),
        "handshake must contain terminal.inventory"
    );
    (ws, inventory)
}

pub async fn create_shell_terminal(ws: &mut TestWs, request_id: &str) -> String {
    ws.send(WsMessage::Text(
        serde_json::json!({
            "type": "terminal.create",
            "requestId": request_id,
            "mode": "shell",
            "shell": "system",
        })
        .to_string(),
    ))
    .await
    .expect("send terminal.create");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
            Ok(Some(Ok(WsMessage::Text(text)))) => {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                    continue;
                };
                if value.get("type").and_then(|v| v.as_str()) == Some("terminal.created")
                    && value.get("requestId").and_then(|v| v.as_str()) == Some(request_id)
                {
                    return value
                        .get("terminalId")
                        .and_then(|v| v.as_str())
                        .expect("terminal.created carries terminalId")
                        .to_string();
                }
            }
            Ok(Some(Ok(_))) => {}
            other => panic!("expected terminal.created, got {other:?}"),
        }
    }
    panic!("terminal.created never arrived");
}

/// Concatenate the `data` payload of every `terminal.output`/`terminal.output.batch`
/// frame seen until either `marker` appears in the accumulated text or the
/// deadline elapses. Returns `(accumulated_text, gap_seen, closed)`.
pub async fn drain_until_marker_or_deadline(
    ws: &mut TestWs,
    marker: &str,
    deadline: tokio::time::Instant,
) -> (String, bool, bool) {
    let mut acc = String::new();
    let mut gap_seen = false;
    let mut closed = false;
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining.max(Duration::from_millis(1)), ws.next()).await {
            Ok(Some(Ok(WsMessage::Text(text)))) => {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                    match value.get("type").and_then(|v| v.as_str()) {
                        Some("terminal.output") | Some("terminal.output.batch") => {
                            if let Some(data) = value.get("data").and_then(|v| v.as_str()) {
                                acc.push_str(data);
                            }
                        }
                        Some("terminal.output.gap") => gap_seen = true,
                        _ => {}
                    }
                }
                if acc.contains(marker) {
                    break;
                }
            }
            Ok(Some(Ok(WsMessage::Close(_)))) | Ok(None) => {
                closed = true;
                break;
            }
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(_))) => {
                closed = true;
                break;
            }
            Err(_) => break, // timed out
        }
    }
    (acc, gap_seen, closed)
}

pub async fn attach_with(
    ws: &mut TestWs,
    terminal_id: &str,
    attach_request_id: &str,
    intent: &str,
    cols: u16,
    rows: u16,
    expected_session_ref: Option<serde_json::Value>,
) {
    let mut msg = serde_json::json!({
        "type": "terminal.attach",
        "terminalId": terminal_id,
        "intent": intent,
        "cols": cols,
        "rows": rows,
        "attachRequestId": attach_request_id,
    });
    if let Some(sr) = expected_session_ref {
        msg["expectedSessionRef"] = sr;
    }
    ws.send(WsMessage::Text(msg.to_string()))
        .await
        .expect("send terminal.attach");
}

pub async fn wait_for_attach_ready(ws: &mut TestWs, attach_request_id: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
            Ok(Some(Ok(WsMessage::Text(text)))) => {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                    continue;
                };
                if value.get("type").and_then(|v| v.as_str()) == Some("terminal.attach.ready")
                    && value.get("attachRequestId").and_then(|v| v.as_str())
                        == Some(attach_request_id)
                {
                    return;
                }
            }
            Ok(Some(Ok(_))) => {}
            other => panic!("expected terminal.attach.ready, got {other:?}"),
        }
    }
    panic!("terminal.attach.ready never arrived for {attach_request_id}");
}

pub async fn send_input(ws: &mut TestWs, terminal_id: &str, data: &str) {
    ws.send(WsMessage::Text(
        serde_json::json!({
            "type": "terminal.input",
            "terminalId": terminal_id,
            "data": data,
        })
        .to_string(),
    ))
    .await
    .expect("send terminal.input");
}

/// Read text frames until one with `type == wanted` arrives (bounded).
pub async fn next_frame_of_type(ws: &mut TestWs, wanted: &str) -> serde_json::Value {
    for _ in 0..20u8 {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for a {wanted} frame"))
            .expect("stream not ended")
            .expect("no ws error");
        if let WsMessage::Text(text) = &msg {
            let value: serde_json::Value = serde_json::from_str(text).expect("json frame");
            if value["type"] == serde_json::json!(wanted) {
                return value;
            }
        }
    }
    panic!("no {wanted} frame within 20 messages");
}

/// Non-null `sessionRef` accessor (robust to both omitted-key and explicit
/// null serializations).
pub fn session_ref_of(frame: &serde_json::Value) -> Option<serde_json::Value> {
    match frame.get("sessionRef") {
        Some(v) if !v.is_null() => Some(v.clone()),
        _ => None,
    }
}
