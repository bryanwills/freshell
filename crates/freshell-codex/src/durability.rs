//! codex **durability / thread-id** handling — the id shapes the T2
//! `session.durable-id-shape` invariant grades, the rollout-filename → threadId extraction
//! (`providers/codex.ts:417-421`), and the sidecar ownership identifiers the `/proc` reaper
//! keys on (parity with `freshell-opencode`'s `OPENCODE_SIDECAR_OWNERSHIP_ENV`).
//!
//! Codex thread ids are **UUIDs and STABLE from create** — placeholder == durable, so NO
//! `freshAgent.session.materialized` event fires (`coding-cli.md §1c`; `codex-gptmini.json`
//! shapes `placeholderIdPattern == durableIdPattern`). The on-disk transcript is
//! `rollout-<ts>-<threadId>.jsonl` under `<CODEX_HOME>/sessions/<date-dirs>/`
//! (`codex-gptmini.json` provenance).

use std::path::Path;

use uuid::Uuid;

/// The env var that tags an owned `codex app-server` sidecar so the `/proc` reaper can
/// SIGTERM exactly our detached child and no other (`runtime.ts:494,1258`). The reaper
/// needle is `"{CODEX_SIDECAR_OWNERSHIP_ENV}={ownership_id}"`. Mirror of
/// `freshell-opencode`'s `OPENCODE_SIDECAR_OWNERSHIP_ENV`.
pub const CODEX_SIDECAR_OWNERSHIP_ENV: &str = "FRESHELL_CODEX_SIDECAR_ID";

/// `true` iff `value` is a bare UUID (8-4-4-4-12 hex) — the codex thread-id / durable-id
/// shape (`codex-gptmini.json` `placeholderIdPattern`/`durableIdPattern`). Case-insensitive
/// hex, matching the reference's `[0-9a-fA-F]` classes (`providers/codex.ts:419`).
pub fn is_codex_thread_id(value: &str) -> bool {
    matches_uuid_at(value.as_bytes(), 0) == Some(value.len())
}

/// The `/proc environ` reaper needle for an owned sidecar (`runtime.ts:494`).
pub fn ownership_needle(ownership_id: &str) -> String {
    format!("{CODEX_SIDECAR_OWNERSHIP_ENV}={ownership_id}")
}

/// Mint a fresh sidecar ownership id `codex-sidecar-<uuid>` (`ownershipIdFactory`,
/// `runtime.ts:924`).
pub fn mint_ownership_id() -> String {
    format!("codex-sidecar-{}", Uuid::new_v4())
}

/// The default server-instance id: `FRESHELL_SERVER_INSTANCE_ID` or `srv-<pid>`
/// (`runtime.ts:923`). Stamped into ownership metadata + durability records.
pub fn default_server_instance_id() -> String {
    std::env::var("FRESHELL_SERVER_INSTANCE_ID")
        .unwrap_or_else(|_| format!("srv-{}", std::process::id()))
}

/// `extractSessionIdFromFilename(filePath)` (`providers/codex.ts:417-421`): the UUID embedded
/// in a `rollout-<ts>-<threadId>.jsonl` basename, else the basename (minus `.jsonl`) verbatim.
pub fn extract_session_id_from_filename(file_path: &str) -> String {
    let base = Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file_path);
    let base = base.strip_suffix(".jsonl").unwrap_or(base);
    match find_uuid(base) {
        Some(uuid) => uuid,
        None => base.to_string(),
    }
}

// ── UUID matching (no regex crate; hand-rolled 8-4-4-4-12 hex) ──────────────────────────

fn is_hex(b: u8) -> bool {
    b.is_ascii_digit() || (b'a'..=b'f').contains(&b.to_ascii_lowercase())
}

/// If `bytes[start..]` begins with a UUID (8-4-4-4-12 hex), return the index just past it.
fn matches_uuid_at(bytes: &[u8], start: usize) -> Option<usize> {
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
    let mut i = start;
    for (g, &len) in GROUPS.iter().enumerate() {
        if g > 0 {
            if bytes.get(i) != Some(&b'-') {
                return None;
            }
            i += 1;
        }
        for _ in 0..len {
            match bytes.get(i) {
                Some(&b) if is_hex(b) => i += 1,
                _ => return None,
            }
        }
    }
    Some(i)
}

/// The first UUID-shaped substring of `text`, if any (`String.match(uuidRegex)`).
fn find_uuid(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    for start in 0..bytes.len() {
        if let Some(end) = matches_uuid_at(bytes, start) {
            return Some(text[start..end].to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_thread_id_shape_is_a_bare_uuid() {
        // The exact codex-gptmini.json placeholder/durable pattern.
        assert!(is_codex_thread_id("019810de-1e5f-7db3-9c47-1c2a3b4c5d6e"));
        assert!(is_codex_thread_id("ABCDEF01-2345-6789-abcd-ef0123456789")); // case-insensitive
                                                                             // Rejections: too short, extra chars, non-hex, wrong grouping.
        assert!(!is_codex_thread_id("thread-new-1"));
        assert!(!is_codex_thread_id("freshopencode-abc"));
        assert!(!is_codex_thread_id("019810de-1e5f-7db3-9c47-1c2a3b4c5d6")); // 11 in last group
        assert!(!is_codex_thread_id("019810de-1e5f-7db3-9c47-1c2a3b4c5d6ef")); // 13 in last group
        assert!(!is_codex_thread_id("g19810de-1e5f-7db3-9c47-1c2a3b4c5d6e")); // non-hex
        assert!(!is_codex_thread_id(" 019810de-1e5f-7db3-9c47-1c2a3b4c5d6e")); // leading space
    }

    #[test]
    fn rollout_filename_yields_embedded_thread_uuid() {
        // rollout-<ts>-<threadId>.jsonl → the UUID (codex-gptmini.json transcript layout).
        assert_eq!(
            extract_session_id_from_filename(
                "/codex/sessions/2026/07/05/rollout-2026-07-05T06-25-37-019810de-1e5f-7db3-9c47-1c2a3b4c5d6e.jsonl"
            ),
            "019810de-1e5f-7db3-9c47-1c2a3b4c5d6e"
        );
        // No UUID → the basename verbatim (reference fallback).
        assert_eq!(
            extract_session_id_from_filename("/x/session-activity.jsonl"),
            "session-activity"
        );
        assert_eq!(
            extract_session_id_from_filename("rollout-plain.jsonl"),
            "rollout-plain"
        );
    }

    #[test]
    fn ownership_id_and_needle_shapes() {
        let id = mint_ownership_id();
        assert!(id.starts_with("codex-sidecar-"));
        assert!(
            is_codex_thread_id(id.trim_start_matches("codex-sidecar-")),
            "the tail is a UUID"
        );
        assert_eq!(
            ownership_needle("codex-sidecar-abc"),
            "FRESHELL_CODEX_SIDECAR_ID=codex-sidecar-abc"
        );
    }

    #[test]
    fn server_instance_id_defaults_to_srv_pid_without_env() {
        // No env override → srv-<pid> shape (we cannot mutate global env safely in parallel
        // tests, so only assert the default branch shape when the var is absent).
        if std::env::var("FRESHELL_SERVER_INSTANCE_ID").is_err() {
            let id = default_server_instance_id();
            assert!(id.starts_with("srv-"), "got {id}");
        }
    }
}
