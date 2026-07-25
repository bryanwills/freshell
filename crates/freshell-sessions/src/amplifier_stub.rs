//! Launcher-assigned amplifier session identity: pre-create ("stub") session
//! dirs on disk so the broker can spawn `amplifier resume <id>` with an
//! identity it minted itself — no post-spawn correlation.
//!
//! Unlike [`crate::amplifier`] (read-only indexing; "never mutates provider
//! data"), this module deliberately WRITES into the amplifier home. The
//! on-disk layout and the cwd→slug algorithm are EXTERNAL contracts owned by
//! the amplifier CLI (amplifier_app_cli `project_utils.py:22-30`); they are
//! pinned by `test/integration/real/amplifier-stub-adoption-contract.test.ts`
//! and re-checked at broker start by [`verify_amplifier_layout_contract`].

use std::path::{Path, PathBuf};

/// amplifier's cwd→project-slug algorithm (amplifier_app_cli
/// `project_utils.py:22-30`), byte-exact:
/// `str(Path.cwd().resolve()).replace("/", "-").replace("\\", "-").replace(":", "")`,
/// then prefix `-` unless it already starts with one. Dots/underscores
/// preserved. Input must already be RESOLVED — callers use [`canonical_cwd`],
/// mirroring Python's `Path.cwd().resolve()` (symlinks resolved).
/// A slug mismatch fails SILENTLY in production (our stub dir and
/// amplifier's own dir diverge), which is why the exact-match contract test
/// (`amplifier-stub-adoption-contract.test.ts`) and the boot canary exist.
pub fn cwd_slug(resolved_cwd: &str) -> String {
    let slug = resolved_cwd
        .replace('/', "-")
        .replace('\\', "-")
        .replace(':', "");
    if slug.starts_with('-') {
        slug
    } else {
        format!("-{slug}")
    }
}

/// `Path.cwd().resolve()` equivalent for the slug contract: canonicalize,
/// falling back to the raw path when canonicalization fails (dir vanished
/// between validation and spawn — the spawn itself surfaces that error).
pub fn canonical_cwd(cwd: &str) -> PathBuf {
    std::fs::canonicalize(cwd).unwrap_or_else(|_| PathBuf::from(cwd))
}

/// The amplifier home ROOT (the dir containing `projects/`):
/// `$FRESHELL_AMPLIFIER_HOME` (freshell-specific test/dev override, used
/// as-is) if set and non-empty, else `$HOME/.amplifier` (real `HOME` only —
/// deliberately NOT `FRESHELL_HOME`). `None` when neither resolves (callers
/// surface a create error).
///
/// VALIDATED divergence — do NOT "fix" this to read `AMPLIFIER_HOME`: the
/// real CLI hardcodes `Path.home()/.amplifier` for session storage
/// (`session_store.py:96-98`) and honors `AMPLIFIER_HOME` ONLY for
/// bundle/module caches + `registry.json`. A user setting `AMPLIFIER_HOME`
/// moves caches, NOT sessions — consulting it here would place stubs where
/// the CLI never looks (silent identity divergence).
///
/// ONE broker-side resolution: [`crate::amplifier::amplifier_home`] (session
/// index + activity events-path resolver) is retargeted in this same task to
/// the identical `FRESHELL_AMPLIFIER_HOME`-else-`<home>/.amplifier` rule, so
/// the resolver that attaches the events lane at create time always looks in
/// the SAME home this module writes stubs into (pinned by the env test above).
pub fn resolve_amplifier_home() -> Option<PathBuf> {
    match std::env::var("FRESHELL_AMPLIFIER_HOME") {
        Ok(v) if !v.is_empty() => Some(PathBuf::from(v)),
        _ => std::env::var("HOME")
            .ok()
            .filter(|v| !v.is_empty())
            .map(|h| PathBuf::from(h).join(".amplifier")),
    }
}

/// The outcome of [`ensure_session`]: where the session dir is, whether
/// THIS call created it (`created` gates the exit-hook GC — the broker only
/// ever deletes litter it wrote itself), and — for FOUND sessions — slug
/// provenance (validated fix F4/V6): whether the dir lives under a project
/// slug DIFFERENT from slug(canonical cwd), plus that session's own
/// metadata `working_dir`. On a divergent find the caller MUST override the
/// spawn cwd with `working_dir_of_existing` (if it exists and is a dir) or
/// reject the create — `amplifier resume` only searches the spawn cwd's
/// slug, so spawning at the requested cwd would silently find nothing.
#[derive(Debug, Clone)]
pub struct EnsuredSession {
    pub session_dir: PathBuf,
    pub created: bool,
    pub found_under_divergent_slug: bool,
    pub working_dir_of_existing: Option<String>,
}

/// Make `amplifier resume <session_id>` guaranteed-resumable from `cwd`
/// BEFORE spawn. If the session dir already exists under ANY project slug
/// (a real session, or a stub from a previous run), it is found and left
/// untouched — with slug provenance reported (see [`EnsuredSession`]) so
/// the caller can spawn at the session's own `working_dir` when the found
/// slug differs from slug(cwd). Otherwise a stub is written under the slug
/// of the CANONICAL cwd (HARD INVARIANT: amplifier only searches the
/// current cwd's slug — the caller must spawn the PTY with this same cwd).
///
/// Stub shape (validated against the real CLI; see the Tier-1 contract
/// test): `metadata.json` with `session_id`, `created` (ISO-8601 UTC),
/// `working_dir` (canonical cwd), custom `freshell_terminal_id` (survives
/// amplifier's saves — durable linkage bonus; Freshell's own registry stays
/// primary), NO `bundle`; plus empty `transcript.jsonl` and empty
/// `events.jsonl` (the latter so the activity hub's create-time resolver
/// attach finds a file — see the module design note).
pub fn ensure_session(
    amplifier_home: &Path,
    session_id: &str,
    cwd: &str,
    terminal_id: &str,
) -> std::io::Result<EnsuredSession> {
    // Path-safety gate (defense in depth): the id is joined into
    // filesystem paths (`projects/<slug>/sessions/<session_id>`) that this
    // function creates and writes, and that the exit-hook GC later
    // `remove_dir_all`s. Reject anything that is not a plain single path
    // segment BEFORE touching disk — an id containing `/`, `\`, or a bare
    // `.`/`..` would escape the amplifier home (client-supplied via WS
    // sessionRef, the REST body, or poisoned persisted state). Enforcing
    // it HERE covers every caller and the GC's delete path (the GC only
    // ever deletes dirs this function returned with `created: true`).
    // Legal-but-implausible segments (e.g. whitespace ids) still pass:
    // they produce at worst a harmless stub dir inside the home, and the
    // pane_content plausibility gate already keeps them out of sessionRef.
    if session_id.is_empty()
        || session_id == "."
        || session_id == ".."
        || session_id.contains(['/', '\\', '\0'])
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("amplifier session id {session_id:?} is not a valid single path segment"),
        ));
    }
    let resolved = canonical_cwd(cwd);
    let expected_slug = cwd_slug(&resolved.to_string_lossy());
    let projects = amplifier_home.join("projects");
    if let Ok(entries) = std::fs::read_dir(&projects) {
        for entry in entries.flatten() {
            let candidate = entry.path().join("sessions").join(session_id);
            if candidate.is_dir() {
                let found_slug = entry.file_name().to_string_lossy().to_string();
                let divergent = found_slug != expected_slug;
                // On a divergent find, surface the session's own recorded
                // working_dir so the caller can spawn there (F4).
                let working_dir_of_existing = if divergent {
                    std::fs::read_to_string(candidate.join("metadata.json"))
                        .ok()
                        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
                        .and_then(|meta| {
                            meta.get("working_dir")
                                .and_then(|v| v.as_str())
                                .map(str::to_string)
                        })
                } else {
                    None
                };
                return Ok(EnsuredSession {
                    session_dir: candidate,
                    created: false,
                    found_under_divergent_slug: divergent,
                    working_dir_of_existing,
                });
            }
        }
    }

    let dir = projects
        .join(expected_slug)
        .join("sessions")
        .join(session_id);
    std::fs::create_dir_all(&dir)?;
    let metadata = serde_json::json!({
        "session_id": session_id,
        "created": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "working_dir": resolved.to_string_lossy(),
        "freshell_terminal_id": terminal_id,
    });
    std::fs::write(
        dir.join("metadata.json"),
        serde_json::to_string_pretty(&metadata)?,
    )?;
    std::fs::write(dir.join("transcript.jsonl"), "")?;
    std::fs::write(dir.join("events.jsonl"), "")?;
    Ok(EnsuredSession {
        session_dir: dir,
        created: true,
        // Fresh stubs carry no divergence provenance (asserted by the
        // fresh-stub test above).
        found_under_divergent_slug: false,
        working_dir_of_existing: None,
    })
}

/// The verified-unambiguous "never used" signature (validated fix F3/V4):
/// `metadata.json` lacks `turn_count` AND `transcript.jsonl` is empty or
/// absent AND `events.jsonl` (if present) contains NO `prompt:submit`
/// event. A lifecycle-only `events.jsonl` of any size is tolerated
/// (zero-turn resumes leave metadata byte-identical but may write a small
/// events file). The `prompt:submit` clause is a data-loss guard: the CLI
/// handles only SIGINT, a PTY close is SIGHUP, and a kill mid-FIRST-turn
/// persists nothing to metadata/transcript — but the user's typed prompt is
/// already in events.jsonl; deleting the dir would destroy it. (Saves are
/// otherwise per-turn synchronous + atomic tmp+rename, so no transient
/// mid-write windows exist and synchronous exit-hook GC is safe with this
/// predicate.) A dir without parseable metadata is NOT recognizably a stub
/// — never touched. Conservative on I/O errors: any error other than
/// NotFound on transcript.jsonl or events.jsonl means we cannot PROVE the
/// never-used signature — keep.
pub fn stub_is_unused(session_dir: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(session_dir.join("metadata.json")) else {
        return false;
    };
    let Ok(meta) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return false;
    };
    if meta.get("turn_count").is_some() {
        return false;
    }
    match std::fs::metadata(session_dir.join("transcript.jsonl")) {
        Ok(m) if m.len() > 0 => return false,
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        // Cannot prove the transcript is empty — keep.
        Err(_) => return false,
    }
    // Substring scan over raw BYTES is deliberate: the event line shape is
    // the CLI's own (hooks-logging module), and any `"prompt:submit"` hit —
    // parseable or not — must veto deletion. Bytes (not read_to_string)
    // because the exact kill-mid-first-turn scenario this guard exists for
    // can truncate events.jsonl mid multi-byte codepoint, making it invalid
    // UTF-8; a decode failure must not skip the veto.
    const PROMPT_SUBMIT: &[u8] = b"\"prompt:submit\"";
    match std::fs::read(session_dir.join("events.jsonl")) {
        Ok(events) => {
            if events
                .windows(PROMPT_SUBMIT.len())
                .any(|w| w == PROMPT_SUBMIT)
            {
                return false;
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        // Cannot prove the absence of a prompt:submit trace — keep.
        Err(_) => return false,
    }
    true
}

/// Delete a broker-created stub iff it is still unused ("own our litter" —
/// without this, every never-typed-in terminal becomes a permanent '0 msgs'
/// row in the user's `amplifier session list`). Returns whether the dir was
/// removed. Best-effort: IO errors just leave the dir in place.
pub fn gc_stub_if_unused(session_dir: &Path) -> bool {
    if !stub_is_unused(session_dir) {
        return false;
    }
    std::fs::remove_dir_all(session_dir).is_ok()
}

/// Outcome of the boot-time layout canary ([`verify_amplifier_layout_contract`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanaryOutcome {
    Pass {
        sessions_checked: usize,
    },
    /// No amplifier home / no sessions with a `working_dir` — nothing to
    /// verify (amplifier unused or brand new). Not an error.
    NothingToCheck,
    Broken {
        detail: String,
    },
}

/// Cheap, re-runnable self-test of the on-disk contract this whole feature
/// rests on (undocumented upstream; microsoft/amplifier#315/#316 track a
/// `--session-id` flag that would collapse this layer into a flag): for a
/// bounded sample of sessions AMPLIFIER ITSELF wrote, verify the project dir
/// name equals [`cwd_slug`] of the session's own `working_dir`. A mismatch
/// means amplifier changed its slug/layout and our pre-created stubs would
/// silently diverge — callers log ERROR loudly but MUST NOT block broker
/// start.
///
/// VALIDATED skip classes (F6/V5 full-corpus census: 5216/5216 parseable
/// sessions match, incl. all 2700 subagent sessions; 0 mismatches) — these
/// are real shapes in real data, NOT violations, and must be skipped rather
/// than reported Broken: (a) session dirs with no/unparseable
/// `metadata.json` (2.4% of the corpus — events.jsonl-only sessions) or no
/// `working_dir`; (b) `projects/` entries with no `sessions/` subdir (a
/// literal `{project}` template dir exists in real data). The `continue`s
/// below implement exactly these skips.
pub fn verify_amplifier_layout_contract(amplifier_home: &Path) -> CanaryOutcome {
    const MAX_SESSIONS: usize = 20;
    let projects = amplifier_home.join("projects");
    let Ok(project_dirs) = std::fs::read_dir(&projects) else {
        return CanaryOutcome::NothingToCheck;
    };
    let mut checked = 0usize;
    for project in project_dirs.flatten() {
        let Some(project_name) = project.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Ok(sessions) = std::fs::read_dir(project.path().join("sessions")) else {
            continue;
        };
        for session in sessions.flatten() {
            if checked >= MAX_SESSIONS {
                return CanaryOutcome::Pass {
                    sessions_checked: checked,
                };
            }
            let Ok(raw) = std::fs::read_to_string(session.path().join("metadata.json")) else {
                continue;
            };
            let Ok(meta) = serde_json::from_str::<serde_json::Value>(&raw) else {
                continue;
            };
            let Some(working_dir) = meta.get("working_dir").and_then(|v| v.as_str()) else {
                continue;
            };
            // `working_dir` was written RESOLVED by amplifier — slug it
            // directly (no canonicalize: the dir may no longer exist).
            let expected = cwd_slug(working_dir);
            if expected != project_name {
                return CanaryOutcome::Broken {
                    detail: format!(
                        "session {} has working_dir {working_dir} → expected project slug {expected}, but lives under {project_name}",
                        session.path().display()
                    ),
                };
            }
            checked += 1;
        }
    }
    if checked == 0 {
        CanaryOutcome::NothingToCheck
    } else {
        CanaryOutcome::Pass {
            sessions_checked: checked,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_home(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "freshell-amp-stub-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ensure_session_writes_the_designed_stub_shape() {
        let home = unique_temp_home("ensure-fresh");
        let cwd = unique_temp_home("ensure-fresh-cwd");
        let ensured = ensure_session(
            &home,
            "11111111-2222-3333-4444-555555555555",
            cwd.to_str().unwrap(),
            "term-1",
        )
        .unwrap();
        assert!(ensured.created);
        // Fresh stubs carry no divergence provenance.
        assert!(!ensured.found_under_divergent_slug);
        assert!(ensured.working_dir_of_existing.is_none());

        let canonical = std::fs::canonicalize(&cwd).unwrap();
        let expected_dir = home
            .join("projects")
            .join(cwd_slug(canonical.to_str().unwrap()))
            .join("sessions")
            .join("11111111-2222-3333-4444-555555555555");
        assert_eq!(ensured.session_dir, expected_dir);

        let meta: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(expected_dir.join("metadata.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(meta["session_id"], "11111111-2222-3333-4444-555555555555");
        assert_eq!(meta["working_dir"], canonical.to_str().unwrap());
        assert_eq!(meta["freshell_terminal_id"], "term-1");
        // ISO-8601 with tz — must parse through the crate's own parser.
        assert!(crate::time::parse_timestamp_ms(&meta["created"]).is_some());
        // Omit `bundle` so the user's default bundle resolves.
        assert!(meta.get("bundle").is_none());
        // No turn_count on a fresh stub (the GC "unused" signature).
        assert!(meta.get("turn_count").is_none());

        // Empty transcript + empty events (see the module design note).
        assert_eq!(
            std::fs::metadata(expected_dir.join("transcript.jsonl"))
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            std::fs::metadata(expected_dir.join("events.jsonl"))
                .unwrap()
                .len(),
            0
        );

        std::fs::remove_dir_all(&home).ok();
        std::fs::remove_dir_all(&cwd).ok();
    }

    #[test]
    fn ensure_session_finds_an_existing_dir_under_any_slug_and_does_not_touch_it() {
        let home = unique_temp_home("ensure-existing");
        let cwd = unique_temp_home("ensure-existing-cwd");
        // Pre-existing session under a DIFFERENT project slug than cwd's.
        let existing = home
            .join("projects")
            .join("-some-other-project")
            .join("sessions")
            .join("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        std::fs::create_dir_all(&existing).unwrap();
        std::fs::write(existing.join("metadata.json"), r#"{"session_id":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","working_dir":"/x","created":"2026-03-01T00:00:00.000Z","turn_count":3}"#).unwrap();

        let ensured = ensure_session(
            &home,
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            cwd.to_str().unwrap(),
            "term-2",
        )
        .unwrap();
        assert!(
            !ensured.created,
            "existing sessions are found, never re-stubbed"
        );
        assert_eq!(ensured.session_dir, existing);
        // Provenance (validated fix F4): found under a slug DIFFERENT from
        // slug(cwd) — the caller must spawn at the session's own
        // working_dir (or reject), never at the requested cwd.
        assert!(ensured.found_under_divergent_slug);
        assert_eq!(ensured.working_dir_of_existing.as_deref(), Some("/x"));
        // Untouched: still has turn_count, no freshell_terminal_id injected.
        let meta: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(existing.join("metadata.json")).unwrap())
                .unwrap();
        assert_eq!(meta["turn_count"], 3);
        assert!(meta.get("freshell_terminal_id").is_none());

        std::fs::remove_dir_all(&home).ok();
        std::fs::remove_dir_all(&cwd).ok();
    }

    #[test]
    fn ensure_session_rejects_ids_that_are_not_a_single_path_segment() {
        // Security gate: the id is client-controlled (WS sessionRef / REST
        // body / persisted snapshots) and is joined into paths that get
        // created, written, and later GC-deleted — separators or dot-dot
        // must never reach the filesystem.
        let home = unique_temp_home("ensure-badid");
        let cwd = unique_temp_home("ensure-badid-cwd");
        for bad in ["", ".", "..", "../../../etc/passwd", "a/b", "a\\b", "x\0y"] {
            let err = ensure_session(&home, bad, cwd.to_str().unwrap(), "term-3")
                .expect_err("non-single-segment ids must be rejected");
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "id {bad:?}");
        }
        // Rejected BEFORE touching disk — not even projects/ appears.
        assert!(!home.join("projects").exists());
        std::fs::remove_dir_all(&home).ok();
        std::fs::remove_dir_all(&cwd).ok();
    }

    fn write_gc_fixture(
        home: &PathBuf,
        id: &str,
        metadata: &str,
        transcript: Option<&str>,
    ) -> PathBuf {
        let dir = home.join("projects").join("-p").join("sessions").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("metadata.json"), metadata).unwrap();
        if let Some(t) = transcript {
            std::fs::write(dir.join("transcript.jsonl"), t).unwrap();
        }
        dir
    }

    #[test]
    fn stub_is_unused_recognizes_only_the_never_used_signature() {
        let home = unique_temp_home("gc");
        let meta_unused =
            r#"{"session_id":"s","working_dir":"/x","created":"2026-03-01T00:00:00.000Z"}"#;
        let meta_used = r#"{"session_id":"s","working_dir":"/x","created":"2026-03-01T00:00:00.000Z","turn_count":1}"#;

        // Never used: no turn_count + empty transcript.
        assert!(stub_is_unused(&write_gc_fixture(
            &home,
            "a",
            meta_unused,
            Some("")
        )));
        // Never used: no turn_count + transcript ABSENT.
        assert!(stub_is_unused(&write_gc_fixture(
            &home,
            "b",
            meta_unused,
            None
        )));
        // Used: turn_count present.
        assert!(!stub_is_unused(&write_gc_fixture(
            &home,
            "c",
            meta_used,
            Some("")
        )));
        // Used: non-empty transcript (even without turn_count).
        assert!(!stub_is_unused(&write_gc_fixture(
            &home,
            "d",
            meta_unused,
            Some("{\"role\":\"user\"}\n")
        )));
        // A zero-turn resume may create a small events.jsonl of session
        // LIFECYCLE events — tolerated (still unused).
        let e = write_gc_fixture(&home, "e", meta_unused, Some(""));
        std::fs::write(e.join("events.jsonl"), "{\"event\":\"session:start\"}\n").unwrap();
        assert!(stub_is_unused(&e));
        // VALIDATED data-loss guard (F3/V4): an events.jsonl holding a
        // `prompt:submit` event means the user TYPED a prompt — a SIGHUP
        // mid-first-turn persists nothing to metadata/transcript, so this is
        // the ONLY trace of their content. NOT unused, even with empty
        // transcript and no turn_count.
        let g = write_gc_fixture(&home, "g", meta_unused, Some(""));
        std::fs::write(
            g.join("events.jsonl"),
            "{\"event\":\"session:start\"}\n{\"event\":\"prompt:submit\",\"data\":{\"prompt\":\"hi there\"}}\n",
        )
        .unwrap();
        assert!(!stub_is_unused(&g));
        // Conservative byte-scan (fix round 1): a SIGHUP kill can truncate
        // events.jsonl mid multi-byte codepoint, making it invalid UTF-8 —
        // the `prompt:submit` veto must still fire on raw bytes.
        let h = write_gc_fixture(&home, "h", meta_unused, Some(""));
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend_from_slice(b"{\"event\":\"prompt:submit\"}\n");
        bytes.push(0xFF); // trailing truncated codepoint
        std::fs::write(h.join("events.jsonl"), bytes).unwrap();
        assert!(!stub_is_unused(&h));
        // Unparseable (present but invalid JSON) metadata.json: NOT
        // recognizably a stub — never delete.
        let i = write_gc_fixture(&home, "i", "{not json", Some(""));
        assert!(!stub_is_unused(&i));
        // Missing metadata.json: NOT recognizably a stub — never delete.
        let f = home.join("projects").join("-p").join("sessions").join("f");
        std::fs::create_dir_all(&f).unwrap();
        assert!(!stub_is_unused(&f));

        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn gc_stub_if_unused_deletes_only_unused_dirs() {
        let home = unique_temp_home("gc-rm");
        let meta_unused =
            r#"{"session_id":"s","working_dir":"/x","created":"2026-03-01T00:00:00.000Z"}"#;
        let meta_used = r#"{"session_id":"s","working_dir":"/x","created":"2026-03-01T00:00:00.000Z","turn_count":2}"#;
        let unused = write_gc_fixture(&home, "u", meta_unused, Some(""));
        let used = write_gc_fixture(&home, "v", meta_used, Some(""));

        assert!(gc_stub_if_unused(&unused));
        assert!(!unused.exists());
        assert!(!gc_stub_if_unused(&used));
        assert!(used.exists());

        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn canary_passes_when_real_session_dirs_match_our_slug() {
        let home = unique_temp_home("canary-pass");
        let dir = home
            .join("projects")
            .join(cwd_slug("/home/user/repos/app"))
            .join("sessions")
            .join("s1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("metadata.json"),
            r#"{"session_id":"s1","working_dir":"/home/user/repos/app","created":"2026-03-01T00:00:00.000Z","turn_count":1}"#,
        )
        .unwrap();
        assert_eq!(
            verify_amplifier_layout_contract(&home),
            CanaryOutcome::Pass {
                sessions_checked: 1
            }
        );
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn canary_reports_broken_on_slug_divergence() {
        let home = unique_temp_home("canary-broken");
        // amplifier "changed" its slug algorithm: dir name no longer matches.
        let dir = home
            .join("projects")
            .join("home_user_repos_app") // hypothetical new scheme
            .join("sessions")
            .join("s1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("metadata.json"),
            r#"{"session_id":"s1","working_dir":"/home/user/repos/app","created":"2026-03-01T00:00:00.000Z"}"#,
        )
        .unwrap();
        assert!(matches!(
            verify_amplifier_layout_contract(&home),
            CanaryOutcome::Broken { .. }
        ));
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn canary_has_nothing_to_check_on_an_empty_or_missing_home() {
        let home = unique_temp_home("canary-empty");
        assert_eq!(
            verify_amplifier_layout_contract(&home),
            CanaryOutcome::NothingToCheck
        );
        assert_eq!(
            verify_amplifier_layout_contract(&home.join("missing")),
            CanaryOutcome::NothingToCheck
        );
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn canary_skips_validated_real_world_shapes_without_false_alarms() {
        // VALIDATED skip classes (F6/V5 census of the real corpus:
        // 5216/5216 parseable sessions match the slug, 0 mismatches; 2.4%
        // of sessions have NO metadata.json — events.jsonl-only; one
        // literal `{project}` template dir with no `sessions/` exists).
        let home = unique_temp_home("canary-skip");
        let slug = cwd_slug("/home/user/repos/app");
        // Skip class 1: session dir lacking metadata.json — skipped, not Broken.
        let no_meta = home
            .join("projects")
            .join(&slug)
            .join("sessions")
            .join("s-nometa");
        std::fs::create_dir_all(&no_meta).unwrap();
        std::fs::write(
            no_meta.join("events.jsonl"),
            "{\"event\":\"session:start\"}\n",
        )
        .unwrap();
        // Skip class 2: projects/ entry lacking a `sessions/` subdir.
        std::fs::create_dir_all(home.join("projects").join("{project}")).unwrap();
        // One qualifying session — the strict dir-name == cwd_slug(working_dir)
        // check still runs and passes.
        let ok = home
            .join("projects")
            .join(&slug)
            .join("sessions")
            .join("s-ok");
        std::fs::create_dir_all(&ok).unwrap();
        std::fs::write(
            ok.join("metadata.json"),
            r#"{"session_id":"s-ok","working_dir":"/home/user/repos/app","created":"2026-03-01T00:00:00.000Z","turn_count":1}"#,
        )
        .unwrap();
        assert_eq!(
            verify_amplifier_layout_contract(&home),
            CanaryOutcome::Pass {
                sessions_checked: 1
            }
        );
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn cwd_slug_matches_amplifiers_algorithm_exactly() {
        // project_utils.py:22-30: replace / \ : then ensure a leading '-'.
        assert_eq!(cwd_slug("/home/dan/code/pedal"), "-home-dan-code-pedal");
        // Dots and underscores are PRESERVED.
        assert_eq!(cwd_slug("/home/dan/my.project_x"), "-home-dan-my.project_x");
        // Root: "/" -> "-".
        assert_eq!(cwd_slug("/"), "-");
        // Windows-shaped input: backslashes -> '-', drive colon stripped,
        // and the result gains a leading '-' because it doesn't start with one.
        assert_eq!(cwd_slug("C:\\Users\\dan"), "-C-Users-dan");
        // Already-leading '-' is not doubled.
        assert_eq!(cwd_slug("-already"), "-already");
    }

    #[test]
    fn canonical_cwd_resolves_symlinks_and_falls_back_on_missing_dirs() {
        let tmp = std::env::temp_dir().join(format!(
            "freshell-amp-stub-canon-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        // Canonicalizing an existing dir yields the same path canonicalize does
        // (this also resolves /tmp -> /private/tmp style symlinks on macOS).
        assert_eq!(
            canonical_cwd(tmp.to_str().unwrap()),
            std::fs::canonicalize(&tmp).unwrap()
        );
        // A vanished dir falls back to the raw path (the spawn itself will
        // surface the real failure).
        let gone = tmp.join("does-not-exist");
        assert_eq!(canonical_cwd(gone.to_str().unwrap()), gone);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn resolve_amplifier_home_prefers_freshell_override_then_home_dot_amplifier() {
        // NOTE: env is process-global; this test is the only one in this
        // crate that sets FRESHELL_AMPLIFIER_HOME, and it restores the prior
        // value.
        let prior = std::env::var("FRESHELL_AMPLIFIER_HOME").ok();
        std::env::set_var("FRESHELL_AMPLIFIER_HOME", "/custom/amp/home");
        // The override IS the amplifier home ROOT, used as-is (callers join
        // `projects/...` onto it) — no `.amplifier` appended.
        assert_eq!(
            resolve_amplifier_home(),
            Some(std::path::PathBuf::from("/custom/amp/home"))
        );
        // Reconciliation (F1): the pre-existing index/resolver resolution
        // (`crate::amplifier::amplifier_home`, retargeted from AMPLIFIER_HOME
        // in this task's Step 3a) must AGREE with resolve_amplifier_home()
        // under both env states — otherwise the create-time events-lane
        // attach would look in a different home than the stub writer wrote
        // into.
        assert_eq!(
            crate::amplifier::amplifier_home(std::path::Path::new("/fake/home")),
            std::path::PathBuf::from("/custom/amp/home")
        );
        // Fallback: `$HOME/.amplifier` — the `.amplifier` segment IS
        // appended here, mirroring the CLI's hardcoded
        // `Path.home()/.amplifier` (session_store.py:96-98).
        std::env::remove_var("FRESHELL_AMPLIFIER_HOME");
        if let Ok(home) = std::env::var("HOME") {
            assert_eq!(
                resolve_amplifier_home(),
                Some(std::path::PathBuf::from(home).join(".amplifier"))
            );
        }
        assert_eq!(
            crate::amplifier::amplifier_home(std::path::Path::new("/fake/home")),
            std::path::PathBuf::from("/fake/home/.amplifier")
        );
        match prior {
            Some(v) => std::env::set_var("FRESHELL_AMPLIFIER_HOME", v),
            None => std::env::remove_var("FRESHELL_AMPLIFIER_HOME"),
        }
    }
}
