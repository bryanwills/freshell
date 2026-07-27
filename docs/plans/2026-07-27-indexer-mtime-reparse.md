# Session-Indexer: Sidecar Activity Mtime Without Re-Parse (kata v4rw) Implementation Plan

> **For agentic workers:** This plan is executed task-by-task by the
> workflow's execute stage: a fresh implementer per task, with a spec +
> quality review after each task. Steps use checkbox (`- [ ]`) syntax
> for tracking.

**Goal:** Stop the coding-CLI session indexer from forcing a full re-parse of a session every refresh when only the Amplifier activity sidecar (`events.jsonl`/`transcript.jsonl`) mtime moved — fold the mtime into `lastActivityAt` on the cache-hit path instead (semantic no-op, kata issue v4rw, P1 oom/perf).

**Architecture:** Single-file behavioral change in `server/coding-cli/session-indexer.ts`. The re-parse gate in `updateCacheEntry` (lines 899-908) currently requires `cached.activityMtimeMs === activityMtimeMs` for a cache hit; a live Amplifier session's `events.jsonl` mtime moves on every 5s refresh, so line 904 fails every time and the session re-parses from byte 0 forever. The only consumer of `activityMtimeMs` besides the gate is the recency fold at line 977 (`lastActivityAt = Math.floor(maxDefined(lastActivityAt, activityMtimeMs) ?? 0)`). The fix drops the `activityMtimeMs` term from the gate and, on the cache-hit branch, folds the fresh sidecar mtime into `cached.baseSession.lastActivityAt` in place (max-monotonic, floored). `buildProjectGroups` (line 1117) reads `cached.baseSession` straight out of `fileCache` on every refresh, so the in-place fold is picked up with no extra plumbing.

**Tech Stack:** TypeScript (NodeNext/ESM), Vitest (server config), node:fs/promises temp-dir fixtures, existing `makeProvider` fake-provider factory in the test file.

## Why this is safe (survey evidence, from the kata issue)

- `events.jsonl` is write-only observability — Amplifier never reads it back. Its mtime moving means ONLY "session had activity".
- `transcript.jsonl` content is never an input to the parse either: the indexer parses `metadata.json` (the primary session file), whose own `mtimeMs`+`size` gate terms remain untouched. Amplifier's `parseSessionFile` reads at most a 64 KiB prefix of `transcript.jsonl` for enrichment — and a content change there without a `metadata.json` change carries no parse-relevant data (verified: `messageCount` comes from metadata's `turn_count`; title/summary/lastActivityAt come from metadata JSON).
- Per-session `events.jsonl` reaches 75 MB–1.1 GB in production while `transcript.jsonl` stays ~200-900 KB, and the forced re-parse was firing for EVERY live session on EVERY refresh (5s throttle floor).
- Only the `amplifier` provider implements `getActivityMtimeMs` — claude/codex/opencode leave it `undefined` (`undefined === undefined` already passed the gate), so this fix is amplifier-only in effect though it lives in shared code.
- The Rust port (`crates/freshell-sessions`) does NOT have this bug: its cache gate (`directory_index.rs:1032-1034`) is `(mtime, size)` on `metadata.json` only; its comments reference only the recency fold, not the re-parse gate. No Rust changes needed.
- The e2e suite (`test/e2e-browser/specs/session-directory-matrix.spec.ts`) asserts only `lastActivityAt` values and recency ordering — both preserved by this fix. No e2e assertion depends on sidecar-triggered re-parse; no e2e changes needed.

## Global Constraints

- Server code is NodeNext/ESM: relative imports in `server/**` MUST include `.js` extensions (test files under `test/**` omit them — follow each file's existing style).
- Focused test runs use the repo-owned coordinated path: `npm run test:vitest -- run <paths> --config config/vitest/vitest.server.config.ts`. Broad runs use `npm test` / `npm run check` with `FRESHELL_TEST_SUMMARY` set. Never raw `npx vitest`.
- Never restart the self-hosted Freshell server; never use broad kill patterns (`pkill -f node`, etc.).
- Never spy on `fsp.stat` in tests — the cache-hit path legitimately stats the primary file and sidecars.
- Every commit message ends with the repo's Amplifier co-author footer (shown verbatim in each commit step).
- Work happens on branch `fix/indexer-mtime-reparse` in this worktree (`.worktrees/indexer-mtime-reparse`); PR targets `main`.
- PR creation and merge for this change are EXPLICITLY PRE-APPROVED by the user (stated in the kata task instructions): push, open PR via `gh` (identity `dan@danshapiro.com`), wait for required checks, squash-merge, fast-forward local `main`, then close the kata with evidence.
- Do NOT close kata 1bt7 (OOM root-cause confirmation) — only note benchmark evidence for it in the final summary if decisive.

---

### Task 1: Regression tests + cheap-path fix in `updateCacheEntry`

**Files:**
- Modify: `server/coding-cli/session-indexer.ts:899-908` (the re-parse gate) and `server/coding-cli/session-indexer.ts:892-894` (the inline comment above it)
- Test: `test/unit/server/coding-cli/session-indexer.test.ts` (rewrite the test at line 699 inside `describe('activity sidecar recency (getActivityMtimeMs)')` at line 617; add six new tests to the same describe block)

**Interfaces:**
- Consumes: existing test-file helpers — `makeProvider(files, { parseSessionFile, getActivityMtimeMs })` (line 66), `tempDir` from `beforeEach` (line 123), `CodingCliSessionIndexer` constructor, `(indexer as any).markDirty(file)`, `(indexer as any).needsFullScan = true`, `indexer.getProjects()`. In production code: module-local `maxDefined(a?: number, b?: number): number | undefined` at `session-indexer.ts:46`, `CachedSessionEntry.activityMtimeMs?: number` and `.baseSession: CodingCliSession | null` (lines 413-428).
- Produces: new gate semantics that Tasks 2-5 rely on — a cache hit is `cached && cached.mtimeMs === mtimeMs && cached.size === size && !cached.lightweight`; on that hit, `cached.activityMtimeMs` is refreshed and `cached.baseSession.lastActivityAt` (when non-null) is updated to `Math.floor(maxDefined(cached.baseSession.lastActivityAt, activityMtimeMs) ?? cached.baseSession.lastActivityAt)`; the function returns without calling `readSessionSnippet`/`parseSessionFile`.

- [ ] **Step 1: Rewrite the old-contract test (RED)**

In `test/unit/server/coding-cli/session-indexer.test.ts`, find the test at line 699:

```ts
    it('re-parses and advances lastActivityAt when the sidecar grows but metadata.json is byte-identical', async () => {
```

Replace that ENTIRE test (lines 699-732, ending at its closing `})`) with:

```ts
    it('advances lastActivityAt from the sidecar mtime WITHOUT re-parsing when metadata.json is byte-identical', async () => {
      const file = path.join(tempDir, 'session-resumed.jsonl')
      await fsp.writeFile(file, JSON.stringify({ cwd: '/project/a', title: 'Deploy' }) + '\n')

      let activityMtimeMs = 5_000
      const parseSessionFile = vi.fn(async () => ({
        cwd: '/project/a',
        sessionId: 'session-resumed',
        title: 'Deploy',
        createdAt: 500,
        lastActivityAt: 1_000,
        messageCount: 1,
      }))

      const provider = makeProvider([file], {
        parseSessionFile,
        getActivityMtimeMs: async () => activityMtimeMs,
      })

      const indexer = new CodingCliSessionIndexer([provider])
      await indexer.refresh()

      expect(indexer.getProjects()[0]?.sessions[0]?.lastActivityAt).toBe(5_000)
      const callsAfterFirstRefresh = parseSessionFile.mock.calls.length

      // metadata.json stand-in is left byte-identical (no writes, no utimes), so the
      // mtime+size gate must treat this as a cache hit. Only the activity sidecar
      // advanced -- that means "session had activity", NOT "content changed", so the
      // indexer must fold recency WITHOUT re-reading or re-parsing (kata v4rw).
      activityMtimeMs = 9_000
      ;(indexer as any).markDirty(file)
      await indexer.refresh()

      expect(parseSessionFile.mock.calls.length).toBe(callsAfterFirstRefresh)
      expect(indexer.getProjects()[0]?.sessions[0]?.lastActivityAt).toBe(9_000)
    })
```

- [ ] **Step 2: Add the six new tests (RED, same describe block)**

Immediately after the test you just rewrote (still inside `describe('activity sidecar recency (getActivityMtimeMs)')`), add:

```ts
    it('never re-parses across repeated refreshes while only the sidecar mtime keeps advancing', async () => {
      const file = path.join(tempDir, 'session-live.jsonl')
      await fsp.writeFile(file, JSON.stringify({ cwd: '/project/a', title: 'Live' }) + '\n')

      let activityMtimeMs = 5_000
      const parseSessionFile = vi.fn(async () => ({
        cwd: '/project/a',
        sessionId: 'session-live',
        title: 'Live',
        createdAt: 500,
        lastActivityAt: 1_000,
        messageCount: 1,
      }))
      const provider = makeProvider([file], {
        parseSessionFile,
        getActivityMtimeMs: async () => activityMtimeMs,
      })

      const indexer = new CodingCliSessionIndexer([provider])
      await indexer.refresh()
      const callsAfterFirstRefresh = parseSessionFile.mock.calls.length

      // A live Amplifier session ticks its events.jsonl mtime between every refresh.
      // Pre-fix this re-parsed from byte 0 on every single sweep (quadratic cost).
      for (const mtime of [6_000, 7_500, 12_345]) {
        activityMtimeMs = mtime
        ;(indexer as any).markDirty(file)
        await indexer.refresh()
        expect(parseSessionFile.mock.calls.length).toBe(callsAfterFirstRefresh)
        expect(indexer.getProjects()[0]?.sessions[0]?.lastActivityAt).toBe(mtime)
      }
    })

    it('still re-parses when the primary session file itself changes', async () => {
      const file = path.join(tempDir, 'session-edited.jsonl')
      await fsp.writeFile(file, JSON.stringify({ cwd: '/project/a', title: 'Deploy' }) + '\n')

      let activityMtimeMs = 5_000
      const parseSessionFile = vi.fn(async () => ({
        cwd: '/project/a',
        sessionId: 'session-edited',
        title: 'Deploy',
        createdAt: 500,
        lastActivityAt: 1_000,
        messageCount: 1,
      }))
      const provider = makeProvider([file], {
        parseSessionFile,
        getActivityMtimeMs: async () => activityMtimeMs,
      })

      const indexer = new CodingCliSessionIndexer([provider])
      await indexer.refresh()
      const callsAfterFirstRefresh = parseSessionFile.mock.calls.length

      // Real content change: size (and mtime) of the primary file move, so the
      // mtime+size gate must still force a re-parse. Guards against over-caching.
      await fsp.writeFile(
        file,
        JSON.stringify({ cwd: '/project/a', title: 'Deploy' }) + '\n' + JSON.stringify({ appended: true }) + '\n',
      )
      activityMtimeMs = 9_000
      ;(indexer as any).markDirty(file)
      await indexer.refresh()

      expect(parseSessionFile.mock.calls.length).toBeGreaterThan(callsAfterFirstRefresh)
      expect(indexer.getProjects()[0]?.sessions[0]?.lastActivityAt).toBe(9_000)
    })

    it('does not regress lastActivityAt when the sidecar mtime moves backwards on the no-re-parse path', async () => {
      const file = path.join(tempDir, 'session-regress.jsonl')
      await fsp.writeFile(file, JSON.stringify({ cwd: '/project/a', title: 'Deploy' }) + '\n')

      let activityMtimeMs = 5_000
      const parseSessionFile = vi.fn(async () => ({
        cwd: '/project/a',
        sessionId: 'session-regress',
        title: 'Deploy',
        createdAt: 500,
        lastActivityAt: 1_000,
        messageCount: 1,
      }))
      const provider = makeProvider([file], {
        parseSessionFile,
        getActivityMtimeMs: async () => activityMtimeMs,
      })

      const indexer = new CodingCliSessionIndexer([provider])
      await indexer.refresh()
      expect(indexer.getProjects()[0]?.sessions[0]?.lastActivityAt).toBe(5_000)
      const callsAfterFirstRefresh = parseSessionFile.mock.calls.length

      // A sidecar being deleted/recreated can present an older mtime; the cheap
      // fold must stay max-monotonic (mirrors the parse-path test at :645).
      activityMtimeMs = 3_000
      ;(indexer as any).markDirty(file)
      await indexer.refresh()

      expect(parseSessionFile.mock.calls.length).toBe(callsAfterFirstRefresh)
      expect(indexer.getProjects()[0]?.sessions[0]?.lastActivityAt).toBe(5_000)
    })

    it('floors fractional sidecar mtimes on the no-re-parse path', async () => {
      const file = path.join(tempDir, 'session-fractional.jsonl')
      await fsp.writeFile(file, JSON.stringify({ cwd: '/project/a', title: 'Deploy' }) + '\n')

      let activityMtimeMs: number = 5_000
      const parseSessionFile = vi.fn(async () => ({
        cwd: '/project/a',
        sessionId: 'session-fractional',
        title: 'Deploy',
        createdAt: 500,
        lastActivityAt: 1_000,
        messageCount: 1,
      }))
      const provider = makeProvider([file], {
        parseSessionFile,
        getActivityMtimeMs: async () => activityMtimeMs,
      })

      const indexer = new CodingCliSessionIndexer([provider])
      await indexer.refresh()
      const callsAfterFirstRefresh = parseSessionFile.mock.calls.length

      // Downstream read-model schemas validate lastActivityAt as z.number().int()
      // (mirrors the parse-path flooring test at :671).
      activityMtimeMs = 9_000.75
      ;(indexer as any).markDirty(file)
      await indexer.refresh()

      expect(parseSessionFile.mock.calls.length).toBe(callsAfterFirstRefresh)
      expect(indexer.getProjects()[0]?.sessions[0]?.lastActivityAt).toBe(9_000)
    })

    it('keeps skipping the re-parse for cwd-less sessions when only the sidecar mtime advances', async () => {
      const file = path.join(tempDir, 'session-nocwd.jsonl')
      await fsp.writeFile(file, JSON.stringify({ title: 'no cwd yet' }) + '\n')

      let activityMtimeMs = 5_000
      // No cwd => the indexer caches { baseSession: null } (session-indexer.ts:940-948).
      const parseSessionFile = vi.fn(async () => ({}))
      const provider = makeProvider([file], {
        parseSessionFile,
        getActivityMtimeMs: async () => activityMtimeMs,
      })

      const indexer = new CodingCliSessionIndexer([provider])
      await indexer.refresh()
      const callsAfterFirstRefresh = parseSessionFile.mock.calls.length
      expect(indexer.getProjects()).toEqual([])

      // The cheap path must tolerate baseSession === null without crashing and
      // without falling through to a re-parse.
      activityMtimeMs = 9_000
      ;(indexer as any).markDirty(file)
      await indexer.refresh()

      expect(parseSessionFile.mock.calls.length).toBe(callsAfterFirstRefresh)
      expect(indexer.getProjects()).toEqual([])
    })

    it('skips the re-parse on a warm full rescan when only the sidecar mtime advanced', async () => {
      const file = path.join(tempDir, 'session-fullscan.jsonl')
      await fsp.writeFile(file, JSON.stringify({ cwd: '/project/a', title: 'Deploy' }) + '\n')

      let activityMtimeMs = 5_000
      const parseSessionFile = vi.fn(async () => ({
        cwd: '/project/a',
        sessionId: 'session-fullscan',
        title: 'Deploy',
        createdAt: 500,
        lastActivityAt: 1_000,
        messageCount: 1,
      }))
      const provider = makeProvider([file], {
        parseSessionFile,
        getActivityMtimeMs: async () => activityMtimeMs,
      })

      const indexer = new CodingCliSessionIndexer([provider])
      await indexer.refresh()
      const callsAfterFirstRefresh = parseSessionFile.mock.calls.length

      // The periodic full rescan (session-indexer.ts:1431-1442) also routes every
      // file through updateCacheEntry -- it must take the same cheap path.
      activityMtimeMs = 9_000
      ;(indexer as any).needsFullScan = true
      await indexer.refresh()

      expect(parseSessionFile.mock.calls.length).toBe(callsAfterFirstRefresh)
      expect(indexer.getProjects()[0]?.sessions[0]?.lastActivityAt).toBe(9_000)
    })
```

- [ ] **Step 3: Run the tests and verify they fail for the right reason (RED)**

Run:

```bash
cd /home/dan/code/freshell/.worktrees/indexer-mtime-reparse
npm run test:vitest -- run test/unit/server/coding-cli/session-indexer.test.ts \
  --config config/vitest/vitest.server.config.ts
```

Expected: the rewritten test and five of the six new tests FAIL on their
`parseSessionFile.mock.calls.length` assertion (actual count is greater than
`callsAfterFirstRefresh`, because current code still re-parses when the sidecar
mtime moves). `'still re-parses when the primary session file itself changes'`
PASSES (it pins current behavior that must survive the fix). All pre-existing
tests in the file (including `:618`, `:645`, `:671`) still PASS. If any test
fails for a different reason (setup error, wrong fixture), fix the test — do
not proceed to implementation until the failures are exactly the no-re-parse
assertions.

- [ ] **Step 4: Implement the cheap path (GREEN)**

In `server/coding-cli/session-indexer.ts`, replace lines 892-908. Current code:

```ts
    // Newest activity-sidecar mtime (Amplifier transcript.jsonl / events.jsonl). Statted
    // once here and reused by both the re-parse gate and the recency fold below. Only
    // providers that opt in pay the extra stat cost; others leave it undefined.
    const activityMtimeMs = provider.getActivityMtimeMs
      ? await provider.getActivityMtimeMs(filePath)
      : undefined

    const cached = this.fileCache.get(cacheKey)
    if (
      cached &&
      cached.mtimeMs === mtimeMs &&
      cached.size === size &&
      cached.activityMtimeMs === activityMtimeMs &&
      !cached.lightweight
    ) {
      return
    }
```

New code:

```ts
    // Newest activity-sidecar mtime (Amplifier transcript.jsonl / events.jsonl). Statted
    // once here and used only for the recency fold. Only providers that opt in pay the
    // extra stat cost; others leave it undefined.
    const activityMtimeMs = provider.getActivityMtimeMs
      ? await provider.getActivityMtimeMs(filePath)
      : undefined

    const cached = this.fileCache.get(cacheKey)
    if (cached && cached.mtimeMs === mtimeMs && cached.size === size && !cached.lightweight) {
      // The primary session file is byte-identical, so the cached parse stays valid.
      // A moved activity-sidecar mtime only means "session had activity" -- sidecars
      // are never inputs to the parse -- so fold it into recency in place instead of
      // re-parsing from byte 0. Treating it as cache-invalidating made every live
      // Amplifier session re-parse on every refresh, quadratic in session lifetime
      // (kata v4rw). The fold mirrors the parse-path fold below: max-monotonic so a
      // regressed sidecar mtime never lowers recency, floored because downstream
      // read models validate lastActivityAt as an integer.
      if (activityMtimeMs !== undefined && activityMtimeMs !== cached.activityMtimeMs) {
        cached.activityMtimeMs = activityMtimeMs
        if (cached.baseSession) {
          cached.baseSession.lastActivityAt = Math.floor(
            maxDefined(cached.baseSession.lastActivityAt, activityMtimeMs) ??
              cached.baseSession.lastActivityAt,
          )
        }
      }
      return
    }
```

Notes for the implementer:
- `maxDefined` is the module-local two-arg helper already defined at `session-indexer.ts:46` — no import needed.
- Do NOT move the early return below the `sessionKeyToFilePath.delete(...)` block at lines 910-913 — the cheap path must not delete the session-key mapping (nothing is being re-created).
- Do NOT touch the `!cached.lightweight` term — lightweight entries must still fall through to a real parse.

- [ ] **Step 5: Run the tests and verify they pass (GREEN)**

Run:

```bash
cd /home/dan/code/freshell/.worktrees/indexer-mtime-reparse
npm run test:vitest -- run test/unit/server/coding-cli/session-indexer.test.ts \
  test/unit/server/coding-cli/amplifier-provider.test.ts \
  --config config/vitest/vitest.server.config.ts
```

Expected: ALL tests PASS in both files (the amplifier-provider `getActivityMtimeMs` tests at `amplifier-provider.test.ts:164-252` are untouched and must stay green — the provider contract is unchanged).

- [ ] **Step 6: Commit**

```bash
cd /home/dan/code/freshell/.worktrees/indexer-mtime-reparse
git add server/coding-cli/session-indexer.ts test/unit/server/coding-cli/session-indexer.test.ts
git commit -m "$(cat <<'EOF'
fix(server): fold sidecar activity mtime without re-parsing sessions (v4rw)

The session-indexer re-parse gate treated the Amplifier activity-sidecar
mtime (events.jsonl/transcript.jsonl) as cache-invalidating, so every live
Amplifier session failed the gate on every refresh (5s throttle floor) and
re-parsed from byte 0 -- quadratic cost over a session's lifetime and the
leading suspect for the production OOM signature. The only datum the mtime
carries is recency, so the gate now folds it into the cached session's
lastActivityAt in place (max-monotonic, floored) and skips the re-parse.

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>
EOF
)"
```

---

### Task 2: Refactor — update the three stale contract comments

**Files:**
- Modify: `server/coding-cli/provider.ts:20-27` (doc comment on `getActivityMtimeMs`)
- Modify: `server/coding-cli/session-indexer.ts:420-426` (doc comment on `CachedSessionEntry.activityMtimeMs`)
- Modify: `server/coding-cli/providers/amplifier.ts:184-187` (inline comment in `getActivityMtimeMs`)

**Interfaces:**
- Consumes: the Task 1 gate semantics (comments must describe them accurately).
- Produces: nothing new — documentation-only refactor step of the Task 1 cycle. No behavior change; no test changes.

- [ ] **Step 1: Update `provider.ts` doc comment**

In `server/coding-cli/provider.ts`, the doc comment above `getActivityMtimeMs?` currently reads:

```ts
  /**
   * Newest mtime (ms) among the session's activity sidecar files (e.g. Amplifier's
   * `transcript.jsonl` / `events.jsonl`), or undefined if none exist. Providers whose
   * recency is fully captured by their primary session file omit this. The indexer uses
   * it both to fold real file activity into recency and to force a re-parse when a sidecar
   * grows even though the primary session file is unchanged.
   */
```

Replace with:

```ts
  /**
   * Newest mtime (ms) among the session's activity sidecar files (e.g. Amplifier's
   * `transcript.jsonl` / `events.jsonl`), or undefined if none exist. Providers whose
   * recency is fully captured by their primary session file omit this. The indexer folds
   * it into the session's lastActivityAt (recency only); it never invalidates the parse
   * cache -- sidecars are not inputs to the parse, so a moved sidecar mtime must not
   * force a re-parse (kata v4rw).
   */
```

- [ ] **Step 2: Update `CachedSessionEntry.activityMtimeMs` doc comment**

In `server/coding-cli/session-indexer.ts` (lines 420-426), the comment currently reads:

```ts
  /**
   * Newest mtime (ms) among the session's activity sidecars (see
   * CodingCliProvider.getActivityMtimeMs). Shared between the re-parse gate and the
   * recency fold so a grown sidecar forces a re-parse even when the primary file is
   * byte-identical. Undefined for providers that don't expose sidecar activity.
   */
```

Replace with:

```ts
  /**
   * Newest mtime (ms) among the session's activity sidecars (see
   * CodingCliProvider.getActivityMtimeMs). Recency bookkeeping only: on a cache hit it
   * is folded into baseSession.lastActivityAt in place and never triggers a re-parse
   * (kata v4rw). Undefined for providers that don't expose sidecar activity.
   */
```

- [ ] **Step 3: Update the `amplifier.ts` inline comment**

In `server/coding-cli/providers/amplifier.ts`, inside `getActivityMtimeMs` (lines 184-187), the comment currently reads:

```ts
    // Amplifier writes session activity to sibling sidecars next to metadata.json.
    // metadata.json's own mtime lags real activity (it only changes on name/description
    // updates), so recency and the re-parse gate must also consider these files.
    // We only stat (never read) so the cost stays a couple of syscalls per session.
```

Replace with:

```ts
    // Amplifier writes session activity to sibling sidecars next to metadata.json.
    // metadata.json's own mtime lags real activity (it only changes on name/description
    // updates), so recency must also consider these files. This feeds ONLY the
    // lastActivityAt fold -- it never invalidates the indexer's parse cache (v4rw).
    // We only stat (never read) so the cost stays a couple of syscalls per session.
```

- [ ] **Step 4: Verify no behavior change**

Run:

```bash
cd /home/dan/code/freshell/.worktrees/indexer-mtime-reparse
npm run test:vitest -- run test/unit/server/coding-cli/session-indexer.test.ts \
  test/unit/server/coding-cli/amplifier-provider.test.ts \
  --config config/vitest/vitest.server.config.ts
```

Expected: ALL tests PASS (comment-only change).

- [ ] **Step 5: Commit**

```bash
cd /home/dan/code/freshell/.worktrees/indexer-mtime-reparse
git add server/coding-cli/provider.ts server/coding-cli/session-indexer.ts server/coding-cli/providers/amplifier.ts
git commit -m "$(cat <<'EOF'
docs(server): align activity-mtime contract comments with recency-only semantics (v4rw)

Three comments still described the sidecar activity mtime as a re-parse
trigger; it is now recency-only.

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>
EOF
)"
```

---

### Task 3: Benchmark evidence for the OOM signature (kata 1bt7 support)

Quantify the work eliminated per refresh, using the REAL amplifier provider and
REAL indexer against a temp Amplifier home. This produces the evidence the kata
task asks for ("if your verification produces evidence tying this mechanism to
the OOM signature ... note that evidence clearly"). The script and results are
working artifacts — NOT committed to the repo.

**Files:**
- Create (NOT committed — lives outside the repo tree): `/home/dan/code/freshell/.worktrees/.the-usual-logs/indexer-mtime-reparse/bench/bench-reparse.mts`
- Create (NOT committed): `/home/dan/code/freshell/.worktrees/.the-usual-logs/indexer-mtime-reparse/benchmark-results.md`

**Interfaces:**
- Consumes: the fixed `CodingCliSessionIndexer` and `amplifierProvider` from Task 1's commit; `configStore` (monkey-patched in-process, never written to disk).
- Produces: `benchmark-results.md` with per-round numbers, consumed by Task 5's PR body and kata close message.

- [ ] **Step 1: Write the benchmark script**

Create `/home/dan/code/freshell/.worktrees/.the-usual-logs/indexer-mtime-reparse/bench/bench-reparse.mts`:

```ts
// Throwaway benchmark for kata v4rw / 1bt7 evidence. NOT part of the repo.
// Run from the worktree root with:
//   npx tsx /home/dan/code/freshell/.worktrees/.the-usual-logs/indexer-mtime-reparse/bench/bench-reparse.mts
//
// Compares, on the FIXED code, two per-round mutations over the same fixture set:
//   A) touch metadata.json mtime  -> fails the mtime gate -> full re-parse path.
//      This is byte-for-byte the code path the pre-fix sidecar gate forced every
//      refresh for every live session (readSessionSnippet + parseSessionFile +
//      resolve + rebuild), so it measures the eliminated work faithfully.
//   B) touch events.jsonl mtime   -> post-fix cheap path (fold only, no re-parse).
import fsp from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { performance } from 'node:perf_hooks'
import { CodingCliSessionIndexer } from '/home/dan/code/freshell/.worktrees/indexer-mtime-reparse/server/coding-cli/session-indexer.js'
import { amplifierProvider } from '/home/dan/code/freshell/.worktrees/indexer-mtime-reparse/server/coding-cli/providers/amplifier.js'
import { configStore } from '/home/dan/code/freshell/.worktrees/indexer-mtime-reparse/server/config-store.js'

const SESSIONS = Number(process.env.BENCH_SESSIONS ?? 200)
const ROUNDS = Number(process.env.BENCH_ROUNDS ?? 10)
const TRANSCRIPT_BYTES = Number(process.env.BENCH_TRANSCRIPT_BYTES ?? 512 * 1024)

;(configStore as any).snapshot = async () => ({
  sessionOverrides: {},
  settings: { codingCli: { enabledProviders: ['amplifier'], providers: {} } },
})
;(configStore as any).getProjectColors = async () => ({})

async function buildFixture(home: string): Promise<string[]> {
  const dirs: string[] = []
  const line = JSON.stringify({ role: 'assistant', content: 'x'.repeat(200) }) + '\n'
  const transcript = line.repeat(Math.ceil(TRANSCRIPT_BYTES / line.length))
  for (let i = 0; i < SESSIONS; i++) {
    const dir = path.join(home, 'projects', 'bench', 'sessions', `s${i}`)
    await fsp.mkdir(dir, { recursive: true })
    await fsp.writeFile(
      path.join(dir, 'metadata.json'),
      JSON.stringify({ session_id: `s${i}`, working_dir: '/tmp/bench-project', turn_count: 12, name: `bench ${i}` }),
    )
    await fsp.writeFile(path.join(dir, 'transcript.jsonl'), transcript)
    await fsp.writeFile(path.join(dir, 'events.jsonl'), '{"event":"noop"}\n')
    dirs.push(dir)
  }
  return dirs
}

async function runScenario(
  name: string,
  home: string,
  dirs: string[],
  touchTarget: 'metadata.json' | 'events.jsonl',
) {
  let parseCalls = 0
  const provider = {
    ...amplifierProvider,
    homeDir: home,
    parseSessionFile: async (content: string, filePath: string) => {
      parseCalls++
      return amplifierProvider.parseSessionFile(content, filePath)
    },
  }
  const indexer = new CodingCliSessionIndexer([provider as any])
  await indexer.refresh() // cold start (full scan + parse of everything)
  if (indexer.getProjects().length === 0) {
    throw new Error('fixture shape wrong -- align metadata.json fields with parseSessionFile in providers/amplifier.ts')
  }
  console.log(`\n=== ${name} (sessions=${SESSIONS}, rounds=${ROUNDS}) ===`)
  console.log(`cold-start parses: ${parseCalls}`)
  for (let round = 1; round <= ROUNDS; round++) {
    const t = new Date(Date.now() + round * 60_000)
    for (const dir of dirs) {
      await fsp.utimes(path.join(dir, touchTarget), t, t)
      ;(indexer as any).markDirty(path.join(dir, 'metadata.json'))
    }
    const parsesBefore = parseCalls
    const heapBefore = process.memoryUsage().heapUsed
    const start = performance.now()
    await indexer.refresh()
    const ms = performance.now() - start
    const heapDeltaMb = (process.memoryUsage().heapUsed - heapBefore) / 1e6
    console.log(
      `round ${round}: ${ms.toFixed(1)} ms, parses=${parseCalls - parsesBefore}, heapDelta=${heapDeltaMb.toFixed(1)} MB`,
    )
  }
}

async function main() {
  const home = await fsp.mkdtemp(path.join(os.tmpdir(), 'freshell-bench-v4rw-'))
  try {
    const dirs = await buildFixture(home)
    await runScenario('A: forced re-parse (pre-fix behavior for live sessions)', home, dirs, 'metadata.json')
    await runScenario('B: sidecar tick only (post-fix cheap path)', home, dirs, 'events.jsonl')
  } finally {
    await fsp.rm(home, { recursive: true, force: true })
  }
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
```

- [ ] **Step 2: Run it and capture output**

```bash
cd /home/dan/code/freshell/.worktrees/indexer-mtime-reparse
npx tsx /home/dan/code/freshell/.worktrees/.the-usual-logs/indexer-mtime-reparse/bench/bench-reparse.mts \
  2>&1 | tee /home/dan/code/freshell/.worktrees/.the-usual-logs/indexer-mtime-reparse/bench-raw.log
```

Expected shape: scenario A shows `parses=200` per round with nonzero per-round
duration/heap churn; scenario B shows `parses=0` per round with near-zero
duration. If the fixture-shape guard throws, read `parseSessionFile` in
`server/coding-cli/providers/amplifier.ts` and align the `metadata.json`
field names in `buildFixture`, then re-run.

- [ ] **Step 3: Write up results HONESTLY**

Write `/home/dan/code/freshell/.worktrees/.the-usual-logs/indexer-mtime-reparse/benchmark-results.md`
containing: the parameters used, the raw per-round table for both scenarios,
and a conclusions section. Honesty rules for the conclusions:

- State plainly what the numbers show (work eliminated per refresh: parse
  calls, per-round duration, heap churn).
- The fix's per-reparse reads are bounded (metadata.json fully + at most
  64 KiB of transcript.jsonl; events.jsonl is stat-only), so this benchmark
  may NOT reproduce GB-scale allocations or 100s event-loop stalls. If it
  does not, say exactly that: "evidence shows the mechanism and its
  elimination, but is NOT decisive for the production GB-heap OOM signature
  (kata 1bt7)". Do NOT extrapolate numbers that were not measured, and do
  NOT recommend closing 1bt7 unless the measurements are decisive on their
  face (e.g. multi-second rounds / GB-scale heap deltas at realistic scale).

No commit — these artifacts stay outside the repo tree.

---

### Task 4: Full verification

**Files:** none modified — verification only.

**Interfaces:**
- Consumes: all commits from Tasks 1-2.
- Produces: a green coordinated full suite + typecheck, prerequisite for Task 5.

- [ ] **Step 1: Confirm clean tree and expected commits**

```bash
cd /home/dan/code/freshell/.worktrees/indexer-mtime-reparse
git status --short && git log --oneline origin/main..HEAD
```

Expected: empty status (the benchmark artifacts live outside the repo tree);
log shows the plan commit plus the Task 1 and Task 2 commits.

- [ ] **Step 2: Run typecheck + coordinated full suite**

```bash
cd /home/dan/code/freshell/.worktrees/indexer-mtime-reparse
FRESHELL_TEST_SUMMARY="v4rw indexer sidecar-mtime no-reparse: full verification" npm run check
```

Expected: typecheck passes and the coordinated full suite (default + server
configs) is green. This waits for the shared coordinator gate if another agent
holds it — wait, never kill a foreign holder. If any test fails, fix it (root
cause, not symptom) before proceeding; re-run until green. Timebox note for the
implementer: this is a broad run and can take many minutes — use a generous
bash timeout (e.g. 1800s).

---

### Task 5: Land — push, PR, merge, kata close (EXPLICITLY PRE-APPROVED)

The user has EXPLICITLY pre-approved PR creation and merge for this change (in
the kata task instructions). This satisfies the AGENTS.md approval gate.

**Files:** none modified — git/gh/kata operations only.

**Interfaces:**
- Consumes: green verification from Task 4; `benchmark-results.md` from Task 3.
- Produces: merged PR on `main`, fast-forwarded local `main`, closed kata v4rw.

- [ ] **Step 1: Push the branch**

```bash
cd /home/dan/code/freshell/.worktrees/indexer-mtime-reparse
git push -u origin fix/indexer-mtime-reparse
```

- [ ] **Step 2: Open the PR (gh identity dan@danshapiro.com)**

Compose the PR body from: the mechanism (gate at session-indexer.ts:900-908
treated sidecar mtime as cache-invalidating), the fix (recency-only in-place
fold), the test coverage added, and a short "Benchmark evidence" section
copied from the conclusions of
`/home/dan/code/freshell/.worktrees/.the-usual-logs/indexer-mtime-reparse/benchmark-results.md`
(honest wording preserved — including the not-decisive caveat if that is what
the results showed). Then:

```bash
cd /home/dan/code/freshell/.worktrees/indexer-mtime-reparse
gh pr create --base main --head fix/indexer-mtime-reparse \
  --title "fix(server): fold sidecar activity mtime without re-parsing sessions (v4rw)" \
  --body-file /home/dan/code/freshell/.worktrees/.the-usual-logs/indexer-mtime-reparse/pr-body.md
```

(Write the composed body to that `pr-body.md` path first.)

- [ ] **Step 3: Wait for required checks, then squash-merge**

```bash
gh pr checks fix/indexer-mtime-reparse --watch
gh pr merge fix/indexer-mtime-reparse --squash --delete-branch
```

Expected: all required checks pass before merge. If a check fails, fix in the
worktree, push, and re-watch — do not merge red.

- [ ] **Step 4: Fast-forward local main**

```bash
cd /home/dan/code/freshell
git checkout main 2>/dev/null || true
git pull --ff-only origin main
git log --oneline -1
```

Expected: fast-forward succeeds; note the merged squash commit SHA from the
last command — it is needed for the kata close. If the pull cannot
fast-forward, STOP and surface the conflict rather than creating a merge
commit.

- [ ] **Step 5: Close kata v4rw with evidence**

Run from the repo root, substituting the real merged SHA and a substantive
summary that includes: the mechanism, the fix, the regression tests, and the
benchmark evidence sentence (decisive or explicitly not decisive for 1bt7):

```bash
cd /home/dan/code/freshell
kata close v4rw --done --commit <merged-sha> --message "Session-indexer no longer treats amplifier sidecar (events.jsonl/transcript.jsonl) mtime as cache-invalidating: the re-parse gate at session-indexer.ts:900-908 dropped the activityMtimeMs term and the cache-hit path now folds the mtime into cached baseSession.lastActivityAt in place (max-monotonic, floored). Regression tests assert zero parseSessionFile calls when only the sidecar mtime moves (incremental and full-rescan paths), plus guards for real-content re-parse, monotonicity, flooring, and cwd-less entries. <one-sentence benchmark evidence summary from benchmark-results.md>"
```

Do NOT close kata 1bt7. If (and only if) the Task 3 benchmark was decisive for
the OOM signature, mention that in the close message so 1bt7 can be closed
against it by the user.

---

## Self-Review (completed by the plan author)

1. **Spec coverage:** Fix mechanism (drop activityMtimeMs from gate, fold-only) — Task 1. TDD regression test asserting no re-read when only events.jsonl mtime changes — Task 1 Steps 1-3 (parseSessionFile call-count is the established idiom in this file; a `readSessionSnippet` spy is impossible without a test-only export, an anti-pattern). Repo conventions (worktree, coordinated tests, .js extensions, commit footer, gh identity) — Global Constraints + concrete commands. Landing instructions incl. kata close — Task 5. Benchmark evidence for 1bt7 — Task 3, with explicit honesty rules. Rust port and e2e checked during exploration: no changes needed (documented in "Why this is safe").
2. **No silent deferrals:** all tests run against the real indexer with real temp-dir files and the fake-provider factory the suite already uses; the benchmark uses the real amplifier provider end to end. No stubs stand in for required production behavior. No known-limitations bucket used.
3. **Placeholder scan:** every code step contains complete code; every command has expected output. The single conditional instruction (benchmark fixture-shape guard) is a real runtime guard with a concrete recovery action, not a deferral.
4. **Type consistency:** `maxDefined(a?: number, b?: number): number | undefined` (session-indexer.ts:46) used with `?? cached.baseSession.lastActivityAt` fallback so the assignment stays `number`; `CachedSessionEntry.activityMtimeMs?: number` assigned from `number | undefined` only inside the `!== undefined` guard; `baseSession: CodingCliSession | null` null-guarded on the cheap path; test fixtures match `ParsedSessionMeta` (all fields optional).
