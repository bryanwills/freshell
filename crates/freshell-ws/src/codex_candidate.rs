//! P0.3 (campaign plan §2.3.1): server-side capture of a codex terminal's
//! session identity from the client's `terminal.codex.candidate.persisted`
//! frame -- guarded so identity never becomes client-writable.
//!
//! Four guards; a candidate failing ANY check is logged at WARN and ignored
//! (never adopted, and nothing is sent back -- legacy parity with
//! `server/ws-handler.ts:2951-2963`):
//!   1. the terminalId exists in the registry
//!   2. that terminal is codex-mode
//!   3. the terminal is not already bound to a DIFFERENT thread id (stale
//!      replay) and the claimed thread id is not already bound to a DIFFERENT
//!      terminal -- live OR retired (cross-pane hijack, including replaying a
//!      DEAD pane's candidate onto a fresh pane)
//!   4. disk truth: the rolloutPath canonicalizes under the codex sessions
//!      root and its FIRST JSONL record is a `session_meta` whose
//!      `payload.id` is the claimed thread id (bounded 1MB read; legacy
//!      parity `server/coding-cli/codex-app-server/durability-proof.ts:88-102`)

use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};

use freshell_protocol::{
    ServerMessage, SessionLocator, TerminalCodexCandidatePersisted, TerminalMetaRecord,
    TerminalMetaUpdated, TerminalSessionAssociated,
};

use crate::terminal::now_ms;
use crate::WsState;

/// Codex thread ids are bare hyphenated UUIDs. Cheap shape check so an empty
/// or junk id can never substring-match everything in the disk guard.
pub(crate) fn is_uuid_shaped(s: &str) -> bool {
    s.len() == 36
        && s.chars().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// Upper bound on the first-line read below. Legacy parity:
/// `MAX_FIRST_RECORD_BYTES` in `durability-proof.ts`. Real first lines
/// observed <= 22.4KB (V5 sampling); 1MB is generous headroom while capping
/// what a 152MB adversarial rollout can cost the sync dispatch loop.
const MAX_FIRST_LINE_BYTES: u64 = 1024 * 1024;

/// Guard 4 (disk truth). Client-supplied paths are NEVER trusted raw:
/// `fs::canonicalize` both sides (stats the file and resolves `..` and
/// symlinks, so traversal and symlink escapes fail containment), require the
/// rollout to live under the sessions root, then prove OWNERSHIP with a
/// bounded read of only the FIRST line, parsed as JSON: it must be a
/// `session_meta` record whose `payload.id` equals the claimed thread id
/// (legacy parity: `durability-proof.ts:88-102`).
///
/// `payload.id` EXACTLY -- NOT `payload.session_id`, which is fork/resume
/// LINEAGE and matches a FOREIGN session in 54/144 real rollouts (V5); and
/// never a substring match on filename or contents, which the same lineage
/// data makes spoofable (40% of sampled rollouts contain foreign uuids).
pub(crate) fn verify_rollout_path(
    rollout_path: &str,
    sessions_root: &Path,
    thread_id: &str,
) -> Result<(), &'static str> {
    let root = std::fs::canonicalize(sessions_root).map_err(|_| "sessions_root_missing")?;
    let rollout = std::fs::canonicalize(rollout_path).map_err(|_| "rollout_missing")?;
    if !rollout.starts_with(&root) {
        return Err("rollout_outside_sessions_root");
    }
    if !rollout.is_file() {
        return Err("rollout_not_a_file");
    }
    // Bounded first-line read: never `read_to_string` a client-named file
    // (real rollouts reach 152MB, p99=28MB -- an uncapped read in the sync
    // dispatch loop is an adversarial hazard).
    let file = std::fs::File::open(&rollout).map_err(|_| "rollout_unreadable")?;
    let mut first_line: Vec<u8> = Vec::new();
    let mut limited = std::io::BufReader::new(file).take(MAX_FIRST_LINE_BYTES);
    limited
        .read_until(b'\n', &mut first_line)
        .map_err(|_| "rollout_unreadable")?;
    if first_line.len() as u64 >= MAX_FIRST_LINE_BYTES && !first_line.ends_with(b"\n") {
        return Err("rollout_first_line_too_large");
    }
    let record: serde_json::Value =
        serde_json::from_slice(&first_line).map_err(|_| "rollout_first_record_not_json")?;
    if record.get("type").and_then(|v| v.as_str()) != Some("session_meta") {
        return Err("rollout_first_record_not_session_meta");
    }
    match record.pointer("/payload/id").and_then(|v| v.as_str()) {
        Some(id) if id == thread_id => Ok(()),
        _ => Err("thread_id_mismatch"),
    }
}

/// `CODEX_HOME` env (non-empty) else `<HOME>/.codex`, then `/sessions` --
/// mirrors `freshell-server/src/session_directory.rs::codex_home` (which is
/// crate-private there; HOME only, never FRESHELL_HOME) joined with the
/// `sessions` dir the way `freshell_sessions::directory_index::CodexSource`
/// does.
fn codex_sessions_root() -> Option<PathBuf> {
    let home = match std::env::var("CODEX_HOME") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => {
            #[cfg(windows)]
            let base = std::env::var("USERPROFILE").ok()?;
            #[cfg(not(windows))]
            let base = std::env::var("HOME").ok()?;
            PathBuf::from(base).join(".codex")
        }
    };
    Some(home.join("sessions"))
}

/// Handle one `terminal.codex.candidate.persisted` frame. No reply frame on
/// any path; rejects are WARN logs, accepts bind BOTH identity homes and
/// broadcast (mirrors `opencode_association.rs`'s resolve path).
pub(crate) fn handle_codex_candidate_persisted(
    state: &WsState,
    msg: TerminalCodexCandidatePersisted,
) {
    let thread_id = msg.candidate_thread_id.as_str();
    if !is_uuid_shaped(thread_id) {
        tracing::warn!(
            terminal_id = %msg.terminal_id,
            candidate_thread_id = %thread_id,
            "codex_candidate_rejected: invalid_thread_id"
        );
        return;
    }
    // Guard 1: the terminal exists.
    let Some(row) = state.registry.probe(&msg.terminal_id) else {
        tracing::warn!(
            terminal_id = %msg.terminal_id,
            candidate_thread_id = %thread_id,
            "codex_candidate_rejected: terminal_missing"
        );
        return;
    };
    // Guard 2: the terminal is codex-mode.
    if row.mode != "codex" {
        tracing::warn!(
            terminal_id = %msg.terminal_id,
            mode = %row.mode,
            "codex_candidate_rejected: terminal_not_codex"
        );
        return;
    }
    // Guard 3a: the terminal is not already bound to a DIFFERENT session
    // (stale-replay defense; a re-announce of the SAME id is an idempotent
    // no-op -- the client re-sends on every durability update). ORDER
    // MATTERS: this same-terminal check must run BEFORE guard 3b's
    // cross-terminal check, so a legit re-announce short-circuits here and
    // never reaches the retired-inclusive lookup.
    if let Some(existing) = row.resume_session_id.as_deref().filter(|s| !s.is_empty()) {
        if existing != thread_id {
            tracing::warn!(
                terminal_id = %msg.terminal_id,
                candidate_thread_id = %thread_id,
                bound_session_id = %existing,
                "codex_candidate_rejected: terminal_already_bound"
            );
        }
        return;
    }
    // Guard 3b: the claimed session is not bound to a DIFFERENT terminal --
    // live OR retired (cross-pane hijack). Retired-INCLUSIVE deliberately
    // (ledger A8): a victim's binding is retired at exit, so a live-only
    // lookup would let a DEAD pane's candidate be replayed onto a fresh
    // terminal. Blocks no legitimate flow: every legit resume binds at
    // create time and short-circuits at guard 3a above.
    if let Some(other_tid) = state
        .identity
        .find_by_session_including_retired("codex", thread_id)
    {
        if other_tid != msg.terminal_id {
            tracing::warn!(
                terminal_id = %msg.terminal_id,
                candidate_thread_id = %thread_id,
                bound_terminal_id = %other_tid,
                "codex_candidate_rejected: session_bound_elsewhere"
            );
            return;
        }
    }
    // Guard 4: disk truth.
    let Some(root) = codex_sessions_root() else {
        tracing::warn!(
            terminal_id = %msg.terminal_id,
            "codex_candidate_rejected: no_codex_sessions_root"
        );
        return;
    };
    if let Err(reason) = verify_rollout_path(&msg.rollout_path, &root, thread_id) {
        tracing::warn!(
            terminal_id = %msg.terminal_id,
            candidate_thread_id = %thread_id,
            rollout_path = %msg.rollout_path,
            "codex_candidate_rejected: {reason}"
        );
        return;
    }
    // Bind both identity homes (they have different consumers -- see
    // opencode_association.rs:135-148), then broadcast.
    state.identity.upsert(
        &msg.terminal_id,
        Some("codex"),
        Some(thread_id),
        row.cwd.as_deref(),
        now_ms(),
    );
    state.registry.set_meta(
        &msg.terminal_id,
        None,
        None,
        Some("codex".to_string()),
        Some(thread_id.to_string()),
    );
    broadcast_terminal_session_associated(state, &msg.terminal_id, thread_id, row.cwd.clone());
}

/// Fan `terminal.session.associated` + a `terminal.meta.updated` upsert to
/// every connection. Byte-for-byte the shape of
/// `opencode_association.rs::broadcast_terminal_session_associated` with
/// provider "codex". EMISSION ORDER IS PINNED: `associated` FIRST, then
/// `meta.updated` (mirroring opencode_association.rs:163-198) -- the
/// integration test awaits them in exactly this order, and
/// `next_frame_of_type` drops out-of-order frames. Do not reorder.
fn broadcast_terminal_session_associated(
    state: &WsState,
    terminal_id: &str,
    session_id: &str,
    cwd: Option<String>,
) {
    let associated = ServerMessage::TerminalSessionAssociated(TerminalSessionAssociated {
        terminal_id: terminal_id.to_string(),
        session_ref: SessionLocator {
            provider: "codex".to_string(),
            session_id: session_id.to_string(),
        },
    });
    if let Ok(frame) = serde_json::to_string(&associated) {
        let _ = state.broadcast_tx.send(frame);
    }

    let meta = ServerMessage::TerminalMetaUpdated(TerminalMetaUpdated {
        remove: Vec::new(),
        upsert: vec![TerminalMetaRecord {
            terminal_id: terminal_id.to_string(),
            updated_at: now_ms(),
            branch: None,
            checkout_root: None,
            cwd,
            display_subdir: None,
            is_dirty: None,
            provider: Some("codex".to_string()),
            repo_root: None,
            session_id: Some(session_id.to_string()),
            token_usage: None,
        }],
    });
    if let Ok(frame) = serde_json::to_string(&meta) {
        let _ = state.broadcast_tx.send(frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TID: &str = "0192aaaa-bbbb-cccc-dddd-eeeeffff0001";
    /// A DIFFERENT session's uuid -- plays the fork/resume-lineage foreign id.
    const OTHER: &str = "0192aaaa-bbbb-cccc-dddd-eeeeffff0099";

    /// The honest first line a real rollout starts with (durability-proof.ts
    /// contract): a `session_meta` record whose `payload.id` is the file's
    /// OWN session id.
    fn session_meta_line(id: &str) -> String {
        format!("{{\"timestamp\":\"2026-07-24T12:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\"}}}}\n")
    }

    fn root_with_rollout(
        file_name: &str,
        contents: &str,
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let sessions = dir
            .path()
            .join("sessions")
            .join("2026")
            .join("07")
            .join("24");
        std::fs::create_dir_all(&sessions).expect("mkdir sessions tree");
        let rollout = sessions.join(file_name);
        std::fs::write(&rollout, contents).expect("write rollout");
        (dir, rollout)
    }

    #[test]
    fn uuid_shape_accepts_canonical_and_rejects_junk() {
        assert!(is_uuid_shaped(TID));
        assert!(!is_uuid_shaped(""));
        assert!(!is_uuid_shaped("not-a-uuid"));
        assert!(!is_uuid_shaped("0192aaaa-bbbb-cccc-dddd-eeeeffff000")); // 35 chars
        assert!(!is_uuid_shaped("0192aaaa+bbbb+cccc+dddd+eeeeffff0001")); // wrong separators
    }

    #[test]
    fn accepts_rollout_whose_first_record_is_own_session_meta() {
        // Later lines (even ones mentioning FOREIGN ids) are irrelevant: only
        // the first record is consulted.
        let contents = format!(
            "{}{{\"type\":\"response_item\",\"session_id\":\"{OTHER}\"}}\n",
            session_meta_line(TID)
        );
        let (dir, rollout) = root_with_rollout(
            &format!("rollout-2026-07-24T12-00-00-{TID}.jsonl"),
            &contents,
        );
        let root = dir.path().join("sessions");
        assert_eq!(
            verify_rollout_path(rollout.to_str().unwrap(), &root, TID),
            Ok(())
        );
    }

    #[test]
    fn rejects_foreign_lineage_rollout() {
        // The real-world fork-lineage spoof (V5: 54/144 real rollouts): the
        // first line IS a session_meta and DOES carry the claimed id -- but
        // only as `payload.session_id` (fork/resume lineage). `payload.id`,
        // the file's OWN id, belongs to a different session. Must reject.
        let contents = format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{OTHER}\",\"session_id\":\"{TID}\"}}}}\n"
        );
        let (dir, rollout) = root_with_rollout(
            &format!("rollout-2026-07-24T12-00-00-{OTHER}.jsonl"),
            &contents,
        );
        let root = dir.path().join("sessions");
        assert_eq!(
            verify_rollout_path(rollout.to_str().unwrap(), &root, TID),
            Err("thread_id_mismatch")
        );
    }

    #[test]
    fn rejects_non_session_meta_first_record() {
        // Even a record carrying the claimed id is not proof unless it is the
        // session_meta header record.
        let contents = format!("{{\"type\":\"response_item\",\"payload\":{{\"id\":\"{TID}\"}}}}\n");
        let (dir, rollout) = root_with_rollout(
            &format!("rollout-2026-07-24T12-00-00-{TID}.jsonl"),
            &contents,
        );
        let root = dir.path().join("sessions");
        assert_eq!(
            verify_rollout_path(rollout.to_str().unwrap(), &root, TID),
            Err("rollout_first_record_not_session_meta")
        );
    }

    #[test]
    fn rejects_malformed_first_line() {
        let (dir, rollout) = root_with_rollout(
            &format!("rollout-2026-07-24T12-00-00-{TID}.jsonl"),
            "not json\n",
        );
        let root = dir.path().join("sessions");
        assert_eq!(
            verify_rollout_path(rollout.to_str().unwrap(), &root, TID),
            Err("rollout_first_record_not_json")
        );
    }

    #[test]
    fn rejects_oversized_first_line() {
        // A >1MB first line is rejected by the cap BEFORE parsing -- even if
        // the JSON would have been a valid own-session session_meta.
        let contents = format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{TID}\",\"pad\":\"{}\"}}}}\n",
            "a".repeat(2 * 1024 * 1024)
        );
        let (dir, rollout) = root_with_rollout(
            &format!("rollout-2026-07-24T12-00-00-{TID}.jsonl"),
            &contents,
        );
        let root = dir.path().join("sessions");
        assert_eq!(
            verify_rollout_path(rollout.to_str().unwrap(), &root, TID),
            Err("rollout_first_line_too_large")
        );
    }

    #[test]
    fn rejects_nonexistent_rollout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("sessions");
        std::fs::create_dir_all(&root).unwrap();
        let missing = root.join(format!("rollout-{TID}.jsonl"));
        assert_eq!(
            verify_rollout_path(missing.to_str().unwrap(), &root, TID),
            Err("rollout_missing")
        );
    }

    #[test]
    fn rejects_rollout_outside_sessions_root() {
        let (dir, _rollout) = root_with_rollout(&format!("rollout-{TID}.jsonl"), "{}");
        let root = dir.path().join("sessions");
        // A real file that exists but lives OUTSIDE the root.
        let outside = dir.path().join(format!("rollout-{TID}.jsonl"));
        std::fs::write(&outside, "{}").unwrap();
        assert_eq!(
            verify_rollout_path(outside.to_str().unwrap(), &root, TID),
            Err("rollout_outside_sessions_root")
        );
    }

    #[test]
    fn rejects_dotdot_traversal_escape() {
        let (dir, _rollout) = root_with_rollout(&format!("rollout-{TID}.jsonl"), "{}");
        let root = dir.path().join("sessions");
        let outside = dir.path().join(format!("escape-{TID}.jsonl"));
        std::fs::write(&outside, "{}").unwrap();
        // Path is SPELLED under the root but traverses out; canonicalize resolves it.
        let sneaky = root.join("..").join(format!("escape-{TID}.jsonl"));
        assert_eq!(
            verify_rollout_path(sneaky.to_str().unwrap(), &root, TID),
            Err("rollout_outside_sessions_root")
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        let (dir, _rollout) = root_with_rollout(&format!("rollout-{TID}.jsonl"), "{}");
        let root = dir.path().join("sessions");
        let outside = dir.path().join(format!("target-{TID}.jsonl"));
        std::fs::write(&outside, "{}").unwrap();
        let link = root.join(format!("rollout-link-{TID}.jsonl"));
        std::os::unix::fs::symlink(&outside, &link).expect("symlink");
        assert_eq!(
            verify_rollout_path(link.to_str().unwrap(), &root, TID),
            Err("rollout_outside_sessions_root")
        );
    }
}
