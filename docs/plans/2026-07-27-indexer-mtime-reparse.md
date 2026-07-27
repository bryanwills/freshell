# Session-Indexer: Sidecar Activity Mtime Without Re-Parse (kata v4rw) Implementation Plan

> **For agentic workers:** This plan is executed task-by-task by the
> workflow's execute stage: a fresh implementer per task, with a spec +
> quality review after each task. Steps use checkbox (`- [ ]`) syntax
> for tracking.

**Goal:** Stop the coding-CLI session indexer from forcing a full re-parse of a session — on every re-evaluation of the cache gate (in production: each warm full rescan, default 10 minutes) — when only the Amplifier activity sidecar (`events.jsonl`/`transcript.jsonl`) mtime moved. Fold the mtime into `lastActivityAt` on the cache-hit path instead (semantic no-op, kata issue v4rw, filed P1 oom/perf; plan review downgraded the OOM linkage — see "Why this is safe").

**Architecture:** Single-file behavioral change in `server/coding-cli/session-indexer.ts`. The re-parse gate in `updateCacheEntry` (lines 899-908) currently requires `cached.activityMtimeMs === activityMtimeMs` for a cache hit; a live Amplifier session's `events.jsonl` mtime keeps advancing, so every time the gate is re-evaluated for an unchanged `metadata.json` the check at line 904 fails and the session re-parses even though nothing parse-relevant changed. Validated trigger path (plan review, V2): sidecar mtime ticks do NOT themselves schedule refreshes — the chokidar watcher globs only `**/sessions/**/metadata.json` (`providers/amplifier.ts:168-170`) and `markDirty` is only called from watcher handlers — so the recurring route to the gate for an unchanged primary file is the warm full rescan via `fullScanTimer` (default 10 minutes, `session-indexer.ts:1431-1442`). Pre-fix, every such rescan re-parsed every session whose sidecar mtime had moved. The only consumer of `activityMtimeMs` besides the gate is the recency fold at line 977 (`lastActivityAt = Math.floor(maxDefined(lastActivityAt, activityMtimeMs) ?? 0)`). The fix drops the `activityMtimeMs` term from the gate and, on the cache-hit branch, folds the fresh sidecar mtime into `cached.baseSession.lastActivityAt` in place (max-monotonic, floored). `buildProjectGroups` (line 1117) reads `cached.baseSession` straight out of `fileCache` on every refresh, so the in-place fold is picked up with no extra plumbing whenever the gate is next re-evaluated (watcher-driven revisit or full rescan) — the fold's live cadence matches the gate's, and adding sidecar-tick-driven refresh plumbing is explicitly out of scope (it would reintroduce refresh churn).

**Tech Stack:** TypeScript (NodeNext/ESM), Vitest (server config), node:fs/promises temp-dir fixtures, existing `makeProvider` fake-provider factory in the test file.

## Why this is safe (survey evidence, from the kata issue)

- `events.jsonl` is write-only observability — Amplifier never reads it back. Its mtime moving means ONLY "session had activity".
- `transcript.jsonl` mtime movement carries no parse-relevant data: the indexer parses `metadata.json` (the primary session file), whose own `mtimeMs`+`size` gate terms remain untouched. The transcript's 64 KiB prefix is read only as enrichment DURING a parse (verified: `messageCount` comes from metadata's `turn_count`; title/summary/lastActivityAt come from metadata JSON). Enrichment staleness is a non-issue: Amplifier pairs every `transcript.jsonl` write with an immediate `metadata.json` rewrite (`session_store.py:103-131`, transcript written first; 19/19 real sessions show transcript mtime <= metadata mtime), so any transcript change re-parses via the metadata gate within milliseconds anyway.
- `metadata.json` is NOT rare-write (plan review falsified the code comment's "only changes on name/description updates"): Amplifier rewrites it at every turn end and on tool:post via the incremental-save hook, carrying `turn_count`. Those rewrites legitimately re-parse (content changed) and are untouched by this fix. The window this fix targets is sidecar-tick-only movement — `events.jsonl` ticking mid-turn while `metadata.json` is static — caught by the periodic full rescan.
- Per-session `events.jsonl` reaches 75 MB–1.1 GB in production while `transcript.jsonl` stays ~200-900 KB. The forced re-parse fired, for every session whose sidecar mtime had advanced, on every gate re-evaluation — in production that is each warm full rescan (default 10 minutes), NOT every 5s refresh (sidecar ticks alone do not schedule refreshes). Per-re-parse cost is size-capped (metadata fully + <=64 KiB transcript prefix; events.jsonl stat-only), so this is bounded waste plus a semantic bug — it is NOT established as the production OOM mechanism (the events-tailer backlog drain, kata myap, fixed in 32463af5, matches that signature better).
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
- Landing gate (validated during plan review, V5): the kata v4rw task text contains NO landing pre-approval, and AGENTS.md requires explicit user approval before `gh pr create`. Task 5 therefore pushes the branch and then HALTS for explicit user approval before creating the PR. After approval: open PR via `gh` (identity `dan@danshapiro.com`), wait for the reported CI checks (main has no branch protection — nothing is server-enforced; never merge red anyway), squash-merge, fast-forward local `main`, then close the kata with evidence.
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

      // A live Amplifier session's events.jsonl mtime keeps advancing between sweeps.
      // Pre-fix, every gate re-evaluation (e.g. each warm full rescan) re-parsed it
      // even though nothing parse-relevant changed.
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
      // re-parsing from byte 0. Treating it as cache-invalidating made every session
      // with a moved sidecar mtime re-parse on every gate re-evaluation (each warm
      // full rescan, plus any watcher-driven revisit) even though nothing
      // parse-relevant changed (kata v4rw). The fold mirrors the parse-path fold
      // below: max-monotonic so a
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
mtime (events.jsonl/transcript.jsonl) as cache-invalidating, so any session
whose sidecar mtime had advanced failed the gate on every re-evaluation --
in production, every warm full rescan (default 10 min) re-parsed every live
Amplifier session from byte 0 even though nothing parse-relevant changed.
The re-parse is size-capped, so this was bounded waste and a semantic bug
rather than a proven OOM mechanism. The only datum the sidecar mtime
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
   * cache -- a moved sidecar mtime means only "session had activity", not "parse inputs
   * changed", so it must not force a re-parse (kata v4rw).
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
    // metadata.json is rewritten at turn boundaries (turn end / incremental tool
    // saves) but goes quiet mid-turn while events.jsonl keeps ticking, so recency
    // must also consider these files. This feeds ONLY the lastActivityAt fold -- it
    // never invalidates the indexer's parse cache (v4rw).
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
// Run from the worktree root with the VENDORED tsx (bare `npx tsx` may fetch):
//   node_modules/.bin/tsx /home/dan/code/freshell/.worktrees/.the-usual-logs/indexer-mtime-reparse/bench/bench-reparse.mts
//
// Compares, on the FIXED code, two per-round mutations over the same fixture set:
//   A) touch metadata.json mtime  -> fails the mtime gate -> full re-parse path.
//      This is byte-for-byte the code path the pre-fix sidecar gate forced on
//      every gate re-evaluation (each warm full rescan, default 10 min) for every
//      live session (readSessionSnippet + parseSessionFile + resolve + rebuild),
//      so it measures the eliminated work faithfully.
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
  await indexer.refresh() // cold start (lightweight scan + enrichment of at most ENRICHMENT_BATCH_SIZE=150 sessions)
  if (indexer.getProjects().length === 0) {
    throw new Error('fixture shape wrong -- align metadata.json fields with parseSessionFile in providers/amplifier.ts')
  }
  console.log(`\n=== ${name} (sessions=${SESSIONS}, rounds=${ROUNDS}) ===`)
  console.log(`cold-start parses: ${parseCalls}`)
  // Warm-up: cold-start enrichment is batched (ENRICHMENT_BATCH_SIZE = 150 in
  // session-indexer.ts), so SESSIONS-150 fixtures are still unparsed here. One
  // warm-up sweep parses the leftovers so measured rounds isolate gate behavior.
  for (const dir of dirs) (indexer as any).markDirty(path.join(dir, 'metadata.json'))
  await indexer.refresh()
  console.log(`parses after cold start + warm-up: ${parseCalls}`)
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
node_modules/.bin/tsx /home/dan/code/freshell/.worktrees/.the-usual-logs/indexer-mtime-reparse/bench/bench-reparse.mts \
  2>&1 | tee /home/dan/code/freshell/.worktrees/.the-usual-logs/indexer-mtime-reparse/bench-raw.log
```

(Vendored tsx: it is a devDependency and validated present at
`node_modules/.bin/tsx` — do NOT use bare `npx tsx`, which may fetch.)

Expected shape (validated against the real indexer during plan review, V4):
cold-start shows `parses=150` (enrichment is batched at
`ENRICHMENT_BATCH_SIZE = 150`), the warm-up sweep parses the remaining 50;
then scenario A shows `parses=200` per round with nonzero per-round
duration/heap churn; scenario B shows `parses=0` per round with near-zero
duration. If the fixture-shape guard throws, read `parseSessionFile` in
`server/coding-cli/providers/amplifier.ts` and align the `metadata.json`
field names in `buildFixture`, then re-run.

- [ ] **Step 3: Write up results HONESTLY**

Write `/home/dan/code/freshell/.worktrees/.the-usual-logs/indexer-mtime-reparse/benchmark-results.md`
containing: the parameters used, the raw per-round table for both scenarios,
and a conclusions section. Honesty rules for the conclusions:

- State plainly what the numbers show (work eliminated per sweep: parse
  calls, per-round duration, heap churn).
- Frame the savings against the PRODUCTION trigger cadence established in
  plan review (V2): the pre-fix re-parse fired on each warm full rescan
  (default 10 minutes; sidecar ticks alone do not schedule refreshes) — NOT
  on every 5s refresh. Do not describe the eliminated work as per-5s or
  "quadratic".
- The fix's per-reparse reads are bounded (metadata.json fully + at most
  64 KiB of transcript.jsonl; events.jsonl is stat-only), so this benchmark
  may NOT reproduce GB-scale allocations or 100s event-loop stalls. If it
  does not, say exactly that: "evidence shows the mechanism and its
  elimination, but is NOT decisive for the production GB-heap OOM signature
  (kata 1bt7)". Plan review's arithmetic (bounded per-parse cost x ~6
  sweeps/hour) already indicates this mechanism cannot explain GB-scale
  RSS; the events-tailer backlog drain (kata myap, fixed in 32463af5) is a
  better-matching suspect — never call this fix the "leading suspect". Do
  NOT extrapolate numbers that were not measured, and do NOT recommend
  closing 1bt7 unless the measurements are decisive on their face (e.g.
  multi-second rounds / GB-scale heap deltas at realistic scale).

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

### Task 5: Land — push, HALT for PR approval, then PR, merge, kata close

Plan review (V5) checked the actual kata v4rw task text: it contains NO
landing/approval language, and AGENTS.md explicitly forbids `gh pr create`
without the user's explicit approval for the branch/change. So: push freely,
then STOP and ask the user before creating the PR. Once PR creation is
approved, merging after green checks follows the repo norm (`main` has no
server-side branch protection — treat the reported CI checks as mandatory
anyway; never merge red).

**Files:** none modified — git/gh/kata operations only.

**Interfaces:**
- Consumes: green verification from Task 4; `benchmark-results.md` from Task 3.
- Produces: merged PR on `main`, fast-forwarded local `main`, closed kata v4rw.

- [ ] **Step 1: Push the branch**

```bash
cd /home/dan/code/freshell/.worktrees/indexer-mtime-reparse
git push -u origin fix/indexer-mtime-reparse
```

- [ ] **Step 2: HALT — request explicit user approval to create the PR**

Present to the user: the branch name, `git log --oneline origin/main..HEAD`,
the test results from Task 4, and the conclusions section of
`benchmark-results.md`. Ask for explicit approval to run `gh pr create` (and
to squash-merge once checks are green). Do NOT proceed to Step 3 without that
explicit approval — if approval is denied or unavailable, stop here: the
pushed branch is the deliverable, and the kata stays open with a note.

- [ ] **Step 3: Open the PR (gh identity dan@danshapiro.com)**

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

- [ ] **Step 4: Wait for the reported CI checks, then squash-merge**

```bash
# CI runs can lag PR creation: `--watch` exits early with "no checks reported"
# if run immediately (validated during plan review). Retry until checks report.
for i in $(seq 1 10); do
  gh pr checks fix/indexer-mtime-reparse --watch && break
  sleep 30
done
gh pr merge fix/indexer-mtime-reparse --squash --delete-branch
```

Expected: the three reported workflows (clippy / contract / typecheck-client)
pass before merge. Note `main` has NO branch protection, so nothing is
server-enforced — treat the reported checks as mandatory anyway. If a check
fails, fix in the worktree, push, and re-watch — do not merge red.

- [ ] **Step 5: Fast-forward local main**

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

- [ ] **Step 6: Close kata v4rw with evidence**

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
5. **Load-bearing validation pass (post-plan review, ledger in `.the-usual-logs/indexer-mtime-reparse/load-bearing-ledger.md`):** 8 assumptions validated by evidence (5 parallel validators; V1–V5 reports). Verified: the fold is a semantic no-op (Amplifier pairs transcript writes with immediate metadata.json rewrites, so enrichment staleness self-heals); in-place mutation of `cached.baseSession` is safe (single clone boundary at `applyOverride:212-219`, no identity-based change detection); the benchmark harness seams all work via vendored tsx. Falsified and corrected in this plan: (a) the trigger-path premise — sidecar ticks never schedule refreshes; the pre-fix re-parse fired per warm full rescan (default 10 min), not per 5s refresh — severity narrative, test comments, commit message, and benchmark framing rewritten, "quadratic"/"leading OOM suspect" claims removed; (b) "metadata.json only changes on name/description updates" — it is rewritten every turn end and on tool:post — Task 2 replacement comments reworded; (c) benchmark cold start parses only `ENRICHMENT_BATCH_SIZE=150` of 200 fixtures — warm-up sweep added and expected shape corrected, `node_modules/.bin/tsx` mandated over bare `npx tsx`; (d) the claimed landing pre-approval does not exist in the kata v4rw text and AGENTS.md forbids unapproved `gh pr create` — Task 5 now halts after push for explicit user approval, and the checks step gains a "no checks reported" retry guard (main is unprotected; checks are convention, not server-enforced). Re-ran the self-review items above over every edited task: no new stubs/placeholders introduced; all edits are narrative/command corrections, and Task 1's code steps are unchanged except comment wording.
