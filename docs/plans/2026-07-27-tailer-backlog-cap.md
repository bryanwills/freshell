# Tailer Live-Backlog Cap (Skip-to-Tail) Implementation Plan

> **For agentic workers:** This plan is executed task-by-task by the
> workflow's execute stage: a fresh implementer per task, with a spec +
> quality review after each task. Steps use checkbox (`- [ ]`) syntax
> for tracking.

**Goal:** Guard the amplifier events tailer against OOM by skipping to EOF (instead of draining) when the live offset→EOF backlog exceeds a byte cap — the same policy already used for attach catch-up.

**Architecture:** A single guard inside `readAppended()` in `server/coding-cli/amplifier-events-tailer.ts`: after the existing stat/shrink checks and before the batch drain loop, if `size - offset > AMPLIFIER_TAILER_BACKLOG_MAX_BYTES`, adopt state from the tail (jump `offset` to `size`, clear the partial-line buffer and the oversized-line-skip flag), emit one structured `warn`, and return an ok/empty result. Because both `read()` and `forceRead()` funnel into `readAppended()`, one guard covers every live drain path (chokidar watcher events, Enter-keypress force-read, deadman force-read). A second, small change plumbs the integration's existing `warn` logger into the tailer so the skip is visible in production logs.

**Tech Stack:** TypeScript (NodeNext/ESM — relative imports need `.js` extensions), Vitest (server config), in-memory fake `fs` injection (existing test harness pattern; no real files).

## Global Constraints

- Kata issue: `myap` — "OOM fix 1: cap live backlog drain in amplifier-events-tailer (skip-to-tail)" (P1, labels: oom, perf).
- Worktree: `/home/dan/code/freshell/.worktrees/tailer-backlog-cap`, branch `fix/tailer-backlog-cap`, PR targets `main`. All commands below run from the worktree root.
- Red-Green-Refactor TDD: write the failing test, watch it fail, minimal implementation, watch it pass, refactor, stay green.
- Server test runs use the coordinated repo-owned path: `npm run test:vitest -- run <file> --config config/vitest/vitest.server.config.ts` (the `--config` flag is REQUIRED — the default config excludes `test/unit/server/**` and silently matches zero tests).
- Never restart the self-hosted Freshell server; never use broad kill patterns (`pkill -f node` etc.).
- Git commits use the repo's Amplifier co-author footer (shown verbatim in each commit step).
- `gh` operations use the `dan@danshapiro.com` identity.
- The user has EXPLICITLY pre-approved PR creation and merge for this change (landing steps are in Task 3).
- Scope guard: do NOT implement related issues `v4rw` (session-indexer re-parse) or `tkd9` (Buffer prefilter before string decode) beyond what the cap requires.

## Design Decisions (read before Task 1)

These were settled during planning from a full read of the tailer, the integration, the tracker, and both test files:

1. **Guard lives inside `readAppended()`** (per the issue text: `amplifier-events-tailer.ts:148-154`). A call-site variant (in the integration's `pump()`) was considered and rejected: `offset` is tailer-private state mutated inside a serialized promise chain, so an outside `getOffset()` + stat is racy against in-flight reads, and there are three independent drivers (watcher, submit-grace force-read, deadman force-read) — one guard inside the tailer covers all of them atomically with the stat it already performs.
2. **Cap value: `64 MiB`** (`AMPLIFIER_TAILER_BACKLOG_MAX_BYTES = 64 * 1024 * 1024`), exported hard-coded const (house style — no env var; matches `AMPLIFIER_CATCHUP_MAX_BYTES`, `AMPLIFIER_TAILER_PARTIAL_MAX_BYTES`, `AMPLIFIER_TAILER_READ_BATCH_MAX_BYTES`). Why 64 MiB and not the attach cap's 4 MiB: the cap must stay STRICTLY GREATER than `AMPLIFIER_TAILER_READ_BATCH_MAX_BYTES` (16 MiB) or the multi-batch drain loop becomes dead code and the existing batch-boundary test (`amplifier-events-tailer.test.ts:289-307`, which fully drains a ~16 MiB append) breaks. 64 MiB = at most 4 read batches of mostly prefilter-rejected noise — bounded event-loop work — while the pathological WSL2 backlogs this guards against are 100s of MB (events files reach 1.1 GB). Normal live gaps are KBs, so the guard is a no-op in normal operation, exactly as the issue requires.
3. **Skip semantics = attach-at-EOF parity:** jump `offset` to raw `size` (no newline alignment — same as `attach()` with `attachAt: 'eof'`; a torn line at the jump point fails the prefilter/JSON.parse later and counts as one skipped line), clear `partial`, clear `skippingOversizedLine`. Do NOT set `degraded` (the lane stays healthy), do NOT touch `schemaValidated` (the gate runs on the first post-jump record), do NOT change the `AmplifierTailerReadResult` shape (the integration's `pump()` treats ok+zero-records as a no-op, which is the desired "stay silent, live records take over" behavior — parity with the attach cap, which leaves the tracker idle). Missing activity signals are acceptable: `events.jsonl` is write-only observability output that Amplifier never reads back, so skip-to-tail can never lose data that matters (worst case: a missed turn-complete chime; the deadman force-read self-heals phase within its window).
4. **Log at `warn`, plumbed from the integration.** The attach cap logs `warn` via the integration logger; the tailer's log slot today is `debug`-only AND production never passes a logger to the tailer at all (`amplifier-activity-integration.ts:336-340`). So: widen the tailer's `log` input type to `{ debug?; warn? }`, emit the skip at `warn?.()`, and pass `{ warn: log.warn }` through at the construction site. Event name `amplifier_tailer_backlog_skipped` with `{ component, event, filePath, backlogBytes, capBytes }` — same shape family as `amplifier_events_catchup_skipped`. One warn per skip occurrence (no latch): each trigger is a distinct multi-64-MiB anomaly worth a log line, and a single skip resolves the whole backlog so there is no per-line spam.
5. **Test level:** unit tests against the tailer's injected fake fs prove the mechanism; one integration-level test drives the full watcher→pump→tailer→tracker path and proves the production-visible warn plus live recovery. This is the highest level of abstraction available for a server-internal guard rail — there is no user-visible UI surface for a 64 MiB backlog skip, so no e2e-browser test applies (and no `docs/index.html` change: not a user-facing feature).

---

### Task 1: Backlog cap in the tailer (skip-to-tail)

**Files:**
- Modify: `server/coding-cli/amplifier-events-tailer.ts` (constants block ~line 37, `log` input type ~line 105, `readAppended()` ~line 144, `attach()` ~line 246)
- Test: `test/unit/server/coding-cli/amplifier-events-tailer.test.ts` (append a new `describe` block at the end of the file)

**Interfaces:**
- Consumes: existing exports of `amplifier-events-tailer.ts` (`createAmplifierEventsTailer`, `AMPLIFIER_TAILER_PARTIAL_MAX_BYTES`) and the test file's existing helpers `line()`, `createFakeFs()`, `okRecords()` (already defined at the top of the test file — do not redefine them).
- Produces: exported `const AMPLIFIER_TAILER_BACKLOG_MAX_BYTES = 64 * 1024 * 1024` (Task 2's test imports it); widened tailer input `log?: { debug?: (payload: object, message?: string) => void; warn?: (payload: object, message?: string) => void }` (Task 2's implementation relies on `warn` being accepted); skip behavior: a read whose backlog exceeds the cap returns `{ ok: true, records: [], skippedLines: 0, bytesConsumed: 0, offset: <file size> }`, performs zero positional reads, and emits one `warn`.

- [ ] **Step 1: Write the main failing regression test**

Append to `test/unit/server/coding-cli/amplifier-events-tailer.test.ts` (after the last existing `describe` block closes; the helpers `line`, `createFakeFs`, `okRecords` and the vitest imports are already at the top of the file). Also add `AMPLIFIER_TAILER_BACKLOG_MAX_BYTES` to the existing import list from `'../../../../server/coding-cli/amplifier-events-tailer.js'`:

```ts
describe('live backlog cap (skip-to-tail)', () => {
  it('skips to EOF instead of draining a backlog beyond the cap; live records take over', async () => {
    // OOM regression (kata myap): a silently-dead WSL2 watcher lets the
    // backlog grow to 100s of MB; a force-read must never drain it all.
    const noise = line('content_block:start')
    const backlog = noise.repeat(
      Math.ceil((AMPLIFIER_TAILER_BACKLOG_MAX_BYTES + 64 * 1024) / noise.length),
    )
    const fsImpl = createFakeFs(backlog)
    const warn = vi.fn()
    const tailer = createAmplifierEventsTailer({
      filePath: '/fake/events.jsonl',
      fsImpl,
      attachAt: 'start',
      log: { warn },
    })
    await tailer.attach()

    const result = await tailer.read()
    expect(okRecords(result)).toEqual([])
    if (!result.ok) throw new Error('unreachable')
    // State adopted from the tail: offset jumps to EOF without reading.
    expect(result.offset).toBe(Buffer.byteLength(backlog))
    expect(result.bytesConsumed).toBe(0)
    expect(tailer.getOffset()).toBe(Buffer.byteLength(backlog))
    // The whole point: NO positional reads, no bytes decoded.
    expect(fsImpl.readCalls).toHaveLength(0)
    expect(warn).toHaveBeenCalledTimes(1)
    expect(warn.mock.calls[0][0]).toMatchObject({
      component: 'amplifier-events-tailer',
      event: 'amplifier_tailer_backlog_skipped',
      filePath: '/fake/events.jsonl',
      backlogBytes: Buffer.byteLength(backlog),
      capBytes: AMPLIFIER_TAILER_BACKLOG_MAX_BYTES,
    })

    // Live records take over from the adopted tail; the lane never degraded.
    fsImpl.append(line('prompt:submit'))
    const second = await tailer.read()
    expect(okRecords(second)).toEqual(['prompt:submit'])
  })
})
```

- [ ] **Step 2: Run the test to verify it fails**

Run:
```bash
npm run test:vitest -- run test/unit/server/coding-cli/amplifier-events-tailer.test.ts --config config/vitest/vitest.server.config.ts
```
Expected: FAIL. Without the cap the tailer drains the whole ~64 MiB in 64 KiB positional reads, so `expect(fsImpl.readCalls).toHaveLength(0)` fails with a length in the hundreds, and the `warn` assertion fails with 0 calls. (Vitest does not typecheck, so the not-yet-widened `log` type does not block the run.) If the test PASSES here, stop — something is wrong with the test.

- [ ] **Step 3: Minimal implementation — constant, widened log type, guard**

In `server/coding-cli/amplifier-events-tailer.ts`, make three edits.

(a) After the `AMPLIFIER_TAILER_READ_BATCH_MAX_BYTES` constant (line ~37), add:

```ts
/**
 * Cap on the live offset->EOF backlog a single read will drain (kata myap,
 * OOM fix 1). READ_BATCH_MAX_BYTES bounds one Buffer.concat; this bounds the
 * TOTAL drain: past it, the tailer skips to EOF and adopts state from the
 * tail -- the exact policy the integration already uses for attach catch-up
 * (AMPLIFIER_CATCHUP_MAX_BYTES). events.jsonl is write-only observability
 * output (Amplifier never reads it back), so the worst case is missed
 * activity signals, never data loss. Must stay strictly greater than
 * READ_BATCH_MAX_BYTES so multi-batch draining below the cap still happens.
 */
export const AMPLIFIER_TAILER_BACKLOG_MAX_BYTES = 64 * 1024 * 1024
```

(b) Widen the `log` input type in `createAmplifierEventsTailer` (line ~105) from:

```ts
  log?: { debug?: (payload: object, message?: string) => void }
```

to:

```ts
  log?: {
    debug?: (payload: object, message?: string) => void
    warn?: (payload: object, message?: string) => void
  }
```

(c) In `readAppended()`, insert the guard between the `size === offset` early return (lines ~144-146) and the `const records: AmplifierParsedRecord[] = []` line (~148). MINIMAL version first — state-reset lines are driven in by the tests in Steps 5-8:

```ts
    if (size - offset > AMPLIFIER_TAILER_BACKLOG_MAX_BYTES) {
      // Live-drain cap (kata myap): a silently-dead watcher (WSL2 inotify)
      // can leave 100s of MB between offset and EOF; draining it decodes
      // every line to a JS string and OOMs the process. Skip to EOF and
      // adopt state from the tail instead -- attach-cap parity.
      const backlogBytes = size - offset
      offset = size
      input.log?.warn?.({
        component: 'amplifier-events-tailer',
        event: 'amplifier_tailer_backlog_skipped',
        filePath,
        backlogBytes,
        capBytes: AMPLIFIER_TAILER_BACKLOG_MAX_BYTES,
      }, 'Amplifier events backlog exceeded the live-drain cap; skipping to EOF (live records take over).')
      return { ok: true, records: [], skippedLines: 0, bytesConsumed: 0, offset }
    }
```

- [ ] **Step 4: Run the test to verify it passes**

Run:
```bash
npm run test:vitest -- run test/unit/server/coding-cli/amplifier-events-tailer.test.ts --config config/vitest/vitest.server.config.ts
```
Expected: PASS — including every pre-existing test in the file (in particular `reads large appends in bounded batches...`, which drains ~16 MiB and must stay green because 16 MiB < 64 MiB).

- [ ] **Step 5: Write the failing partial-buffer-reset test**

The skip must drop any buffered partial line — those bytes belong to a line whose continuation was just skipped over, and gluing them onto post-jump bytes corrupts the next record. Append inside the same `describe('live backlog cap (skip-to-tail)')` block:

```ts
  it('drops a buffered partial line when skipping to tail', async () => {
    // 40 bytes of an incomplete record (no newline yet) get buffered...
    const partialHead = line('prompt:submit').slice(0, 40)
    const fsImpl = createFakeFs(partialHead)
    const tailer = createAmplifierEventsTailer({
      filePath: '/fake/events.jsonl',
      fsImpl,
      attachAt: 'start',
    })
    await tailer.attach()
    await tailer.read()
    expect(tailer.getBufferedBytes()).toBe(40)

    // ...then the backlog explodes past the cap: the skip must discard them.
    fsImpl.append('x'.repeat(AMPLIFIER_TAILER_BACKLOG_MAX_BYTES + 1024) + '\n')
    const skipped = await tailer.read()
    expect(okRecords(skipped)).toEqual([])
    expect(tailer.getBufferedBytes()).toBe(0)

    // A fresh post-jump record parses cleanly (no stale-prefix corruption).
    fsImpl.append(line('prompt:complete'))
    expect(okRecords(await tailer.read())).toEqual(['prompt:complete'])
  })
```

- [ ] **Step 6: Run it to verify it fails, then make it pass**

Run the same command as Step 4. Expected: the new test FAILS at `expect(tailer.getBufferedBytes()).toBe(0)` (buffer still holds 40) — and the final assertion would yield `[]` because the stale 40-byte prefix corrupts the `prompt:complete` line into unparseable JSON.

Then add ONE line to the guard block in `readAppended()`, directly after `offset = size`:

```ts
      partial = Buffer.alloc(0)
```

Re-run the same command. Expected: PASS (all tests).

- [ ] **Step 7: Write the failing oversized-line-flag-reset test**

If a skip happens while the tailer is mid-oversized-line discard (`skippingOversizedLine === true`), the flag must be cleared — otherwise the first post-jump bytes are silently discarded up to their first newline (double data loss). Append inside the same `describe` block:

```ts
  it('clears oversized-line skip mode when skipping to tail', async () => {
    // Overflow the partial-line cap with a never-ending line: the tailer
    // enters skippingOversizedLine mode (existing behavior).
    const oversized = 'x'.repeat(AMPLIFIER_TAILER_PARTIAL_MAX_BYTES + 1024)
    const fsImpl = createFakeFs(oversized)
    const tailer = createAmplifierEventsTailer({
      filePath: '/fake/events.jsonl',
      fsImpl,
      attachAt: 'start',
    })
    await tailer.attach()
    await tailer.read()

    // Backlog explodes past the cap while still mid-oversized-line.
    fsImpl.append('y'.repeat(AMPLIFIER_TAILER_BACKLOG_MAX_BYTES + 1024))
    await tailer.read()

    // If the flag survived the jump, this whole record would be discarded up
    // to its trailing newline and counted as a skipped line instead.
    fsImpl.append(line('prompt:submit'))
    const result = await tailer.read()
    expect(okRecords(result)).toEqual(['prompt:submit'])
    if (!result.ok) throw new Error('unreachable')
    expect(result.skippedLines).toBe(0)
  })
```

- [ ] **Step 8: Run it to verify it fails, then make it pass**

Run the same command as Step 4. Expected: the new test FAILS — `okRecords(result)` is `[]` and `skippedLines` is `1` (the flag ate the record).

Then add ONE line to the guard block, directly after `partial = Buffer.alloc(0)`:

```ts
      skippingOversizedLine = false
```

Re-run the same command. Expected: PASS (all tests).

- [ ] **Step 9: Write the no-op-below-cap regression guard test**

This pins the strict `>` comparison and the "no-op in normal operation" requirement (it should pass immediately — it guards against a future `>=`/off-by-one or a cap accidentally set at or below the batch size). Append inside the same `describe` block:

```ts
  it('drains a just-below-cap backlog fully (the guard is a no-op below the cap)', async () => {
    const noise = line('content_block:start')
    const filler = noise.repeat(
      Math.floor((AMPLIFIER_TAILER_BACKLOG_MAX_BYTES - 64 * 1024) / noise.length),
    )
    const fsImpl = createFakeFs(line('session:start') + filler + line('prompt:complete'))
    const warn = vi.fn()
    const tailer = createAmplifierEventsTailer({
      filePath: '/fake/events.jsonl',
      fsImpl,
      attachAt: 'start',
      log: { warn },
    })
    await tailer.attach()

    const result = await tailer.read()
    expect(okRecords(result)).toEqual(['session:start', 'prompt:complete'])
    expect(warn).not.toHaveBeenCalled()
  })
```

Run the same command as Step 4. Expected: PASS on first run (this one is a regression guard, not a RED step — Steps 1-8 already did the RED-GREEN cycles for the new behavior). If it FAILS, the comparison or cap value is wrong — fix the implementation, not the test.

- [ ] **Step 10: Refactor — deduplicate the tail-adoption reset**

`attach()` (lines ~246-249) and the new guard now share the same "reset line state" moves (`partial = Buffer.alloc(0)`; `skippingOversizedLine = false`). Extract a tiny helper next to the `degrade` helper inside `createAmplifierEventsTailer`:

```ts
  const resetLineState = (): void => {
    partial = Buffer.alloc(0)
    skippingOversizedLine = false
  }
```

Use it in both places. In `attach()`, replace:

```ts
        partial = Buffer.alloc(0)
        skippingOversizedLine = false
```

with:

```ts
        resetLineState()
```

In the guard block, replace the two lines added in Steps 6 and 8 with `resetLineState()` (keep `offset = size` where it is). Also update the file-header doc comment (lines 1-13) by adding one sentence to the contract description: after "validates the schema once per file;" insert "a live backlog beyond `AMPLIFIER_TAILER_BACKLOG_MAX_BYTES` is skipped, not drained (state adopts the tail);".

Run the same command as Step 4 plus the typecheck:

```bash
npm run test:vitest -- run test/unit/server/coding-cli/amplifier-events-tailer.test.ts --config config/vitest/vitest.server.config.ts
npm run typecheck:server
```
Expected: tests PASS, typecheck clean.

- [ ] **Step 11: Commit**

```bash
git add server/coding-cli/amplifier-events-tailer.ts test/unit/server/coding-cli/amplifier-events-tailer.test.ts
git commit -m "$(cat <<'EOF'
fix(server): cap live backlog drain in amplifier events tailer (kata myap)

A silently-dead chokidar watcher (WSL2 inotify) lets the events.jsonl
offset->EOF gap grow to 100s of MB; the next read (watcher event or
force-read) drained it all, decoding every line to a JS string and
OOMing at the 4GB heap limit. readAppended() now skips to EOF and
adopts state from the tail when the backlog exceeds 64MiB -- the same
policy the integration already applies to attach catch-up. events.jsonl
is write-only observability output, so the worst case is missed
activity signals, never data loss.

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>
EOF
)"
```

---

### Task 2: Production log visibility — plumb the integration logger into the tailer

**Files:**
- Modify: `server/coding-cli/amplifier-activity-integration.ts:336-340` (the `createAmplifierEventsTailer` construction inside `doAttachInner`)
- Test: `test/unit/server/coding-cli/amplifier-activity-integration.test.ts` (append one test inside the existing top-level `describe('amplifier activity integration')` block)

**Interfaces:**
- Consumes: `AMPLIFIER_TAILER_BACKLOG_MAX_BYTES` (exported by Task 1 from `amplifier-events-tailer.ts`); the widened tailer `log` input `{ debug?; warn? }` (Task 1); the integration test harness helpers already defined at the top of the test file: `setup()` (returns `{ registry, tracker, completions, fsStore, watchers, warn, integration }` with `log: { warn }` pre-wired into the integration), `bound(registry, input)`, `flush()`, `line(event, atMs)`, `EVENTS_PATH`.
- Produces: production behavior — the integration's `log.warn` reaches the tailer, so `amplifier_tailer_backlog_skipped` is visible in server logs (the integration is constructed with the real server logger in production wiring; tests prove the pass-through).

- [ ] **Step 1: Write the failing integration test**

Append inside the existing `describe('amplifier activity integration', ...)` block in `test/unit/server/coding-cli/amplifier-activity-integration.test.ts`. Add this import line after the existing import of `AmplifierTailerFs` (line ~14):

```ts
import { AMPLIFIER_TAILER_BACKLOG_MAX_BYTES } from '../../../../server/coding-cli/amplifier-events-tailer.js'
```

The test (uses the canonical harness rhythm: `fsStore.append` → `watchers[0].fire('change', ...)` → `await flush()`):

```ts
  it('caps a live backlog: one production-visible warn, skip to EOF, live records take over', async () => {
    const { registry, tracker, completions, fsStore, watchers, warn } = setup()
    fsStore.write(EVENTS_PATH, line('session:start', 1000))
    bound(registry, { reason: 'association' })
    await flush()
    expect(tracker.getActivity('t1')?.phase).toBe('idle')
    expect(warn).not.toHaveBeenCalled()

    // The watcher went silent (WSL2) while the CLI kept writing; the next
    // event sees a giant offset->EOF gap. Must skip, never drain (OOM).
    fsStore.append(EVENTS_PATH, 'x'.repeat(AMPLIFIER_TAILER_BACKLOG_MAX_BYTES + 1024) + '\n')
    watchers[0].fire('change', EVENTS_PATH)
    await flush()
    expect(warn).toHaveBeenCalledTimes(1)
    expect(warn.mock.calls[0][0]).toMatchObject({
      component: 'amplifier-events-tailer',
      event: 'amplifier_tailer_backlog_skipped',
      filePath: EVENTS_PATH,
      capBytes: AMPLIFIER_TAILER_BACKLOG_MAX_BYTES,
    })
    // The skip is silent toward the tracker: no phase change, no completion,
    // no degrade (attach-cap parity).
    expect(tracker.getActivity('t1')?.phase).toBe('idle')
    expect(completions).toHaveLength(0)
    expect(watchers[0].closed).toBe(false)

    // Live records take over from the adopted tail.
    fsStore.append(EVENTS_PATH, line('prompt:submit', 9000))
    watchers[0].fire('change', EVENTS_PATH)
    await flush()
    expect(tracker.getActivity('t1')?.phase).toBe('busy')

    fsStore.append(EVENTS_PATH, line('prompt:complete', 10000))
    watchers[0].fire('change', EVENTS_PATH)
    await flush()
    expect(tracker.getActivity('t1')?.phase).toBe('idle')
    expect(completions).toHaveLength(1)
    // Still exactly one warn: the skip logged once, recovery logged nothing.
    expect(warn).toHaveBeenCalledTimes(1)
  })
```

- [ ] **Step 2: Run the test to verify it fails**

Run:
```bash
npm run test:vitest -- run test/unit/server/coding-cli/amplifier-activity-integration.test.ts --config config/vitest/vitest.server.config.ts
```
Expected: FAIL at `expect(warn).toHaveBeenCalledTimes(1)` (received 0) — the tailer skips (Task 1) but the integration never passes it a logger, so the warn is invisible. This is exactly the production gap being fixed. Every pre-existing test in the file must still pass.

- [ ] **Step 3: Minimal implementation — pass the logger through**

In `server/coding-cli/amplifier-activity-integration.ts`, inside `doAttachInner`, change the tailer construction (lines ~336-340) from:

```ts
      tailer: createAmplifierEventsTailer({
        filePath: eventsPath,
        attachAt,
        ...(fsImpl ? { fsImpl } : {}),
      }),
```

to:

```ts
      tailer: createAmplifierEventsTailer({
        filePath: eventsPath,
        attachAt,
        ...(fsImpl ? { fsImpl } : {}),
        // Kata myap: surface tailer-level warns (live backlog skip) through
        // the integration's production logger.
        ...(log ? { log: { warn: log.warn } } : {}),
      }),
```

(`log` is already in scope — it is destructured from `input` at the top of `createAmplifierActivityIntegration` and used by the attach-cap warn a few lines above.)

- [ ] **Step 4: Run tests to verify they pass**

Run:
```bash
npm run test:vitest -- run test/unit/server/coding-cli/amplifier-activity-integration.test.ts test/unit/server/coding-cli/amplifier-events-tailer.test.ts --config config/vitest/vitest.server.config.ts
npm run typecheck:server
```
Expected: all tests in both files PASS; typecheck clean. (Refactor check: the three-line spread is the whole change — nothing to extract; the existing test at `:503-536` that omits `log` entirely keeps the optional path covered.)

- [ ] **Step 5: Commit**

```bash
git add server/coding-cli/amplifier-activity-integration.ts test/unit/server/coding-cli/amplifier-activity-integration.test.ts
git commit -m "$(cat <<'EOF'
fix(server): surface tailer backlog-skip warn through the integration logger (kata myap)

The tailer's log input was never wired in production, so the new
amplifier_tailer_backlog_skipped warn (and any future tailer-level
diagnostics) would be invisible. Pass the integration's warn logger
through at tailer construction; integration test drives the full
watcher->pump->tailer->tracker path and asserts the warn, the silent
skip (no phase change, no completion, no degrade), and live recovery.

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>
EOF
)"
```

---

### Task 3: Full-suite verification and landing (PR + merge + kata close — pre-approved)

**Files:**
- No source changes. Verification and landing only.

**Interfaces:**
- Consumes: the two commits from Tasks 1-2 on branch `fix/tailer-backlog-cap`.
- Produces: a merged PR on `main`, local `main` fast-forwarded, kata `myap` closed with the merged commit SHA.

- [ ] **Step 1: Run the coordinated full suite**

The user has pre-approved landing, and the repo requires green required checks anyway — run the full coordinated suite first (this waits on the shared coordinator gate if another agent holds it; wait, never kill a foreign holder):

```bash
FRESHELL_TEST_SUMMARY="tailer-backlog-cap: full-suite gate before PR (kata myap)" npm run check
```

Expected: typecheck clean, full suite green (exit code 0). If a failure appears, first check whether it reproduces on the base commit (`git stash` any dirty state; the suite must be green including our change — fix forward if our change caused it; if it is a pre-existing flake unrelated to `server/coding-cli/`, re-run once and note it in the PR description).

- [ ] **Step 2: Push the branch and open the PR (explicitly pre-approved)**

PR creation and merge were EXPLICITLY pre-approved by the user for this change — the usual stop-before-`gh pr create` rule is waived for this branch only.

```bash
git push -u origin fix/tailer-backlog-cap
gh pr create --base main --title "fix(server): cap live backlog drain in amplifier-events-tailer (skip-to-tail, kata myap)" --body "$(cat <<'EOF'
## Summary
- OOM fix 1 (kata `myap`, P1, oom/perf): `readAppended()` in `server/coding-cli/amplifier-events-tailer.ts` drained the entire offset->EOF gap with no total-bytes cap; with a silently-dead WSL2 watcher the backlog reaches 100s of MB (events files hit 1.1GB) and a force-read drained it all at once, starving the event loop and OOMing at the 4GB heap limit (confirmed via perf_system heap telemetry: 1.5GB spike at 23:01 preceded by amplifier_events_lane_suspect at 22:56).
- Guard rail, no-op in normal operation: if `size - offset > AMPLIFIER_TAILER_BACKLOG_MAX_BYTES` (64MiB), skip to EOF, warn once, adopt state from the tail — the exact policy already used for attach catch-up (`AMPLIFIER_CATCHUP_MAX_BYTES`). Covers all three live drivers (watcher event, submit-grace force-read, deadman force-read) since both `read()` and `forceRead()` funnel into `readAppended()`.
- Semantically safe: `events.jsonl` is write-only observability output — Amplifier never reads it back (resume uses transcript.jsonl+metadata.json). Worst case is missed activity/turn-complete signals, never data loss.
- Also plumbs the integration's `warn` logger into the tailer (it was never wired), making `amplifier_tailer_backlog_skipped` production-visible.
- 64MiB (not the attach cap's 4MiB) keeps the cap strictly above `AMPLIFIER_TAILER_READ_BATCH_MAX_BYTES` (16MiB) so bounded multi-batch draining below the cap is preserved along with its existing test.

## Test plan
- [x] TDD unit tests: skip-to-tail regression (zero positional reads on a >64MiB backlog, warn payload, offset adoption, live recovery), partial-buffer reset, oversized-line-flag reset, below-cap no-op guard.
- [x] Integration test: full watcher->pump->tailer->tracker path — one production-visible warn, silent skip (no phase change / completion / degrade), live records take over.
- [x] `FRESHELL_TEST_SUMMARY=... npm run check` (typecheck + coordinated full suite) green.

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)
EOF
)"
```

Expected: PR URL printed.

- [ ] **Step 3: Wait for required checks, then squash-merge**

```bash
gh pr checks --watch
```
Expected: all required checks pass. Then (squash merge is the repo norm; self-merging is the norm):

```bash
gh pr merge --squash --delete-branch
```
Expected: merged. If checks fail, fix forward on the branch (returning to TDD for any code change) and re-push; do not merge red.

- [ ] **Step 4: Fast-forward local main**

From the main checkout (NOT the worktree — and fast-forward only; if it cannot fast-forward, stop and resolve explicitly rather than creating a merge commit):

```bash
git -C /home/dan/code/freshell checkout main
git -C /home/dan/code/freshell pull --ff-only origin main
```
Expected: local `main` now contains the squashed commit. Record its SHA:

```bash
git -C /home/dan/code/freshell log --oneline -1
```

- [ ] **Step 5: Close the kata with evidence**

Only if everything above is fully verified (merged, checks green). From the repo root, with `<merged-sha>` from Step 4:

```bash
cd /home/dan/code/freshell && kata close myap --done --commit <merged-sha> --message "Live backlog drain in amplifier-events-tailer is now capped at AMPLIFIER_TAILER_BACKLOG_MAX_BYTES (64MiB): readAppended() skips to EOF, resets partial-line state, and warns (amplifier_tailer_backlog_skipped, now production-visible via the integration logger) instead of draining 100s-of-MB backlogs after silent WSL2 watcher death — attach-cap parity, no-op below the cap. TDD: 4 tailer unit tests + 1 integration test; full coordinated suite green; merged to main."
```

Expected: kata `myap` closed. Do NOT close it if any prior step is unverified.

---

## Self-Review (performed at planning time)

**1. Spec coverage:**
- "if size - offset > cap, skip to EOF" → Task 1 Steps 1-4 (strict `>` pinned by Step 9's below-cap test).
- "log once" → one structured warn per skip (Task 1 Step 3); production visibility gap closed in Task 2 (the issue's intent is an observable guard rail — an invisible log would be a silent deferral).
- "adopt state from the tail — the exact policy already used for attach" → offset jump + `partial`/`skippingOversizedLine` reset, no degrade, tracker stays silent (Task 1 Steps 5-8; attach-cap parity asserted in Task 2's integration test).
- "no-op in normal operation" → 64 MiB cap + Task 1 Step 9 below-cap drain test + Task 2's assertion that recovery reads log nothing.
- "TDD with synthetic large-backlog regression test" → Task 1 Step 1 is exactly that (synthetic ~64 MiB backlog, in-memory fs, asserts zero positional reads).
- Repo conventions (worktree, coordinated tests, `.js` imports, co-author footer, no server restarts, no broad kills) → Global Constraints + every command uses the coordinated path.
- Landing instructions (pre-approved PR, checks, squash merge, ff main, kata close with evidence) → Task 3.
- Scope exclusions (`v4rw`, `tkd9`) → Global Constraints; no task touches the session-indexer or converts the prefilter to Buffer-level.

**1b. No silent deferrals:** No stubs, mocks-of-behavior-under-test, or deferred requirements. The fake `fsImpl` is the file's established injection seam (existing harness), used to isolate the filesystem, not to fake the behavior under test; the integration test exercises the real tailer, real reducer, real tracker through the production pump path. Production log visibility is implemented (Task 2), not deferred. No UNRESOLVED COVERAGE GAPS.

**2. Placeholder scan:** No TBD/TODO/"handle edge cases"/"similar to Task N" — every code step shows complete code; every run step has the exact command and expected outcome.

**3. Type consistency:** `AMPLIFIER_TAILER_BACKLOG_MAX_BYTES` is defined in Task 1(a) and imported with that exact name in Task 2 Step 1. The widened `log` type `{ debug?; warn? }` (Task 1(b)) accepts Task 2 Step 3's `{ warn: log.warn }` (both `(payload: object, message?: string) => void`). The guard's return `{ ok: true, records: [], skippedLines: 0, bytesConsumed: 0, offset }` matches the existing `AmplifierTailerReadResult` ok-variant exactly (no shape change). `resetLineState()` (Task 1 Step 10) is used only within the tailer file.
