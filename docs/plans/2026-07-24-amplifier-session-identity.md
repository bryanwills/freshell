# Launcher-Assigned Amplifier Session Identity Implementation Plan

> **For agentic workers:** This plan is executed task-by-task by the
> workflow's execute stage: a fresh implementer per task, with a spec +
> quality review after each task. Steps use checkbox (`- [ ]`) syntax
> for tracking.

**Goal:** At amplifier terminal create time, the Rust broker mints a UUID, pre-creates the session on disk under `~/.amplifier/projects/<cwd-slug>/sessions/<uuid>/`, and spawns `amplifier resume <uuid>` — identity is known before spawn, and the entire fragile post-spawn correlation-window path (amplifier locator + association) is deleted.

**Architecture:** A new `freshell-sessions::amplifier_stub` module owns the amplifier on-disk contract writers (cwd→slug, stub writer, ensure-exists, GC predicate, layout canary). Both create paths — WS `terminal.create` (`freshell-ws/src/terminal.rs`) and REST `POST /api/tabs` (`freshell-freshagent/src/terminal_tabs.rs`) — call the same helpers. Setting `resume_session_id` before spawn makes ALL existing plumbing (argv `resume <uuid>` via the manifest resumeArgs template, `registry.set_meta`, identity upsert, `terminal.created.sessionRef`, activity events-lane attach via the resolver) work with zero client changes. `crates/freshell-sessions/src/amplifier_locator.rs` and `crates/freshell-ws/src/amplifier_association.rs` are deleted with all their plumbing; the `terminal_identity_unresolved` invariant alarm is re-homed onto its own sweep.

**Tech Stack:** Rust (workspace crates: freshell-sessions, freshell-terminal, freshell-platform, freshell-ws, freshell-freshagent, freshell-server), vitest (TS integration/real contract tests), Playwright (e2e). New deps in `freshell-sessions`: promote `uuid` to `[dependencies]`, add `chrono` (both have workspace precedent).

## Global Constraints

- **Rust ONLY.** Do NOT modify the legacy Node server's amplifier correlation code (`server/coding-cli/amplifier-session-locator.ts`, `server/session-association-coordinator.ts`, `server/index.ts`, etc. stay untouched). Node-side test files may only be ADDED under `test/integration/real/` and modified under `test/e2e-browser/`.
- **Keep `LaunchIntent::Resume` for amplifier.** The amplifier manifest (`extensions/amplifier/freshell.json`) has `resumeArgs: ["resume", "{{sessionId}}"]` only; `LaunchIntent::Start` without `createSessionArgs` is a hard `StartIntentUnsupported` error (`crates/freshell-platform/src/cli_launch.rs:431-445`, message `Fresh Amplifier launch requires createSessionArgs support.`). No argv/manifest changes.
- **Slug algorithm is an external contract** (amplifier_app_cli `project_utils.py:22-30`): `slug = str(Path.cwd().resolve()).replace("/", "-").replace("\\", "-").replace(":", "")`, prefixed with `-` if not already starting with one. E.g. `/home/dan/code/pedal` → `-home-dan-code-pedal`; dots and underscores preserved.
- **cwd is part of the identity contract (HARD INVARIANT):** `amplifier resume <id>` only searches the current cwd's project slug. The stub must be created under the slug of the exact (canonicalized) cwd the PTY will spawn with.
- **Stub shape (designed path, not accidental tolerances):** `metadata.json` with `session_id`, `created` (ISO-8601 with tz), `working_dir` (resolved cwd), custom `freshell_terminal_id` key; NO `bundle` key (so the user's default bundle resolves); plus an empty `transcript.jsonl`. This plan additionally writes an empty `events.jsonl` (rationale in Task 3).
- **GC never-used signature:** `metadata.json` lacks `turn_count` AND `transcript.jsonl` is empty/absent. Tolerate an `events.jsonl` of any size (zero-turn resumes may create a small one). GC only dirs the broker itself created.
- **Do NOT touch** `crates/freshell-sessions/src/opencode_locator.rs` / `crates/freshell-ws/src/opencode_association.rs` (sibling pattern, out of scope) — sole exception: the one-line `amplifier_locator: None,` test-struct field at `opencode_association.rs:274` must be removed when the `WsState` field is deleted (compile requirement, not a behavior change).
- **Keep** the `terminal_identity_unresolved` alarm in `crates/freshell-ws/src/invariants.rs`; inline `IDENTITY_RESOLUTION_GRACE_MS = 10_000` (it currently derives from the deleted locator's `AMPLIFIER_DIR_APPEAR_WINDOW_MS`).
- **Keep** `AMPLIFIER_LOCATOR_SWEEP_INTERVAL` (`main.rs:1112`) — it is shared with the opencode sweep; rename to `LOCATOR_SWEEP_INTERVAL`.
- Repo conventions (AGENTS.md, binding): Red-Green-Refactor TDD; coordinated test runs (`npm test`, `npm run test:vitest -- ...` — never raw `npx vitest`); Rust: `cargo test -p <crate>`; e2e: `npm run test:e2e -- --project=rust-chromium`; NodeNext ESM — relative TS imports need `.js` extensions; never restart the self-hosted Freshell server; commit frequently; push the branch but do NOT create a PR without explicit user approval.
- Worktree: `/home/dan/code/freshell/.worktrees/amplifier-session-identity`, branch `feat/amplifier-session-identity`.

---

## File Structure

**Created:**
- `crates/freshell-sessions/src/amplifier_stub.rs` — slug fn, home resolution, stub writer/ensure, GC, layout canary (+ unit tests in-file)
- `crates/freshell-ws/tests/amplifier_session_identity.rs` — WS wire+argv+disk integration test (private-harness style)
- `test/integration/real/amplifier-stub-adoption-contract.test.ts` — Tier-1 real-CLI contract test (opt-in gated)

**Modified:**
- `crates/freshell-sessions/Cargo.toml` — promote `uuid`, add `chrono`
- `crates/freshell-sessions/src/lib.rs` — `pub mod amplifier_stub;` added; `pub mod amplifier_locator;` removed (Task 13)
- `crates/freshell-terminal/src/registry.rs` — `has_live_resume` helper
- `crates/freshell-platform/src/cli_launch_goldens.rs` — G-A4 golden
- `crates/freshell-ws/src/terminal.rs` — amplifier preallocation branch, reject/guard, stub write, GC in exit hook; locator call sites removed
- `crates/freshell-ws/src/lib.rs` — `amplifier_locator` field removed; `invariants` made `pub`
- `crates/freshell-ws/src/invariants.rs` — const inlined; `spawn_identity_invariant_sweep` added
- `crates/freshell-ws/tests/common/mod.rs` — `isolate_amplifier_home`, amplifier sleeper `resume_args` fixed to `["resume","{{sessionId}}"]`
- `crates/freshell-freshagent/src/terminal_tabs.rs` — shared pre-create; locator plumbing stripped; tests replaced
- `crates/freshell-freshagent/src/lib.rs` — `amplifier_locator` field/builder removed
- `crates/freshell-server/src/main.rs` — locator wiring removed, invariant sweep + canary spawned, const renamed
- 12 `crates/freshell-ws/tests/*` files + `crates/freshell-ws/src/opencode_association.rs:274` — one-line `amplifier_locator: None,` removals
- `test/e2e-browser/fixtures/fake-amplifier-cli.mjs`, `test/e2e-browser/specs/amplifier-restore-rust.spec.ts` — rewritten for the new mechanism

**Deleted:**
- `crates/freshell-sessions/src/amplifier_locator.rs` (1047 lines incl. 15 tests)
- `crates/freshell-ws/src/amplifier_association.rs` (479 lines incl. 6 tests)

> **Line numbers** in this plan were verified against worktree HEAD `cdb760cc`. They will drift a few lines as earlier tasks land — every edit site also gives a searchable code anchor; trust the anchor over the number.

---

### Task 1: Tier-1 real-CLI contract test (contract-first, opt-in)

This is a contract PIN against the real amplifier CLI, written BEFORE any broker code. It is not red-green against our code — it encodes the external contract the whole feature rests on (stub adoption + slug algorithm), and self-skips when `amplifier` is not on PATH or the opt-in gate is off.

**Files:**
- Create: `test/integration/real/amplifier-stub-adoption-contract.test.ts`

**Interfaces:**
- Consumes: nothing from this repo (spawns the real `amplifier` binary; template: `test/integration/real/amplifier-launch-smoke.test.ts` gates).
- Produces: the TS reference implementation of `cwdSlug` and the stub shape that Task 2/3's Rust code must byte-match.

- [ ] **Step 1: Write the contract test**

```ts
// @vitest-environment node
//
// Real Amplifier stub-adoption contract (launcher-assigned session identity).
//
// The Rust broker pre-creates ~/.amplifier/projects/<cwd-slug>/sessions/<id>/
// stubs and spawns `amplifier resume <id>`. This test pins the two external
// contracts that path rests on, against the REAL CLI:
//   1. STUB ADOPTION: `amplifier resume <id>` of a pre-created stub is
//      accepted (not rejected like an unknown id), the metadata survives in
//      place, and custom keys (freshell_terminal_id) are preserved.
//      Adoption also implicitly proves the slug: amplifier only searches the
//      CURRENT cwd's project slug, so finding our stub means our slug
//      matched its algorithm.
//   2. SLUG ALGORITHM (explicit, key-gated): a real headless turn creates
//      amplifier's own session dir; its project dir name must equal our
//      computed slug, and its metadata must carry `turn_count` (the GC
//      "used" signature).
//
// Gates mirror amplifier-launch-smoke.test.ts: on-PATH probe (top-level
// await), FRESHELL_RUN_REAL_PROVIDER_CONTRACTS=1, provider key for the
// turn-making test. Opt-in run:
//   FRESHELL_RUN_REAL_PROVIDER_CONTRACTS=1 npm run test:vitest -- \
//     run test/integration/real/amplifier-stub-adoption-contract.test.ts \
//     --config config/vitest/vitest.server.config.ts
//
import { execFile, spawn } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { promisify } from 'node:util'

import { describe, it, expect } from 'vitest'

const execFileAsync = promisify(execFile)

async function amplifierOnPath(): Promise<boolean> {
  try {
    await execFileAsync('amplifier', ['--version'], { timeout: 15_000 })
    return true
  } catch {
    return false
  }
}

const onPath = await amplifierOnPath()
const realEnabled = process.env.FRESHELL_RUN_REAL_PROVIDER_CONTRACTS === '1'
const hasProviderKey = Boolean(
  process.env.ANTHROPIC_API_KEY
  || process.env.OPENAI_API_KEY
  || process.env.AZURE_OPENAI_API_KEY
  || process.env.GOOGLE_API_KEY,
)

// The slug contract (amplifier_app_cli project_utils.py:22-30). The Rust
// twin is freshell_sessions::amplifier_stub::cwd_slug — keep byte-identical.
function cwdSlug(resolvedCwd: string): string {
  const slug = resolvedCwd.replaceAll('/', '-').replaceAll('\\', '-').replaceAll(':', '')
  return slug.startsWith('-') ? slug : `-${slug}`
}

// The exact stub shape the Rust broker writes (plan Global Constraints).
async function writeStub(home: string, resolvedCwd: string, sessionId: string): Promise<string> {
  const dir = path.join(home, 'projects', cwdSlug(resolvedCwd), 'sessions', sessionId)
  await fs.mkdir(dir, { recursive: true })
  await fs.writeFile(path.join(dir, 'metadata.json'), JSON.stringify({
    session_id: sessionId,
    created: new Date().toISOString(),
    working_dir: resolvedCwd,
    freshell_terminal_id: 'contract-test-terminal',
  }))
  await fs.writeFile(path.join(dir, 'transcript.jsonl'), '')
  await fs.writeFile(path.join(dir, 'events.jsonl'), '')
  return dir
}

// Spawn `amplifier resume <id>` (interactive), collect combined output for
// up to timeoutMs, then SIGTERM. We never make a turn — a zero-turn resume
// is the validated adoption shape.
function runResume(sessionId: string, opts: { home: string; cwd: string; timeoutMs: number }): Promise<string> {
  return new Promise((resolve) => {
    const child = spawn('amplifier', ['resume', sessionId], {
      cwd: opts.cwd,
      env: { ...process.env, AMPLIFIER_HOME: opts.home, PROMPT_TOOLKIT_NO_CPR: '1' },
    })
    let output = ''
    child.stdout.on('data', (d) => { output += String(d) })
    child.stderr.on('data', (d) => { output += String(d) })
    const timer = setTimeout(() => child.kill('SIGTERM'), opts.timeoutMs)
    child.on('close', () => { clearTimeout(timer); resolve(output) })
  })
}

describe('amplifier stub-adoption contract (real CLI)', () => {
  const itAdoption = onPath && realEnabled ? it : it.skip
  itAdoption('adopts a broker-shaped pre-created stub under the cwd slug', async () => {
    const home = await fs.mkdtemp(path.join(os.tmpdir(), 'amp-contract-home-'))
    const cwdRaw = await fs.mkdtemp(path.join(os.tmpdir(), 'amp-contract-cwd-'))
    const cwd = await fs.realpath(cwdRaw) // mirror Path.cwd().resolve()
    try {
      // Self-calibrating negative probe: what does a resume of an UNKNOWN id
      // look like? (No hardcoded CLI error strings.)
      const unknownOut = await runResume(randomUUID(), { home, cwd, timeoutMs: 20_000 })

      const sessionId = randomUUID()
      const dir = await writeStub(home, cwd, sessionId)
      const stubOut = await runResume(sessionId, { home, cwd, timeoutMs: 20_000 })

      // Adoption: the stub resume must NOT behave like the unknown-id resume.
      expect(stubOut).not.toEqual(unknownOut)

      // The dir survived, metadata still parses, identity + custom key intact.
      const meta = JSON.parse(await fs.readFile(path.join(dir, 'metadata.json'), 'utf8'))
      expect(meta.session_id).toBe(sessionId)
      expect(meta.freshell_terminal_id).toBe('contract-test-terminal')
      // Zero-turn adoption must not mark the session used (GC contract).
      expect(meta.turn_count).toBeUndefined()
    } finally {
      await fs.rm(home, { recursive: true, force: true }).catch(() => {})
      await fs.rm(cwdRaw, { recursive: true, force: true }).catch(() => {})
    }
  }, 120_000)

  const itSlug = onPath && realEnabled && hasProviderKey ? it : it.skip
  itSlug('creates its own session dirs under exactly our computed slug, with turn_count', async () => {
    const home = await fs.mkdtemp(path.join(os.tmpdir(), 'amp-contract-slug-'))
    const cwdRaw = await fs.mkdtemp(path.join(os.tmpdir(), 'amp-contract-slugcwd-'))
    const cwd = await fs.realpath(cwdRaw)
    try {
      await execFileAsync(
        'amplifier',
        ['run', '--output-format', 'json', 'Reply with exactly: contract-ok'],
        {
          cwd,
          env: { ...process.env, AMPLIFIER_HOME: home, PROMPT_TOOLKIT_NO_CPR: '1' },
          timeout: 180_000,
          maxBuffer: 16 * 1024 * 1024,
        },
      )
      const projectDirs = await fs.readdir(path.join(home, 'projects'))
      // EXACT-match slug contract: a mismatch here fails silently in prod
      // (stub dir and amplifier's own dir diverge), so this must be strict.
      expect(projectDirs).toContain(cwdSlug(cwd))
      const sessionsDir = path.join(home, 'projects', cwdSlug(cwd), 'sessions')
      const sessions = await fs.readdir(sessionsDir)
      expect(sessions.length).toBeGreaterThan(0)
      const meta = JSON.parse(
        await fs.readFile(path.join(sessionsDir, sessions[0], 'metadata.json'), 'utf8'),
      )
      // The "used" signature the broker's GC keys off.
      expect(meta.turn_count).toBeDefined()
      expect(meta.working_dir).toBe(cwd)
    } finally {
      await fs.rm(home, { recursive: true, force: true }).catch(() => {})
      await fs.rm(cwdRaw, { recursive: true, force: true }).catch(() => {})
    }
  }, 240_000)
})
```

- [ ] **Step 2: Run it (gated run if the environment allows; otherwise verify it self-skips)**

Run: `npm run test:vitest -- run test/integration/real/amplifier-stub-adoption-contract.test.ts --config config/vitest/vitest.server.config.ts`
Expected: both tests SKIP (no opt-in env), zero failures. If `amplifier` is on PATH and you can set `FRESHELL_RUN_REAL_PROVIDER_CONTRACTS=1`, run again with it set: the adoption test must PASS (the slug test additionally needs a provider key).

- [ ] **Step 3: Commit**

```bash
git add test/integration/real/amplifier-stub-adoption-contract.test.ts
git commit -m "test(amplifier): real-CLI contract pin for stub adoption and cwd-slug algorithm"
```

---

### Task 2: `freshell-sessions::amplifier_stub` — slug function + module scaffold

**Files:**
- Create: `crates/freshell-sessions/src/amplifier_stub.rs`
- Modify: `crates/freshell-sessions/src/lib.rs` (add `pub mod amplifier_stub;` after line 17's `pub mod amplifier;`)
- Modify: `crates/freshell-sessions/Cargo.toml` (move `uuid = { version = "1", features = ["v4"] }` from `[dev-dependencies]` to `[dependencies]`; add `chrono = { version = "0.4", default-features = false, features = ["clock"] }` — both specs have workspace precedent, e.g. `crates/freshell-server/Cargo.toml`)

**Interfaces:**
- Consumes: nothing.
- Produces: `pub fn cwd_slug(resolved_cwd: &str) -> String`, `pub fn canonical_cwd(cwd: &str) -> PathBuf`, `pub fn resolve_amplifier_home() -> Option<PathBuf>` (Tasks 3, 9, 12 build on these).

- [ ] **Step 1: Write the failing unit tests** — create `crates/freshell-sessions/src/amplifier_stub.rs` containing ONLY the test module for now:

```rust
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn resolve_amplifier_home_prefers_amplifier_home_env() {
        // NOTE: env is process-global; this test is the only one in this
        // crate that sets AMPLIFIER_HOME, and it restores the prior value.
        let prior = std::env::var("AMPLIFIER_HOME").ok();
        std::env::set_var("AMPLIFIER_HOME", "/custom/amp/home");
        assert_eq!(
            resolve_amplifier_home(),
            Some(std::path::PathBuf::from("/custom/amp/home"))
        );
        match prior {
            Some(v) => std::env::set_var("AMPLIFIER_HOME", v),
            None => std::env::remove_var("AMPLIFIER_HOME"),
        }
    }
}
```

- [ ] **Step 2: Wire the module and run tests to verify they fail**

Add `pub mod amplifier_stub;` to `crates/freshell-sessions/src/lib.rs` (in the alphabetical `pub mod` block, directly after `pub mod amplifier;`). Apply the Cargo.toml dependency changes.

Run: `cargo test -p freshell-sessions amplifier_stub`
Expected: COMPILE FAILURE — `cwd_slug`, `canonical_cwd`, `resolve_amplifier_home` not found.

- [ ] **Step 3: Implement the three functions** (above the test module):

```rust
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

/// `AMPLIFIER_HOME` env (non-empty) else `$HOME/.amplifier`. Mirrors
/// [`crate::amplifier::amplifier_home`] + freshell-server's `provider_home()`
/// semantics (real `HOME` only — deliberately NOT `FRESHELL_HOME`). `None`
/// when neither resolves (callers surface a create error).
pub fn resolve_amplifier_home() -> Option<PathBuf> {
    match std::env::var("AMPLIFIER_HOME") {
        Ok(v) if !v.is_empty() => Some(PathBuf::from(v)),
        _ => std::env::var("HOME")
            .ok()
            .filter(|v| !v.is_empty())
            .map(|h| PathBuf::from(h).join(".amplifier")),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p freshell-sessions amplifier_stub`
Expected: 3 tests PASS. Also run `cargo test -p freshell-sessions` to confirm no regressions.

- [ ] **Step 5: Commit**

```bash
git add crates/freshell-sessions/src/amplifier_stub.rs crates/freshell-sessions/src/lib.rs crates/freshell-sessions/Cargo.toml Cargo.lock
git commit -m "feat(sessions): amplifier cwd-slug contract + home resolution in new amplifier_stub module"
```

---

### Task 3: Stub writer + `ensure_session`

**Files:**
- Modify: `crates/freshell-sessions/src/amplifier_stub.rs`

**Interfaces:**
- Consumes: `cwd_slug`, `canonical_cwd` (Task 2).
- Produces: `pub struct EnsuredSession { pub session_dir: PathBuf, pub created: bool }` and `pub fn ensure_session(amplifier_home: &Path, session_id: &str, cwd: &str, terminal_id: &str) -> std::io::Result<EnsuredSession>` (Tasks 9, 12).

**Design note (events.jsonl):** the stub also includes an empty `events.jsonl`. Rationale: the activity hub's create-time events-lane attach rides `resolve_amplifier_events_path` (`crates/freshell-server/src/main.rs:1019-1032` → `crates/freshell-ws/src/activity.rs:281-296`), which fires on `ActivityEvent::Created` for any amplifier terminal with a `resume_session_id` — but only if `events.jsonl` already `is_file()`. Pre-creating the empty file makes the existing, already-tested resolver path attach at create time for BOTH the WS and REST paths with zero new cross-crate plumbing (attach at `Eof` of an empty file ≡ `Start`). This is a file amplifier itself creates and appends to — not reliance on a tolerance — and Task 1's adoption contract test runs against a stub containing it.

- [ ] **Step 1: Add failing unit tests** (inside `mod tests`):

```rust
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
        let ensured =
            ensure_session(&home, "11111111-2222-3333-4444-555555555555", cwd.to_str().unwrap(), "term-1")
                .unwrap();
        assert!(ensured.created);

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
        assert_eq!(std::fs::metadata(expected_dir.join("transcript.jsonl")).unwrap().len(), 0);
        assert_eq!(std::fs::metadata(expected_dir.join("events.jsonl")).unwrap().len(), 0);

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
        assert!(!ensured.created, "existing sessions are found, never re-stubbed");
        assert_eq!(ensured.session_dir, existing);
        // Untouched: still has turn_count, no freshell_terminal_id injected.
        let meta: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(existing.join("metadata.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(meta["turn_count"], 3);
        assert!(meta.get("freshell_terminal_id").is_none());

        std::fs::remove_dir_all(&home).ok();
        std::fs::remove_dir_all(&cwd).ok();
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p freshell-sessions amplifier_stub`
Expected: COMPILE FAILURE — `ensure_session` / `EnsuredSession` not found.

- [ ] **Step 3: Implement**

```rust
/// The outcome of [`ensure_session`]: where the session dir is, and whether
/// THIS call created it (`created` gates the exit-hook GC — the broker only
/// ever deletes litter it wrote itself).
#[derive(Debug, Clone)]
pub struct EnsuredSession {
    pub session_dir: PathBuf,
    pub created: bool,
}

/// Make `amplifier resume <session_id>` guaranteed-resumable from `cwd`
/// BEFORE spawn. If the session dir already exists under ANY project slug
/// (a real session, or a stub from a previous run), it is found and left
/// untouched. Otherwise a stub is written under the slug of the CANONICAL
/// cwd (HARD INVARIANT: amplifier only searches the current cwd's slug —
/// the caller must spawn the PTY with this same cwd).
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
    let projects = amplifier_home.join("projects");
    if let Ok(entries) = std::fs::read_dir(&projects) {
        for entry in entries.flatten() {
            let candidate = entry.path().join("sessions").join(session_id);
            if candidate.is_dir() {
                return Ok(EnsuredSession { session_dir: candidate, created: false });
            }
        }
    }

    let resolved = canonical_cwd(cwd);
    let dir = projects
        .join(cwd_slug(&resolved.to_string_lossy()))
        .join("sessions")
        .join(session_id);
    std::fs::create_dir_all(&dir)?;
    let metadata = serde_json::json!({
        "session_id": session_id,
        "created": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "working_dir": resolved.to_string_lossy(),
        "freshell_terminal_id": terminal_id,
    });
    std::fs::write(dir.join("metadata.json"), serde_json::to_string_pretty(&metadata)?)?;
    std::fs::write(dir.join("transcript.jsonl"), "")?;
    std::fs::write(dir.join("events.jsonl"), "")?;
    Ok(EnsuredSession { session_dir: dir, created: true })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p freshell-sessions amplifier_stub`
Expected: all PASS (5 tests so far).

- [ ] **Step 5: Commit**

```bash
git add crates/freshell-sessions/src/amplifier_stub.rs
git commit -m "feat(sessions): amplifier stub writer with ensure-exists semantics"
```

---

### Task 4: Stub GC predicate

**Files:**
- Modify: `crates/freshell-sessions/src/amplifier_stub.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `pub fn stub_is_unused(session_dir: &Path) -> bool`, `pub fn gc_stub_if_unused(session_dir: &Path) -> bool` (Tasks 9, 12).

- [ ] **Step 1: Add failing unit tests**

```rust
    fn write_gc_fixture(home: &PathBuf, id: &str, metadata: &str, transcript: Option<&str>) -> PathBuf {
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
        let meta_unused = r#"{"session_id":"s","working_dir":"/x","created":"2026-03-01T00:00:00.000Z"}"#;
        let meta_used = r#"{"session_id":"s","working_dir":"/x","created":"2026-03-01T00:00:00.000Z","turn_count":1}"#;

        // Never used: no turn_count + empty transcript.
        assert!(stub_is_unused(&write_gc_fixture(&home, "a", meta_unused, Some(""))));
        // Never used: no turn_count + transcript ABSENT.
        assert!(stub_is_unused(&write_gc_fixture(&home, "b", meta_unused, None)));
        // Used: turn_count present.
        assert!(!stub_is_unused(&write_gc_fixture(&home, "c", meta_used, Some(""))));
        // Used: non-empty transcript (even without turn_count).
        assert!(!stub_is_unused(&write_gc_fixture(&home, "d", meta_unused, Some("{\"role\":\"user\"}\n"))));
        // A zero-turn resume may create a small events.jsonl — tolerated.
        let e = write_gc_fixture(&home, "e", meta_unused, Some(""));
        std::fs::write(e.join("events.jsonl"), "{\"event\":\"session:start\"}\n").unwrap();
        assert!(stub_is_unused(&e));
        // Missing metadata.json: NOT recognizably a stub — never delete.
        let f = home.join("projects").join("-p").join("sessions").join("f");
        std::fs::create_dir_all(&f).unwrap();
        assert!(!stub_is_unused(&f));

        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn gc_stub_if_unused_deletes_only_unused_dirs() {
        let home = unique_temp_home("gc-rm");
        let meta_unused = r#"{"session_id":"s","working_dir":"/x","created":"2026-03-01T00:00:00.000Z"}"#;
        let meta_used = r#"{"session_id":"s","working_dir":"/x","created":"2026-03-01T00:00:00.000Z","turn_count":2}"#;
        let unused = write_gc_fixture(&home, "u", meta_unused, Some(""));
        let used = write_gc_fixture(&home, "v", meta_used, Some(""));

        assert!(gc_stub_if_unused(&unused));
        assert!(!unused.exists());
        assert!(!gc_stub_if_unused(&used));
        assert!(used.exists());

        std::fs::remove_dir_all(&home).ok();
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p freshell-sessions amplifier_stub`
Expected: COMPILE FAILURE — `stub_is_unused` / `gc_stub_if_unused` not found.

- [ ] **Step 3: Implement**

```rust
/// The verified-unambiguous "never used" signature: `metadata.json` lacks
/// `turn_count` AND `transcript.jsonl` is empty or absent. An `events.jsonl`
/// of any size is tolerated (zero-turn resumes leave metadata byte-identical
/// but may write a small events file). A dir without parseable metadata is
/// NOT recognizably a stub — never touched.
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
        Ok(m) => m.len() == 0,
        Err(_) => true,
    }
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p freshell-sessions amplifier_stub`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/freshell-sessions/src/amplifier_stub.rs
git commit -m "feat(sessions): never-used stub GC predicate and remover"
```

---

### Task 5: Layout/version canary

**Files:**
- Modify: `crates/freshell-sessions/src/amplifier_stub.rs`

**Interfaces:**
- Consumes: `cwd_slug` (Task 2).
- Produces: `pub enum CanaryOutcome { Pass { sessions_checked: usize }, NothingToCheck, Broken { detail: String } }`, `pub fn verify_amplifier_layout_contract(amplifier_home: &Path) -> CanaryOutcome` (Task 14 wires it into broker start).

- [ ] **Step 1: Add failing unit tests**

```rust
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
            CanaryOutcome::Pass { sessions_checked: 1 }
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
        assert_eq!(verify_amplifier_layout_contract(&home), CanaryOutcome::NothingToCheck);
        assert_eq!(
            verify_amplifier_layout_contract(&home.join("missing")),
            CanaryOutcome::NothingToCheck
        );
        std::fs::remove_dir_all(&home).ok();
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p freshell-sessions amplifier_stub canary`
Expected: COMPILE FAILURE.

- [ ] **Step 3: Implement**

```rust
/// Outcome of the boot-time layout canary ([`verify_amplifier_layout_contract`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanaryOutcome {
    Pass { sessions_checked: usize },
    /// No amplifier home / no sessions with a `working_dir` — nothing to
    /// verify (amplifier unused or brand new). Not an error.
    NothingToCheck,
    Broken { detail: String },
}

/// Cheap, re-runnable self-test of the on-disk contract this whole feature
/// rests on (undocumented upstream; microsoft/amplifier#315/#316 track a
/// `--session-id` flag that would collapse this layer into a flag): for a
/// bounded sample of sessions AMPLIFIER ITSELF wrote, verify the project dir
/// name equals [`cwd_slug`] of the session's own `working_dir`. A mismatch
/// means amplifier changed its slug/layout and our pre-created stubs would
/// silently diverge — callers log ERROR loudly but MUST NOT block broker
/// start.
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
                return CanaryOutcome::Pass { sessions_checked: checked };
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
        CanaryOutcome::Pass { sessions_checked: checked }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p freshell-sessions amplifier_stub`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/freshell-sessions/src/amplifier_stub.rs
git commit -m "feat(sessions): amplifier on-disk layout canary (slug contract self-test)"
```

---

### Task 6: Argv golden — pin that the pre-create branch keeps `LaunchIntent::Resume`

**Files:**
- Modify: `crates/freshell-platform/src/cli_launch_goldens.rs` (append after `g_a3_amplifier_env_var_override`, ~line 742; helpers `amplifier_spec()`/`amplifier_inputs()` already exist at :681-708)

**Interfaces:**
- Consumes: existing golden harness (`specs()`, `amplifier_spec()`, `amplifier_inputs()`, `env_of`, `resolve_coding_cli_command`).
- Produces: nothing (a pin). G-A2 (`g_a2_amplifier_resume_appends_resume_args`, :709-726) already proves `Resume` + id → `["resume", "<id>"]` — exactly the argv the new branch produces.

- [ ] **Step 1: Write the golden (a pin — it passes immediately; that is its job)**

```rust
/// G-A4 — the amplifier pre-create branch (launcher-assigned session
/// identity plan §1) MUST keep `LaunchIntent::Resume`: the manifest has
/// `resumeArgs` only, so `Start` with a preallocated id is a hard
/// `StartIntentUnsupported` error. `amplifier resume <uuid>` of the
/// pre-created stub IS the fresh-session launch (G-A2 pins that argv).
#[test]
fn g_a4_amplifier_start_intent_with_preallocated_id_errors() {
    let mut all_specs = specs();
    all_specs.push(amplifier_spec());
    let mut inputs = amplifier_inputs(Some("11111111-2222-3333-4444-555555555555"));
    inputs.launch_intent = LaunchIntent::Start;
    let err = resolve_coding_cli_command(&all_specs, &inputs, &env_of(&[])).unwrap_err();
    assert_eq!(
        err.message(),
        "Fresh Amplifier launch requires createSessionArgs support."
    );
}
```

- [ ] **Step 2: Run**

Run: `cargo test -p freshell-platform g_a4_amplifier`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/freshell-platform/src/cli_launch_goldens.rs
git commit -m "test(platform): G-A4 golden pins Resume-intent requirement for amplifier pre-create"
```

---

### Task 7: `has_live_resume` helper in freshell-terminal

Shared home for the double-resume guard — both `freshell-ws` and `freshell-freshagent` depend on `freshell-terminal`, and `IdentityProbeRow` already lives there (`crates/freshell-terminal/src/registry.rs:279-293`).

**Files:**
- Modify: `crates/freshell-terminal/src/registry.rs`

**Interfaces:**
- Consumes: `IdentityProbeRow` (fields: `terminal_id: String`, `mode: String`, `status: TerminalRunStatus`, `created_at: i64`, `resume_session_id: Option<String>`, `cwd: Option<String>`), `TerminalRunStatus` (freshell_protocol).
- Produces: `pub fn has_live_resume(rows: &[IdentityProbeRow], mode: &str, session_id: &str) -> bool` (Tasks 10, 12).

- [ ] **Step 1: Write the failing test** (in `registry.rs`'s existing `#[cfg(test)]` module; follow its local conventions for constructing rows):

```rust
    #[test]
    fn has_live_resume_matches_only_running_terminals_of_the_same_mode_and_id() {
        let rows = vec![
            IdentityProbeRow {
                terminal_id: "t1".into(),
                mode: "amplifier".into(),
                status: TerminalRunStatus::Running,
                created_at: 0,
                resume_session_id: Some("sess-x".into()),
                cwd: None,
            },
            IdentityProbeRow {
                terminal_id: "t2".into(),
                mode: "amplifier".into(),
                status: TerminalRunStatus::Exited,
                created_at: 0,
                resume_session_id: Some("sess-y".into()),
                cwd: None,
            },
            IdentityProbeRow {
                terminal_id: "t3".into(),
                mode: "codex".into(),
                status: TerminalRunStatus::Running,
                created_at: 0,
                resume_session_id: Some("sess-z".into()),
                cwd: None,
            },
        ];
        assert!(has_live_resume(&rows, "amplifier", "sess-x"));
        assert!(!has_live_resume(&rows, "amplifier", "sess-y"), "exited terminals don't block");
        assert!(!has_live_resume(&rows, "amplifier", "sess-z"), "other modes don't block");
        assert!(!has_live_resume(&rows, "amplifier", "sess-unknown"));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p freshell-terminal has_live_resume`
Expected: COMPILE FAILURE.

- [ ] **Step 3: Implement** (free function next to `IdentityProbeRow`):

```rust
/// Same-id double-resume guard (launcher-assigned amplifier identity plan
/// §11): does any RUNNING terminal of `mode` already carry `session_id` as
/// its resume id? Amplifier has no upstream concurrency guard — two live
/// PTYs resuming one session id would interleave writes into one session
/// dir. Shared here so both the WS create path (`freshell-ws`) and the REST
/// create path (`freshell-freshagent`) apply the identical predicate.
pub fn has_live_resume(rows: &[IdentityProbeRow], mode: &str, session_id: &str) -> bool {
    rows.iter().any(|row| {
        row.mode == mode
            && row.status == TerminalRunStatus::Running
            && row.resume_session_id.as_deref() == Some(session_id)
    })
}
```

(If `TerminalRunStatus` isn't already imported at the top of `registry.rs`, it is — `IdentityProbeRow.status` uses it.)

- [ ] **Step 4: Run tests**

Run: `cargo test -p freshell-terminal`
Expected: PASS, no regressions.

- [ ] **Step 5: Commit**

```bash
git add crates/freshell-terminal/src/registry.rs
git commit -m "feat(terminal): has_live_resume predicate for same-id double-resume guard"
```

---

### Task 8: WS test-harness prep — isolated AMPLIFIER_HOME + real amplifier resume args

Once Task 9 lands, EVERY amplifier-mode create writes into the amplifier home. Without isolation, existing shared-harness tests (e.g. `session_identity_frames.rs`) would litter the developer's real `~/.amplifier`. Do this BEFORE the production change.

**Files:**
- Modify: `crates/freshell-ws/tests/common/mod.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `pub fn isolate_amplifier_home() -> &'static std::path::Path` (called by `spawn_server()` and by Task 9's new test); the `sleeper_cli_spec` amplifier registration now has manifest-true `resume_args`.

- [ ] **Step 1: Add the helper** (near the top of `common/mod.rs`, after `AUTH_TOKEN`):

```rust
/// Process-wide isolated AMPLIFIER_HOME. The amplifier pre-create path
/// (launcher-assigned identity plan) writes stub session dirs at terminal
/// create time — without this, any shared-harness test that creates an
/// amplifier terminal would litter the developer's real ~/.amplifier.
/// OnceLock ⇒ a single `set_var` per process with one stable value, safe
/// under parallel tests (mirrors the CODEX_CMD env discipline in
/// `codex_session_ref_resume.rs`).
pub fn isolate_amplifier_home() -> &'static std::path::Path {
    use std::sync::OnceLock;
    static HOME: OnceLock<std::path::PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!(
            "freshell-ws-test-amplifier-home-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create isolated AMPLIFIER_HOME");
        std::env::set_var("AMPLIFIER_HOME", &dir);
        dir
    })
}
```

- [ ] **Step 2: Call it first thing inside `spawn_server()`** (`common/mod.rs:82`):

```rust
pub async fn spawn_server() -> (String, freshell_terminal::TerminalRegistry) {
    isolate_amplifier_home();
    // ... existing body unchanged ...
```

- [ ] **Step 3: Fix the amplifier sleeper spec's resume args** — in `sleeper_cli_spec` (`common/mod.rs:58-77`) the spec is registered under BOTH names `amplifier` and `claude`; its `resume_args` are claude-shaped. Make the fn honest per name by replacing the single hardcoded field:

```rust
        // Manifest-true resume args per provider: amplifier is
        // `["resume", "{{sessionId}}"]` (extensions/amplifier/freshell.json),
        // claude-shaped specs keep `["--resume", "{{sessionId}}"]`.
        resume_args: Some(if name == "amplifier" {
            vec!["resume".to_string(), "{{sessionId}}".to_string()]
        } else {
            vec!["--resume".to_string(), "{{sessionId}}".to_string()]
        }),
```

- [ ] **Step 4: Run the existing WS suite to verify no regressions**

Run: `cargo test -p freshell-ws`
Expected: all PASS (behavior-neutral until Task 9; the resume-args shape change only affects argv content no shared test asserts).

- [ ] **Step 5: Commit**

```bash
git add crates/freshell-ws/tests/common/mod.rs
git commit -m "test(ws): isolated AMPLIFIER_HOME harness + manifest-true amplifier resume args"
```

---

### Task 9: WS create path — preallocate + pre-create stub + identity frames + argv

**Files:**
- Modify: `crates/freshell-ws/src/terminal.rs`
- Create: `crates/freshell-ws/tests/amplifier_session_identity.rs`

**Interfaces:**
- Consumes: `freshell_sessions::amplifier_stub::{resolve_amplifier_home, ensure_session, EnsuredSession}` (Tasks 2-3). In-scope locals at the edit sites: `mode: String`, `create: TerminalCreate` (fields `request_id`, `restore: Option<bool>`, `resume_session_id: Option<String>`, `session_ref: Option<SessionLocator>`), `resolved_cwd: Option<String>` (terminal.rs:963-967), `terminal_id: String` (in scope by the mcp_injection block at :1115), `launch_intent`/`resume_session_id` mut bindings (:973-974), `send_create_error` (:1626).
- Produces: `terminal.created.session_ref = {amplifier, <uuid>}` for fresh amplifier creates (via the untouched existing plumbing: `set_meta` :1331, identity upsert :1377-1397, created frame :1399-1410); a stub on disk before spawn; `amplifier_stub: Option<EnsuredSession>` local consumed by Task 11's GC.

- [ ] **Step 1: Write the failing integration test** — create `crates/freshell-ws/tests/amplifier_session_identity.rs`. Private-harness style (one `multi_thread` test fn, because `AMPLIFIER_CMD` env is process-global — the pattern of `codex_session_ref_resume.rs`), but reusing the shared harness for server/WS plumbing:

```rust
//! Launcher-assigned amplifier session identity — wire + disk + argv proof.
//!
//! ONE test fn (env vars are process-global; mirrors
//! `codex_session_ref_resume.rs`'s phase discipline). Fake `amplifier` is a
//! recording sh script installed via AMPLIFIER_CMD; AMPLIFIER_HOME is the
//! shared isolated harness home.

mod common;
use common::*;

use futures_util::SinkExt;
use serde_json::json;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

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

    let (ws_url, registry) = spawn_server().await;
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
    std::env::remove_var("AMPLIFIER_ARGV_CAPTURE_PATH");
    std::env::remove_var("AMPLIFIER_CMD");
}
```

Note: `session_id` / `dir` stay in scope — Tasks 10 and 11 append further phases to this same test fn.

- [ ] **Step 2: Run to verify it fails for the RIGHT reason**

Run: `cargo test -p freshell-ws --test amplifier_session_identity`
Expected: FAIL at the `sessionRef` assertion ("fresh amplifier terminal.created must carry sessionRef") — today a fresh amplifier create carries no identity.

- [ ] **Step 3: Add the preallocation branch** — in `crates/freshell-ws/src/terminal.rs`, inside the spawn-time block (anchor: `let should_preallocate_fresh_claude` at :977). Add a sibling predicate directly after it (after :984) and a new arm between the `if` arm's closing `}` (:990) and the `} else {` (:991):

```rust
        // Launcher-assigned amplifier identity (plan §1), the fresh-claude
        // preallocation's sibling: a FRESH amplifier pane gets a
        // server-minted session id, and (below, once `terminal_id` exists)
        // a pre-created stub dir — `amplifier resume <uuid>` of that stub
        // IS the fresh launch. CRITICAL: `launch_intent` STAYS `Resume` —
        // amplifier's manifest has resumeArgs only; `Start` without
        // createSessionArgs is a hard StartIntentUnsupported error
        // (cli_launch.rs:431-445; pinned by golden G-A4).
        let should_preallocate_fresh_amplifier = mode == "amplifier"
            && create.restore != Some(true)
            && create.session_ref.is_none()
            && create
                .resume_session_id
                .as_deref()
                .filter(|s| !s.is_empty())
                .is_none();
```

and the arm:

```rust
        } else if should_preallocate_fresh_amplifier {
            resume_session_id = Some(Uuid::new_v4().to_string());
        } else {
```

- [ ] **Step 4: Pre-create the stub before spawn** — insert AFTER the `let cli = match resolve_coding_cli_command(...)` block (anchor: its closing `};` at ~:1164, immediately before the `build_terminal_base_env` comment at :1166). At this point `terminal_id`, `resolved_cwd`, and `resume_session_id` are all in scope, and no further early-return sits between here and `registry.create` except the spawn-failure branch (which Task 11 teaches to GC):

```rust
    // Amplifier pre-create (plan §1/§3/§5): make `amplifier resume <id>`
    // guaranteed-resumable BEFORE spawn. Fresh creates get a brand-new stub;
    // requested resumes whose dir is gone (e.g. a GC'd never-used stub from
    // a previous run) are re-stubbed under the SAME id so restore keeps
    // working; existing sessions are found and left untouched.
    // HARD INVARIANT (plan §5): amplifier only searches the CURRENT cwd's
    // project slug, so the stub is keyed to the exact cwd this PTY spawns
    // with (`resolved_cwd`, resolved above at the top of this handler).
    let mut amplifier_stub: Option<freshell_sessions::amplifier_stub::EnsuredSession> = None;
    if mode == "amplifier" {
        if let Some(session_id) = resume_session_id.as_deref() {
            let Some(stub_cwd) = resolved_cwd.as_deref() else {
                return send_create_error(
                    ws_tx,
                    ErrorCode::PtySpawnFailed,
                    "Amplifier requires a resolvable working directory (cwd is part of the session identity contract).".to_string(),
                    &create.request_id,
                )
                .await;
            };
            let ensured = freshell_sessions::amplifier_stub::resolve_amplifier_home()
                .ok_or_else(|| "amplifier home unresolvable (no AMPLIFIER_HOME and no HOME)".to_string())
                .and_then(|amp_home| {
                    freshell_sessions::amplifier_stub::ensure_session(
                        &amp_home,
                        session_id,
                        stub_cwd,
                        &terminal_id,
                    )
                    .map_err(|e| e.to_string())
                });
            match ensured {
                Ok(ensured) => amplifier_stub = Some(ensured),
                Err(detail) => {
                    // Fail LOUD: spawning `amplifier resume <id>` without a
                    // resumable dir would hang a doomed CLI (the exact
                    // failure mode this feature deletes).
                    return send_create_error(
                        ws_tx,
                        ErrorCode::PtySpawnFailed,
                        format!("Failed to pre-create amplifier session {session_id}: {detail}"),
                        &create.request_id,
                    )
                    .await;
                }
            }
        }
    }
    let _ = &amplifier_stub; // consumed by the exit-hook GC (Task 11)
```

(`freshell-ws` already depends on `freshell-sessions`; use the fully-qualified paths shown — no new `use` needed. Remove the `let _ = &amplifier_stub;` placeholder line in Task 11 when the GC consumes it.)

- [ ] **Step 5: Run the integration test to verify Phase 1 passes**

Run: `cargo test -p freshell-ws --test amplifier_session_identity`
Expected: PASS.

- [ ] **Step 6: Run the neighboring suites (regression sweep)**

Run: `cargo test -p freshell-ws`
Expected: all PASS. Watch specifically `session_identity_frames.rs` (its amplifier resume test now also ensure-stubs into the isolated home — behavior compatible) and the in-file `terminal.rs` unit tests.

- [ ] **Step 7: Commit**

```bash
git add crates/freshell-ws/src/terminal.rs crates/freshell-ws/tests/amplifier_session_identity.rs
git commit -m "feat(ws): launcher-assigned amplifier session identity — preallocate uuid + pre-create stub before spawn"
```

---

### Task 10: WS create path — `terminal:` reject + same-id double-resume guard

**Files:**
- Modify: `crates/freshell-ws/src/terminal.rs`
- Modify: `crates/freshell-ws/tests/amplifier_session_identity.rs`

**Interfaces:**
- Consumes: `freshell_terminal::registry::has_live_resume` (Task 7); `resume_session_id` derived by the block Task 9 touched.
- Produces: error frames (`ErrorCode::PtySpawnFailed`, the handler's uniform reject code — `InvalidCreateRequest`/`InvalidSessionId` are declared but never constructed anywhere in `crates/`, so `PtySpawnFailed` keeps the reject native).

- [ ] **Step 1: Extend the integration test with failing phases** — append to the single test fn (before the final `remove_var` teardown lines; move those to the very end):

```rust
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
```

- [ ] **Step 2: Run to verify the new phases fail**

Run: `cargo test -p freshell-ws --test amplifier_session_identity`
Expected: FAIL — Phase 2 gets a `terminal.created` (or a hung doomed create) instead of an error; Phase 3's duplicate create succeeds.

- [ ] **Step 3: Implement both checks** — in `handle_create`, immediately AFTER the spawn-time resume-id block's closing brace (anchor: the `}` that closes `if mode != "shell" {` at ~:1008, right before whatever statement follows it):

```rust
    // Amplifier identity hardening (plan §10/§11) — evaluated on the FINAL
    // derived resume id, so both `sessionRef` and legacy `resumeSessionId`
    // carriers are covered.
    if mode == "amplifier" {
        if resume_session_id
            .as_deref()
            .is_some_and(|s| s.starts_with("terminal:"))
        {
            // Defense-in-depth against the old correlation bug's poisoned
            // persisted tab state: `terminal:<id>` is Freshell's own
            // synthetic sidebar placeholder, never a resumable amplifier
            // session — a resume of it hangs forever.
            let poisoned = resume_session_id.clone().unwrap_or_default();
            return send_create_error(
                ws_tx,
                ErrorCode::PtySpawnFailed,
                format!(
                    "Invalid amplifier sessionRef '{poisoned}': synthetic terminal placeholder ids are not resumable sessions."
                ),
                &create.request_id,
            )
            .await;
        }
        if let Some(requested) = resume_session_id.as_deref() {
            // Same-id double-resume guard: amplifier has no upstream
            // concurrency guard — never spawn two live PTYs resuming one
            // session id. (Preallocated fresh UUIDs never collide.)
            if freshell_terminal::registry::has_live_resume(
                &state.registry.identity_probe_rows(),
                "amplifier",
                requested,
            ) {
                return send_create_error(
                    ws_tx,
                    ErrorCode::PtySpawnFailed,
                    format!("Amplifier session {requested} is already open in a live terminal."),
                    &create.request_id,
                )
                .await;
            }
        }
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p freshell-ws --test amplifier_session_identity` then `cargo test -p freshell-ws`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/freshell-ws/src/terminal.rs crates/freshell-ws/tests/amplifier_session_identity.rs
git commit -m "feat(ws): reject terminal:-poisoned amplifier refs and guard same-id double resume"
```

---

### Task 11: WS create path — stub GC on exit + spawn-failure cleanup + ensure-after-GC

**Files:**
- Modify: `crates/freshell-ws/src/terminal.rs`
- Modify: `crates/freshell-ws/tests/amplifier_session_identity.rs`

**Interfaces:**
- Consumes: `amplifier_stub: Option<EnsuredSession>` (Task 9), `freshell_sessions::amplifier_stub::gc_stub_if_unused` (Task 4).
- Produces: exit-hook GC of broker-created never-used stubs; spawn-failure stub cleanup.

- [ ] **Step 1: Extend the integration test with failing phases** (append before the env teardown):

```rust
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p freshell-ws --test amplifier_session_identity`
Expected: FAIL at Phase 4 (dir never deleted — no GC exists yet). Phases 5-6 should already pass mechanically once 4 does (ensure-on-resume landed in Task 9), but they pin the tolerances.

- [ ] **Step 3: Implement the GC** — in `handle_create`:

(a) Directly before the `let on_exit: Option<freshell_terminal::pty::ExitHook> = {` block (anchor :1223), replace Task 9's `let _ = &amplifier_stub;` placeholder with:

```rust
    // Amplifier stub GC (plan §8): on exit, delete OUR never-used stub so a
    // never-typed-in terminal doesn't become a permanent '0 msgs' row in the
    // user's `amplifier session list`. Only dirs THIS create wrote
    // (`created == true`), and only while still carrying the never-used
    // signature (no turn_count + empty transcript; events.jsonl tolerated).
    let amplifier_stub_dir_for_exit = amplifier_stub
        .as_ref()
        .filter(|s| s.created)
        .map(|s| s.session_dir.clone());
```

(b) Inside the exit-hook closure, after `identity.retire(&tid);` (anchor :1250):

```rust
            if let Some(dir) = &amplifier_stub_dir_for_exit {
                if freshell_sessions::amplifier_stub::gc_stub_if_unused(dir) {
                    tracing::debug!(
                        terminal_id = %tid,
                        dir = %dir.display(),
                        "amplifier_stub_gc: removed never-used pre-created session"
                    );
                }
            }
```

(c) In the `registry.create` FAILURE branch (anchor: `cleanup_mcp_config(&RealMcpRuntime, &terminal_id, ...)` inside the `if let Err(err) = state.registry.create(...)` block at ~:1284), add right after that cleanup call:

```rust
        // A stub written for a spawn that never happened is pure litter.
        if let Some(stub) = amplifier_stub.as_ref().filter(|s| s.created) {
            let _ = freshell_sessions::amplifier_stub::gc_stub_if_unused(&stub.session_dir);
        }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p freshell-ws --test amplifier_session_identity` then `cargo test -p freshell-ws`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/freshell-ws/src/terminal.rs crates/freshell-ws/tests/amplifier_session_identity.rs
git commit -m "feat(ws): GC never-used amplifier stubs on exit and spawn failure; pin ensure-after-GC restore"
```

---

### Task 12: REST create path (`POST /api/tabs` + splits) shares the pre-create

`spawn_terminal_pane` covers both REST tab creates AND `pane_ops::split_pane`, so one insertion covers both. This crate cannot reach `freshell-ws` (circular dep) — sharing happens via the SAME `freshell-sessions::amplifier_stub` + `freshell-terminal::registry::has_live_resume` helpers the WS path uses.

**Files:**
- Modify: `crates/freshell-freshagent/src/terminal_tabs.rs`

**Interfaces:**
- Consumes: `ensure_session`, `resolve_amplifier_home`, `gc_stub_if_unused` (Tasks 2-4), `has_live_resume` (Task 7). In-scope locals in `spawn_terminal_pane`: `mode: String` (:580), `cwd: Option<String>` (:608, `is_dir()`-validated), `(mut resume_session_id, accepted_session_ref)` from `derive_resume_identity` (:623), `terminal_id` minted at :627, `registry` (:599), `fail_json(StatusCode, String) -> Response` (this file's uniform reject), exit hook at :841-863, `registry.create` at :866, `set_meta` at :926, `pane_content` at :947.
- Produces: REST-created amplifier panes carry a pre-created identity; `pane_content.sessionRef`/`resumeSessionId` populated; stub GC on REST-pane exit.

- [ ] **Step 1: Write the failing tests** — in `terminal_tabs.rs`'s `#[cfg(test)]` module, add (reuse the module's existing `state_with_registry()` + CLI-spec helpers exactly as the locator tests at :2225-2307 do today — copy their spec-registration shape; those old tests are deleted in Task 13, these replace them):

```rust
    fn isolated_amplifier_home() -> std::path::PathBuf {
        use std::sync::OnceLock;
        static HOME: OnceLock<std::path::PathBuf> = OnceLock::new();
        HOME.get_or_init(|| {
            let dir = std::env::temp_dir().join(format!(
                "freshell-freshagent-test-amplifier-home-{}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            std::env::set_var("AMPLIFIER_HOME", &dir);
            dir
        })
        .clone()
    }

    fn find_session_dir(home: &std::path::Path, session_id: &str) -> Option<std::path::PathBuf> {
        for entry in std::fs::read_dir(home.join("projects")).ok()?.flatten() {
            let candidate = entry.path().join("sessions").join(session_id);
            if candidate.is_dir() {
                return Some(candidate);
            }
        }
        None
    }

    #[tokio::test]
    async fn rest_amplifier_create_preallocates_identity_and_stub() {
        let home = isolated_amplifier_home();
        let cwd = std::env::temp_dir().join(format!("fa-amp-cwd-{}", std::process::id()));
        std::fs::create_dir_all(&cwd).unwrap();
        let state = state_with_registry()
            .with_cli_commands(/* same sleeper/recording amplifier spec shape the
                deleted locator tests used — a CliCommandSpec named "amplifier"
                whose default_cmd is an executable sleep script */);
        let result = spawn_terminal_pane(
            &state,
            &serde_json::json!({ "mode": "amplifier", "cwd": cwd.to_string_lossy() }),
            "tab-1",
            "pane-1",
        )
        .await
        .expect("spawn ok");

        let sid = result.pane_content["sessionRef"]["sessionId"]
            .as_str()
            .expect("REST pane_content must carry the preallocated sessionRef")
            .to_string();
        assert_eq!(result.pane_content["sessionRef"]["provider"], "amplifier");
        assert_eq!(sid.len(), 36, "server-minted uuid: {sid}");
        assert_eq!(result.pane_content["resumeSessionId"], serde_json::json!(sid));
        assert!(find_session_dir(&home, &sid).is_some(), "stub on disk");
        // Registry meta records it (identity_probe_rows is the shared truth).
        let registry = state.terminal_registry.clone().unwrap();
        let row = registry
            .identity_probe_rows()
            .into_iter()
            .find(|r| r.resume_session_id.as_deref() == Some(sid.as_str()))
            .expect("registry row carries the resume id");
        assert_eq!(row.mode, "amplifier");
        registry.kill(&row.terminal_id);
    }

    #[tokio::test]
    async fn rest_amplifier_create_rejects_poisoned_and_duplicate_ids() {
        let home = isolated_amplifier_home();
        let cwd = std::env::temp_dir().join(format!("fa-amp-cwd2-{}", std::process::id()));
        std::fs::create_dir_all(&cwd).unwrap();
        let state = state_with_registry().with_cli_commands(/* as above */);

        // terminal:-poisoned ref → 400.
        let err = spawn_terminal_pane(
            &state,
            &serde_json::json!({
                "mode": "amplifier",
                "cwd": cwd.to_string_lossy(),
                "sessionRef": { "provider": "amplifier", "sessionId": "terminal:deadbeef" },
            }),
            "tab-2",
            "pane-2",
        )
        .await
        .expect_err("poisoned ref must be rejected");
        drop(err);

        // Double resume of one id → second create rejected.
        let sid = "12121212-3434-5656-7878-909090909090";
        let ok = spawn_terminal_pane(
            &state,
            &serde_json::json!({
                "mode": "amplifier",
                "cwd": cwd.to_string_lossy(),
                "sessionRef": { "provider": "amplifier", "sessionId": sid },
            }),
            "tab-3",
            "pane-3",
        )
        .await
        .expect("first resume spawns");
        assert!(find_session_dir(&home, sid).is_some(), "ensure-stub for requested resume");
        let dup = spawn_terminal_pane(
            &state,
            &serde_json::json!({
                "mode": "amplifier",
                "cwd": cwd.to_string_lossy(),
                "sessionRef": { "provider": "amplifier", "sessionId": sid },
            }),
            "tab-4",
            "pane-4",
        )
        .await;
        assert!(dup.is_err(), "same-id double resume must be rejected");
        let registry = state.terminal_registry.clone().unwrap();
        registry.kill(&ok.pane_content["terminalId"].as_str().unwrap().to_string());
    }
```

(Fill the `with_cli_commands(...)` argument from the spec-construction helper visible in this file's existing test module — the deleted locator tests at :2241/:2281 show the exact shape; keep it verbatim minus the locator.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p freshell-freshagent rest_amplifier`
Expected: FAIL — no sessionRef in pane_content, no stub, no rejects.

- [ ] **Step 3: Implement** — in `spawn_terminal_pane`, insert directly after `let stream_id = Uuid::new_v4().to_string();` (:628):

```rust
    // Launcher-assigned amplifier identity, REST half (plan §2): the SAME
    // freshell-sessions/freshell-terminal helpers the WS path uses — this
    // crate cannot reach freshell-ws (circular dep), so the shared logic
    // lives below both. Covers POST /api/tabs creates AND pane splits.
    let mut amplifier_stub: Option<freshell_sessions::amplifier_stub::EnsuredSession> = None;
    if mode == "amplifier" {
        if resume_session_id
            .as_deref()
            .is_some_and(|s| s.starts_with("terminal:"))
        {
            return Err(fail_json(
                StatusCode::BAD_REQUEST,
                format!(
                    "Invalid amplifier sessionRef '{}': synthetic terminal placeholder ids are not resumable sessions.",
                    resume_session_id.as_deref().unwrap_or_default()
                ),
            ));
        }
        let is_restore = body.get("restore").and_then(Value::as_bool) == Some(true);
        if resume_session_id.as_deref().filter(|s| !s.is_empty()).is_none() && !is_restore {
            resume_session_id = Some(Uuid::new_v4().to_string());
        }
        if let Some(session_id) = resume_session_id.as_deref() {
            if freshell_terminal::registry::has_live_resume(
                &registry.identity_probe_rows(),
                "amplifier",
                session_id,
            ) {
                return Err(fail_json(
                    StatusCode::CONFLICT,
                    format!("Amplifier session {session_id} is already open in a live terminal."),
                ));
            }
            // Same cwd fallback chain the WS path's resolve_create_cwd ends
            // with (explicit cwd, else $HOME) — the stub's slug MUST match
            // the cwd the PTY spawns with (plan §5 hard invariant).
            let effective_cwd = cwd
                .clone()
                .or_else(|| std::env::var("HOME").ok().filter(|v| !v.is_empty()));
            let Some(effective_cwd) = effective_cwd else {
                return Err(fail_json(
                    StatusCode::BAD_REQUEST,
                    "Amplifier requires a resolvable working directory (cwd is part of the session identity contract).".to_string(),
                ));
            };
            match freshell_sessions::amplifier_stub::resolve_amplifier_home()
                .ok_or_else(|| "amplifier home unresolvable (no AMPLIFIER_HOME and no HOME)".to_string())
                .and_then(|amp_home| {
                    freshell_sessions::amplifier_stub::ensure_session(
                        &amp_home,
                        session_id,
                        &effective_cwd,
                        &terminal_id,
                    )
                    .map_err(|e| e.to_string())
                }) {
                Ok(e) => amplifier_stub = Some(e),
                Err(detail) => {
                    return Err(fail_json(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to pre-create amplifier session {session_id}: {detail}"),
                    ));
                }
            }
        }
    }
```

- [ ] **Step 4: GC in the REST exit hook** — in the `on_exit` closure block (:841-863): before `Some(Box::new(move |exit_code: i64| {`, add

```rust
        // Amplifier stub GC (plan §8) — REST twin of the WS exit-hook GC.
        let amplifier_stub_dir_for_exit = amplifier_stub
            .as_ref()
            .filter(|s| s.created)
            .map(|s| s.session_dir.clone());
```

and inside the closure, after `.notify_terminal_exit(&tid);`:

```rust
            if let Some(dir) = &amplifier_stub_dir_for_exit {
                let _ = freshell_sessions::amplifier_stub::gc_stub_if_unused(dir);
            }
```

Also, in the `registry.create` failure branch of this fn (:866-877 region — find its `Err` arm), add the same spawn-failure cleanup as the WS path:

```rust
        if let Some(stub) = amplifier_stub.as_ref().filter(|s| s.created) {
            let _ = freshell_sessions::amplifier_stub::gc_stub_if_unused(&stub.session_dir);
        }
```

- [ ] **Step 5: Promote the identity into `pane_content`** — after the `pane_content` json is built (:947-954), next to the existing `codexDurability` promotion (:960-962), add:

```rust
    // Launcher-assigned amplifier identity: the frozen client folds
    // paneContent verbatim, so the preallocated identity must ride here for
    // the sidebar/restore join (the WS path's terminal.created.sessionRef
    // twin). Requested resumes already carry these via the body promotion.
    if mode == "amplifier" {
        if let Some(sid) = resume_session_id.as_deref() {
            pane_content["sessionRef"] =
                json!({ "provider": "amplifier", "sessionId": sid });
            pane_content["resumeSessionId"] = json!(sid);
        }
    }
```

(If this file already promotes `accepted_session_ref`/`resumeSessionId` into `pane_content` further down, extend that site instead of duplicating — one promotion, amplifier included.)

- [ ] **Step 6: Run tests**

Run: `cargo test -p freshell-freshagent`
Expected: the two new tests PASS; the OLD locator tests (`arm_locators...`, `send_keys_enter_feeds_amplifier_locator_and_tick_locates_session`, :2225-2307/:2735-2793) may now FAIL because fresh amplifier panes carry a resume id and `locator.arm` skips resuming panes — if so, this is the planned obsolescence: `#[ignore = "replaced by rest_amplifier_* tests; deleted with the locator in the deletion task"]` them here and delete them in Task 13.

- [ ] **Step 7: Commit**

```bash
git add crates/freshell-freshagent/src/terminal_tabs.rs
git commit -m "feat(freshagent): REST/split amplifier creates share launcher-assigned identity pre-create"
```

---

### Task 13: Delete the correlation path

Everything amplifier-locator dies; opencode's sibling stays untouched (one compile-forced test-field line excepted). The invariant alarm is re-homed first so it never goes dark.

**Files:**
- Modify: `crates/freshell-ws/src/invariants.rs`, `crates/freshell-ws/src/lib.rs`, `crates/freshell-ws/src/terminal.rs`, `crates/freshell-server/src/main.rs`, `crates/freshell-freshagent/src/lib.rs`, `crates/freshell-freshagent/src/terminal_tabs.rs`, `crates/freshell-ws/src/opencode_association.rs` (one line), `crates/freshell-sessions/src/lib.rs`
- Modify (one line each — remove `amplifier_locator: None,`): `crates/freshell-ws/tests/codex_session_ref_resume.rs:144`, `max_payload.rs:89`, `safe08_restore_diagnostics.rs:175`, `origin_policy.rs:79`, `hello_timeout.rs:88`, `common/mod.rs:124`, `pane_reconcile.rs:103`, `freshagent_claude_kill_interrupt.rs:203`, `keepalive.rs:89`, `term09_output_queue.rs:82`, `codex_managed_launch_e2e.rs:151`, `diag01_lifecycle_events.rs:168`
- Delete: `crates/freshell-sessions/src/amplifier_locator.rs`, `crates/freshell-ws/src/amplifier_association.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `pub mod invariants;` (was `pub(crate)`) exporting `pub fn spawn_identity_invariant_sweep(state: WsState, interval: std::time::Duration)` — called by `main.rs`.

- [ ] **Step 1 (re-home the alarm FIRST, red):** in `crates/freshell-ws/src/invariants.rs`, replace the derived const (:33-40) with a literal and add the sweep spawner:

```rust
/// How long after terminal creation an unresolved identity becomes
/// alarm-worthy. Amplifier identity is now launcher-assigned at create time
/// (pre-create + resume), so any RUNNING non-shell terminal without a
/// resolvable identity after a generous 10s is a genuine bug, not
/// association latency. (Historically 5 × the deleted amplifier locator's
/// 2s dir-appear correlation window — same value, now a literal.)
pub(crate) const IDENTITY_RESOLUTION_GRACE_MS: i64 = 10_000;
```

and at the end of the module's production code (before `#[cfg(test)]`):

```rust
/// STATE-SYNC FIX 1 increment 2b, re-homed: the identity invariant alarm
/// previously rode the (deleted) amplifier locator sweep — it now runs on
/// its own timer, spawned unconditionally from `freshell-server::main`, so
/// it also observes modes that never had a locator (gemini/kimi).
pub fn spawn_identity_invariant_sweep(state: crate::WsState, interval: std::time::Duration) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        let mut identity_warned = std::collections::HashSet::new();
        loop {
            ticker.tick().await;
            warn_unresolved_terminal_identities(
                &state.registry.identity_probe_rows(),
                &state.identity,
                &mut identity_warned,
                crate::terminal::now_ms(),
            );
        }
    });
}
```

In `crates/freshell-ws/src/lib.rs:28` change `pub(crate) mod invariants;` → `pub mod invariants;`. Update the doc comment on the test at invariants.rs:187 region only if it names the locator (the test body and the module's four tests stay as-is — the alarm's behavior is unchanged).

- [ ] **Step 2 (the deletion sweep):** apply ALL of the following in one pass — the crate won't compile until every site is done:

1. `git rm crates/freshell-sessions/src/amplifier_locator.rs`; remove `pub mod amplifier_locator;` from `crates/freshell-sessions/src/lib.rs:18` (no re-exports exist).
2. `git rm crates/freshell-ws/src/amplifier_association.rs`; remove `pub mod amplifier_association;` from `crates/freshell-ws/src/lib.rs:24`.
3. `crates/freshell-ws/src/lib.rs`: delete the `amplifier_locator` field + its doc block (:203-211) and the test-ctor line (:710).
4. `crates/freshell-ws/src/terminal.rs`: delete the `amplifier_association::note_possible_submit` call + its comment in the `TerminalInput` arm (:485-493 — keep the opencode sibling); delete the exit-hook's `let amplifier_locator = state.amplifier_locator.clone();` + comment (:1233-1237 region) and the `if let Some(locator) = &amplifier_locator { locator.disarm(&tid); }` block; delete the `crate::amplifier_association::maybe_arm(...)` call + comment (:1339-1348 — keep `opencode_association::maybe_arm`); delete `amplifier_locator: None,` at :2591 and :2795; reword the doc mention at :83.
5. `crates/freshell-ws/src/opencode_association.rs:274`: delete the single `amplifier_locator: None,` test-struct line (compile-forced; nothing else in the file changes).
6. `crates/freshell-server/src/main.rs`: delete the locator construction (:317-329) and `.with_amplifier_locator(amplifier_locator.clone())` (:352, keep `.with_opencode_locator(...)`); delete `amplifier_locator: amplifier_locator.clone(),` from the `WsState` literal (:412); delete the amplifier sweep spawn (:526-537); rename `AMPLIFIER_LOCATOR_SWEEP_INTERVAL` → `LOCATOR_SWEEP_INTERVAL` (const :1112 + the opencode usage :545) with doc updated to name the opencode locator + identity sweeps; reword the activity-hub resolver comment (:392-396) — with pre-created stubs every amplifier create carries a resume id and an events.jsonl, so the resolver now covers fresh AND resumed creates (no locator hand-off); and where the amplifier sweep used to be spawned, add:

```rust
    // The identity invariant alarm (terminal_identity_unresolved) previously
    // rode the amplifier locator sweep; amplifier identity is now assigned
    // at create time, so the alarm runs on its own timer — unconditionally,
    // covering every session-provider mode.
    freshell_ws::invariants::spawn_identity_invariant_sweep(
        ws_state.clone(),
        LOCATOR_SWEEP_INTERVAL,
    );
```

7. `crates/freshell-freshagent/src/lib.rs`: delete the `amplifier_locator` field (:167-168) + doc (:155-157), the `new()` init (:235), and `with_amplifier_locator` (:330-345); fix the sibling doc refs (:169, :346) to stop naming it.
8. `crates/freshell-freshagent/src/terminal_tabs.rs`: in `arm_locators_for_fresh_pane` (:448-471) delete the amplifier half + rename nothing (fn keeps arming opencode; update its doc comment); exit hook (:841-863): delete `let amplifier_locator = state.amplifier_locator.clone();` and its disarm block (keep opencode); REST send-keys (:1264-1291): delete the amplifier `note_submit` half (keep opencode; trim the comment); delete the locator test helpers/tests (`state_with_amplifier_locator` :2225-2232 and the tests at :2241-2307, :2735-2793 — including any `#[ignore]`d in Task 12).
9. Remove the 12 one-line `amplifier_locator: None,` initializers in `crates/freshell-ws/tests/` (list in Files above).
10. `crates/freshell-freshagent/Cargo.toml:45-54`: reword the comment (dep on freshell-sessions is still needed — opencode locator AND the new amplifier_stub).
11. Doc-comment mentions that name `amplifier_locator`/`amplifier_association` in files being kept: reword `invariants.rs:35` (done in Step 1), `activity.rs:142-145` (`attach_amplifier_association`'s doc — it keeps its test-only caller; note the create-time resolver is now the production path), `freshagent/lib.rs:333`. Leave `opencode_locator.rs`'s prose references untouched (out-of-scope file; `cargo doc` is not in CI — note the broken intra-doc links at `opencode_locator.rs:2,56` in the commit message as accepted).

- [ ] **Step 3: Compile + full workspace test run**

Run: `cargo build --workspace && cargo test --workspace`
Expected: green. Deleted with their files: the 15 unit tests inside `amplifier_locator.rs` and the 6 in `amplifier_association.rs` (names for the accounting: `is_submit_input_matches_enter_only_sequences`, `maybe_arm_ignores_non_amplifier_modes`, `maybe_arm_arms_a_fresh_amplifier_terminal`, `maybe_arm_skips_a_resuming_amplifier_terminal`, `note_possible_submit_ignores_non_enter_input`, `drain_and_associate_binds_identity_and_broadcasts_on_location`). The invariants tests (`warns_once_per_unresolved_non_shell_terminal_past_the_grace_window` etc.) must still pass.

- [ ] **Step 4: Grep-gate**

Run: `grep -rn "amplifier_locator\|amplifier_association" crates/ --include=*.rs | grep -v opencode_locator.rs`
Expected: zero hits (opencode_locator.rs's inert prose mentions are the only survivors, excluded above).

- [ ] **Step 5: Commit**

```bash
git add -A crates/
git commit -m "refactor(rust)!: delete amplifier correlation-window locator/association path; re-home identity invariant sweep"
```

---

### Task 14: Boot canary wiring

**Files:**
- Modify: `crates/freshell-server/src/main.rs`

**Interfaces:**
- Consumes: `verify_amplifier_layout_contract`, `resolve_amplifier_home`, `CanaryOutcome` (Task 5).
- Produces: a WARN/ERROR-loud, non-blocking boot self-test.

- [ ] **Step 1: Spawn the canary** — next to the sweep spawns added in Task 13:

```rust
    // Version canary (plan §9): the pre-create path rests on amplifier's
    // undocumented on-disk layout (upstream microsoft/amplifier#315/#316
    // track a --session-id flag that would collapse this layer into a
    // flag). Verify our slug/layout assumptions against sessions amplifier
    // ITSELF wrote — loud on breakage, never blocking broker start.
    tokio::task::spawn_blocking(|| {
        use freshell_sessions::amplifier_stub::{
            resolve_amplifier_home, verify_amplifier_layout_contract, CanaryOutcome,
        };
        let Some(amp_home) = resolve_amplifier_home() else {
            return;
        };
        match verify_amplifier_layout_contract(&amp_home) {
            CanaryOutcome::Broken { detail } => tracing::error!(
                target: "freshell_ws::invariants",
                %detail,
                "amplifier_layout_contract_broken: amplifier's on-disk session layout no \
                 longer matches the broker's stub pre-create assumptions — pre-created \
                 identities may silently diverge from the CLI's own sessions"
            ),
            outcome => tracing::debug!(?outcome, "amplifier layout canary"),
        }
    });
```

- [ ] **Step 2: Build + targeted tests**

Run: `cargo build -p freshell-server && cargo test -p freshell-sessions amplifier_stub`
Expected: builds; the canary's own behavior is already unit-pinned (Task 5). Do NOT restart the self-hosted Freshell server to "try it".

- [ ] **Step 3: Commit**

```bash
git add crates/freshell-server/src/main.rs
git commit -m "feat(server): boot-time amplifier layout canary (loud, non-blocking)"
```

---

### Task 15: E2E rewrite — restore-across-restart on the new mechanism

**Files:**
- Modify: `test/e2e-browser/fixtures/fake-amplifier-cli.mjs`
- Modify: `test/e2e-browser/specs/amplifier-restore-rust.spec.ts`

**Interfaces:**
- Consumes: the spec's existing helpers (`installFakeAmplifierCli`, `bootAndConnect`, `openAmplifierPaneAndGetLeaf`, `findLeafById`, `createE2eServerHandle` with `env`/`setupHome`) — all kept.
- Produces: e2e proof of the same user outcome (restore across restart), simplified: identity at create time, no submit-to-associate, never-used panes still restore.

- [ ] **Step 1: Extend the fake CLI's resume mode** — in `fake-amplifier-cli.mjs`, replace the `if (argv[0] === 'resume') { ... }` branch (currently prints the marker and idles) with:

```js
if (argv[0] === 'resume') {
  const sessionId = argv[1] ?? ''
  process.stdout.write(`amplifier: resumed session ${sessionId}\r\n`)
  // Mirror the real CLI's first-turn save: find the (broker pre-created)
  // session dir under any project slug, stamp turn_count into metadata.json
  // and append one transcript line — the exact "used" signature the
  // broker's stub GC respects (used sessions survive terminal exit).
  let turnRecorded = false
  process.stdin.setEncoding('utf8')
  process.stdin.on('data', () => {
    if (turnRecorded) return
    turnRecorded = true
    const projectsDir = path.join(amplifierHome(), 'projects')
    let slugs = []
    try { slugs = fs.readdirSync(projectsDir) } catch { /* no home yet */ }
    for (const slug of slugs) {
      const dir = path.join(projectsDir, slug, 'sessions', sessionId)
      const metaPath = path.join(dir, 'metadata.json')
      if (!fs.existsSync(metaPath)) continue
      const meta = JSON.parse(fs.readFileSync(metaPath, 'utf8'))
      meta.turn_count = (meta.turn_count ?? 0) + 1
      fs.writeFileSync(metaPath, JSON.stringify(meta))
      fs.appendFileSync(path.join(dir, 'transcript.jsonl'), `${JSON.stringify({ role: 'user', content: 'fake turn' })}\n`)
      process.stdout.write(`amplifier: turn recorded ${sessionId}\r\n`)
      break
    }
  })
  process.stdin.resume()
}
```

(The fresh no-args branch stays — it no longer runs for broker-created amplifier panes, but keeps the fixture self-consistent.)

- [ ] **Step 2: Rewrite the spec's single test** — keep the file's doc header (update its mechanism description), helpers, setup (including `AMPLIFIER_CMD` + `FAKE_AMPLIFIER_ARGV_LOG` env and `setupHome` seeding `.freshell/config.json`), and add `AMPLIFIER_HOME: path.join(sharedRoot, 'amplifier-home')` to the `construct.env` so server and fake CLI agree deterministically. Replace the test body's assertion flow with:

```ts
    // ── Positive pane: identity is launcher-assigned AT CREATE — no submit needed.
    const { paneId: positivePaneId, terminalId } = await openAmplifierPaneAndGetLeaf(page, harness, tabId!)
    const sessionId: string = await expect.poll(async () => {
      const leaf = await findLeafById(tabId!, positivePaneId)
      return leaf?.content?.sessionRef?.sessionId ?? null
    }, { timeout: 15_000 }).not.toBeNull().then(async () => {
      const leaf = await findLeafById(tabId!, positivePaneId)
      return leaf!.content!.sessionRef!.sessionId as string
    })
    // Server-minted UUID (36 chars, 4 hyphens) — NOT a fake-amp-* id minted
    // by the CLI, and present BEFORE any input.
    expect(sessionId).toHaveLength(36)
    expect(sessionId.split('-')).toHaveLength(5)
    const positiveLeaf = await findLeafById(tabId!, positivePaneId)
    expect(positiveLeaf?.content?.sessionRef?.provider).toBe('amplifier')

    // The PTY was spawned as `resume <sessionId>` and the fake CLI adopted it.
    await expect.poll(async () =>
      (await harness.getTerminalBuffer(terminalId)).replace(/\n/g, ''),
    { timeout: 15_000 }).toContain(`amplifier: resumed session ${sessionId}`)

    // Type a turn → the fake CLI stamps the "used" signature.
    await page.locator('.xterm').last().click()
    await page.keyboard.type('hello amplifier')
    await page.keyboard.press('Enter')
    await expect.poll(async () =>
      (await harness.getTerminalBuffer(terminalId)).replace(/\n/g, ''),
    { timeout: 15_000 }).toContain(`amplifier: turn recorded ${sessionId}`)

    // ── Negative pane: never typed in. It ALSO gets create-time identity
    // (the old "no identity until submit" behavior is gone by design).
    const { paneId: negativePaneId } = await openAmplifierPaneAndGetLeaf(page, harness, tabId!)
    const negativeSessionId: string = await expect.poll(async () => {
      const leaf = await findLeafById(tabId!, negativePaneId)
      return leaf?.content?.sessionRef?.sessionId ?? null
    }, { timeout: 15_000 }).not.toBeNull().then(async () => {
      const leaf = await findLeafById(tabId!, negativePaneId)
      return leaf!.content!.sessionRef!.sessionId as string
    })
    expect(negativeSessionId).not.toBe(sessionId)

    // persist flush + restart + reconnect (existing helpers, unchanged)
    ...

    // ── Restore proof, two independent ways, for BOTH panes:
    // (a) used pane resumes the SAME id;
    // (b) never-used pane (stub GC'd at shutdown) ALSO resumes its SAME id —
    //     the broker re-stubs GC'd ids at create (ensure-after-GC), so a
    //     never-typed pane restores instead of hanging.
    for (const [paneId, sid] of [[positivePaneId, sessionId], [negativePaneId, negativeSessionId]] as const) {
      const leaf = await findLeafById(tabId!, paneId)
      const restoredTerminalId = leaf!.content!.terminalId as string
      await expect.poll(async () =>
        (await harness.getTerminalBuffer(restoredTerminalId)).replace(/\n/g, ''),
      { timeout: 20_000 }).toContain(`amplifier: resumed session ${sid}`)
    }
    // argv log: every amplifier spawn in this scenario was a resume, and
    // both ids appear as `resume <id>` invocations post-restart.
    const entries = (await fs.readFile(argLogPath, 'utf8')).trim().split('\n').map((l) => JSON.parse(l))
    const resumes = entries.filter((e) => e.argv[0] === 'resume')
    expect(resumes.some((e) => e.argv[1] === sessionId)).toBe(true)
    expect(resumes.some((e) => e.argv[1] === negativeSessionId)).toBe(true)
    expect(entries.every((e) => e.argv[0] === 'resume')).toBe(true)
```

Keep the existing structure for boot, tab lookup, persist-flush, restart, and the `finally` cleanup exactly as the current file has them (they are mechanism-agnostic); delete the old locator-era assertions (the `/^fake-amp-/` match, the submit-then-associate poll, and the `sessionRef ... toBeUndefined()` negative control).

- [ ] **Step 3: Run the spec**

Run: `npm run test:e2e -- --project=rust-chromium --grep "Amplifier Restore"`
Expected: PASS. (First run before Step 1/2 edits would fail — the old spec asserts submit-time association that no longer exists; that failure is this task's RED.)

- [ ] **Step 4: Commit**

```bash
git add test/e2e-browser/fixtures/fake-amplifier-cli.mjs test/e2e-browser/specs/amplifier-restore-rust.spec.ts
git commit -m "test(e2e): amplifier restore-across-restart on launcher-assigned identity (create-time sessionRef, GC re-stub)"
```

---

### Task 16: Full verification + push

**Files:** none (verification only).

**Interfaces:** none.

- [ ] **Step 1: Full Rust workspace**

Run: `cargo test --workspace`
Expected: green.

- [ ] **Step 2: Coordinated JS/TS suite**

Run: `FRESHELL_TEST_SUMMARY="amplifier-session-identity full gate" npm test`
Expected: green (the real-provider contract tests self-skip; wait for the shared coordinator gate if held).

- [ ] **Step 3: E2E**

Run: `npm run test:e2e -- --project=rust-chromium`
Expected: green.

- [ ] **Step 4: Log check for stragglers**

Run: `grep -rn "AMPLIFIER_LOCATOR_SWEEP_INTERVAL" crates/ ; grep -rn "amplifier_locator" crates/ --include=*.rs | grep -v opencode_locator.rs`
Expected: zero hits for both.

- [ ] **Step 5: Push the branch — and STOP (no PR without explicit user approval)**

```bash
git push -u origin feat/amplifier-session-identity
```

---

## Self-Review

**1. Spec coverage** (spec §Design 1-11 → tasks):
- §1 WS insertion point, keep `Resume` intent → Tasks 6, 9. §2 REST shares pre-create → Task 12. §3 stub writer + slug in freshell-sessions → Tasks 2-3 (new `amplifier_stub` module rather than `amplifier.rs` itself, because that module's contract is "never mutates provider data" — documented in the module doc). §4 stub shape → Task 3 (adds an empty `events.jsonl` beyond the spec's enumerated shape — rationale and real-CLI validation documented in Task 3's design note and covered by Task 1's adoption test). §5 cwd invariant → Tasks 3, 9 (canonicalize + slug of the exact spawn cwd; hard-asserted in Task 9 Phase 1). §6 identity broadcast zero-client-changes → existing plumbing, proven by Task 9 (terminal.created.sessionRef) and Task 12 (pane_content); events-lane attach at create rides the existing resolver (Task 3 note, comment updates in Task 13 step 2.6). §7 deletion incl. const rename, invariants alarm kept with inlined grace → Task 13. §8 GC → Tasks 4, 11, 12 (+ ensure-after-GC so never-used restored panes don't spawn doomed resumes — Task 11 Phase 6, Task 15 negative pane). §9 canary → Tasks 5, 14 (layout-assumption verification variant — spec explicitly allows "or verify layout assumptions"; lightweight, non-blocking). §10 `terminal:` reject → Tasks 10, 12. §11 double-resume guard → Tasks 7, 10, 12. Test plan tiers 1-5 → Tasks 1, 2-7, 8-12, 15, 13(step 3 accounting). Out-of-scope respected: zero Node-server edits; opencode files untouched except the compile-forced one-line test field (called out in Global Constraints).

**1b. No silent deferrals:** the production outcome — fresh amplifier terminal has a real resumable identity before first keystroke and restores across restart — is proven at three levels with no stub standing in for behavior: real CLI adoption + slug (Task 1, opt-in but runnable), real broker + fake CLI argv/disk/wire (Tasks 9-12), full user flow through the browser (Task 15). The fake CLI in WS/e2e tests substitutes only the *amplifier binary*, whose real behavior is separately pinned by Task 1 against the same stub bytes. No TODOs, no deferred requirements. One consciously-accepted non-requirement gap: broker crash before terminal exit can leak a never-used stub (no exit hook runs); the spec's GC requirement is "on terminal close/exit", which is implemented, and ensure-after-GC makes leaks harmless to restore.

**2. Placeholder scan:** two intentional adaptation points remain and are explicitly bounded, not open-ended: Task 12 Step 1's `with_cli_commands(/* ... */)` points at the exact in-file helper (the deleted tests at :2241/:2281) to copy verbatim, and Task 15 keeps named existing helpers unchanged. Every other code step is complete.

**3. Type consistency:** `cwd_slug(&str) -> String`, `canonical_cwd(&str) -> PathBuf`, `resolve_amplifier_home() -> Option<PathBuf>`, `ensure_session(&Path, &str, &str, &str) -> io::Result<EnsuredSession>`, `EnsuredSession { session_dir: PathBuf, created: bool }`, `stub_is_unused(&Path) -> bool`, `gc_stub_if_unused(&Path) -> bool`, `verify_amplifier_layout_contract(&Path) -> CanaryOutcome`, `has_live_resume(&[IdentityProbeRow], &str, &str) -> bool`, `spawn_identity_invariant_sweep(WsState, Duration)` — used with these exact signatures in Tasks 9-14. Error paths uniformly use `send_create_error(..., ErrorCode::PtySpawnFailed, ...)` (WS) and `fail_json(StatusCode, String)` (REST), matching each handler's native reject convention.
