# Freshclaude Client-Side Durable Identity Persistence (P0.2 Wall Pin Flip) Implementation Plan

> **For agentic workers:** This plan is executed task-by-task by the
> workflow's execute stage: a fresh implementer per task, with a spec +
> quality review after each task. Steps use checkbox (`- [ ]`) syntax
> for tracking.

**Goal:** Make a freshclaude/kilroy pane's durable CLI session id survive a browser reload (via persisted `content.sessionRef`), so the pane resumes the SAME conversation after reload + server SIGKILL restart — closing the last expected-fail pin (P0.2) in the restore contract wall.

**Architecture:** The client already LEARNS the durable id (`cliSessionId` on `freshAgent.session.init` / `freshAgent.session.metadata` events) but folds it only into the unpersisted `freshAgentSlice`. We add one panesSlice reducer (`adoptFreshAgentDurableIdentity`) that folds the durable id into pane content as `sessionRef` + `resumeSessionId` (canonical-UUID-gated), wire it from the two ws-transport learn sites, and flush persistence. `persistMiddleware` already round-trips `sessionRef` untouched — the live placeholder in `content.sessionId` stays stripped, exactly as the durable-session contract requires. Then we flip the wall pin.

**Tech Stack:** React + Redux Toolkit (immer) client, Vitest units (via repo test coordinator), Playwright e2e against per-test `RustServer` instances with the fake claude sidecar fixture.

## Global Constraints

- Worktree: `/home/dan/code/freshell/.worktrees/freshclaude-identity-persistence` (branch `freshclaude-identity-persistence`). ALL work happens here.
- Base: CURRENT `origin/main` — fetch first; `~3f096412` or newer (at plan time `origin/main` is `7508149b`).
- e2e servers: every spec owns its `RustServer` instances on ephemeral ports via `findFreePort()`; **NEVER ports 3001/3002** — the user's LIVE server is on 3002.
- **NEVER restart the user's self-hosted Freshell server. NEVER use broad kill patterns** (no `pkill -f vite`, `pkill node`, etc.) — AGENTS.md Process Safety.
- Broad test runs wait on the shared coordinator gate (3 sibling lanes run concurrently): check `npm run test:status` first; if another agent holds the gate, WAIT. Set `FRESHELL_TEST_SUMMARY="lane D4 freshclaude identity persistence"` on broad runs. Prefix unit runs with `env -u FRESHELL_BIND_HOST`.
- Worktree may need `npm ci` and the tsx symlink: `ln -s ../node_modules/tsx node_modules/tsx` (only if `npm test` complains).
- SCOPE FENCE — may touch ONLY: `src/store/persistMiddleware.ts` + persistedState, `src/store/panesSlice.ts` (fresh-agent identity fold), `src/store/freshAgentSlice.ts`, `src/lib/fresh-agent-ws.ts`, `src/components/fresh-agent/FreshAgentView.tsx`, `src/store/paneTypes.ts` (minimal/distinct region — Lane D1 may also touch it), the wall spec's P0.2 pin block (+ its identity reader), new test files, and `test/e2e-browser/playwright.config.ts` registration lines. Do NOT touch: `TerminalView` / `registry.rs` / exited-pane UI (Lane D1); `crates/` freshagent rust code + spawn gate (D2 — **the server side is DONE; if a server change seems required, STOP the task and report**); D3's flake-test regions (double-restart test, remote-proxy, sidebar case-a, pane_ledger tests).
- Wall pin discipline (file doc of the wall spec, lines 12-16): flip = **DELETE the `test.fail(...)` call** and rewrite the pin comment into a HISTORY note. Never widen a pin; never convert to `test.fixme`.
- PR POLICY: **NOT approved.** Push the branch, STOP before `gh pr create`. Final report must include: branch name, the archaeology finding, red→green proof including the full-wall-green run.
- TDD Red-Green-Refactor throughout. Conventional, focused commits.
- Long commands (e2e, coordinated suites) can run 10–30+ minutes — use generous timeouts and never kill a coordinator-gated run.

---

## Archaeology: why `persistMiddleware` strips `content.sessionId` — and why this fix is safe NOW

*(Required context for every task. Sourced from `git log --follow`/`git blame` on `src/store/persistMiddleware.ts` and the contract docs; verify with `git show 976d3d48` if needed.)*

**Original rationale.** The strip was introduced by commit `976d3d48` "Repair fresh-agent persistence migrations" (2026-05-08; re-landed through squash `d4c7f5b5`, PR #358 — the squash message notes interim history was lost). The commit implements the written contract `docs/plans/2026-04-19-exact-durable-session-contract.md`:

- §1: "`sessionRef` is the only replay-safe identity written to persisted terminal pane/tab state… live reattach handles… **must never be interpreted as durable restore targets**."
- §2: "Live handles are not replay targets and are never used as durable restore keys."
- §3: live inputs "may exist in memory to finish the current live session, but **they are never persisted as restore targets**."
- Restore-Unavailable Rule: absence must be an explicit `RESTORE_UNAVAILABLE`, never a surviving stale token.

This is the incident-class the strip defends against: **stale persisted live-handle identity causing a wrong-session attach** after the handle was recycled by a different server process. The narrow fresh-agent exception (same commit, same hunk) is a *same-server lease*: it re-admits a bare `sessionId` only when co-persisted with `serverInstanceId`, so another server process can never mistake it for durable identity. There is no `docs/incidents/` entry; the contract doc IS the recorded rationale.

**Why the fix does not reintroduce the hazard.**

1. **We never persist the live handle.** `content.sessionId` (the sidecar-minted nanoid placeholder) remains stripped, untouched. We persist the **durable provider session id** in `content.sessionRef` — the exact field §1 designates as "the only replay-safe identity", already round-tripped by `stripTransientSessionFields` via `sanitizeSessionRef`. No change to the strip function is needed or made.
2. **Canonical gating.** The fold refuses any claude id that fails `isValidClaudeSessionId` (canonical UUID), so a placeholder can never masquerade as durable identity. Rehydration additionally re-checks via `migrateLegacyFreshAgentDurableState({ rejectNonCanonicalClaudeSessionRef: true })` (`persistedState.ts:293-302`).
3. **The §4.2 authority chain makes a persisted client identity a PROPOSAL, not truth** — this is what makes it safe NOW when it wasn't in April. The chain (campaign plan `docs/plans/2026-07-24-restart-resilience-architecture-analysis.md` §4.2): *in-memory registry (live process truth) → ledger `bound` rows (durable server truth) → client claim (proposal only) → tabs-snapshot (rescue mirror)*. On disagreement "the ledger wins for identity; the client's claim is recorded and answered with a `corrected` verdict… user-visible, never a silent switch." With verdicts live since wave C (`foldFreshAgentVerdict`, `src/lib/pane-reconcile.ts:262-357`), a stale client claim gets `corrected:true` or a loud `dead_session` breadcrumb — never silently honored. Task 5's stale-sessionRef e2e test pins exactly this.

**Validated scope notes (2026-07-27 load-bearing review — evidence in the review ledger; server cites verified against this worktree):**

- The `dead_session` guarantee was verified in server code: an Absent-but-ever-observed claim yields `dead_session{session_not_on_disk}` echoing the claimed ref (`crates/freshell-ws/src/reconcile_freshagent.rs:120-129, 259-265`); `ever_observed` is durable across restart via the ledger; no reconcile arm can bind a *different* session id to a claim (wrong-session attach is structurally impossible). The reconcile snapshot consumes ONLY `pane.session_ref` for fresh-agent panes (`reconcile_freshagent.rs:73-79`) — exactly the field we persist.
- **The guarantee's precondition is the durable binding recorded at `session.init`** — and `claude.rs:1156-1159` skips that binding when the create carried all-default settings (the no-laundering guard, `identity_sink.rs:10-17`). This hole is unreachable from the shipped client: the sole create constructor `buildCreateMessage` (`FreshAgentView.tsx:951-972`) sends `effort:` unconditionally (`:969`, registry fallback `'high'`, `shared/fresh-agent-models.ts:131-152`), so every real freshclaude/kilroy create is non-default and always binds. **Watch note:** this invariant is un-pinned — do NOT refactor `buildCreateMessage` toward "send only user-chosen fields" without adding a pin, or the silent-`fresh` hole reopens.
- **Bound on "never":** ledger retention (~30/90d GC horizon) means an *ancient* stale ref can age out of `ever_observed` and adjudicate `fresh` (silent) instead of `dead_session` — but never a wrong-session attach (the actual incident class), and claude transcript retention (`cleanupPeriodDays` ~30d) makes such refs unresumable anyway. Accepted residual.
- **Real claude resume semantics (verified externally):** default `--resume`/SDK resume REUSES the original session UUID and appends to the same transcript — it does NOT mint a new id per resume; a new id requires explicit `--fork-session`, which our sidecar does not pass (`crates/freshell-claude-sidecar/index.mjs:209`). So the fixture's stable durable id *matches* production default semantics, and a single overwritable `sessionRef` is the correct data model in both id-worlds (newest-wins remains as robustness for ledger supersession / future fork use). **Watch note (future hardening, out of scope):** a cwd-mismatched resume can silently create a fresh session under a new valid UUID which the newest-wins fold would adopt — upstream CLI/SDK behavior, not introduced by this change.

---

## Design overview and file structure

**The gap (one paragraph).** A freshclaude pane's `content.sessionId` is an ephemeral nanoid minted by the Node claude sidecar (`crates/freshell-claude-sidecar/index.mjs:199`); the server broadcasts `freshAgent.created` with `session_ref: None` for claude (`crates/freshell-freshagent/src/claude.rs:500-509`), and — unlike opencode — never emits `freshAgent.session.materialized`. The durable Claude UUID arrives as `event.cliSessionId` on `freshAgent.session.init`/`.metadata` and is written ONLY to `freshAgentSlice.sessions[key].cliSessionId` (`src/store/freshAgentSlice.ts:256`, `:282`) — an unpersisted slice. So the pane persists with no `sessionId`, no `sessionRef`, no `resumeSessionId`, and on reload `FreshAgentView`'s mount create-effect (`FreshAgentView.tsx:1199-1277`) sends a bare `freshAgent.create` → new placeholder → new CLI session.

**The fix (all client-side; ~40 lines of production code).**

| File | Change |
|---|---|
| `src/store/panesSlice.ts` | NEW reducer `adoptFreshAgentDurableIdentity` — layout-walk (mirroring `materializeFreshAgentSession`, `panesSlice.ts:1454-1481`) matching fresh-agent leaves by live locator (placeholder `sessionId` + `sessionType` + `provider`); writes `sessionRef: { provider, sessionId: cliSessionId }` + `resumeSessionId: cliSessionId`; canonical-gated for claude; no-op when already adopted. **Leaves `content.sessionId` (live handle) untouched** — re-keying it would break the wire protocol (attach messages and incoming events are keyed on the placeholder). |
| `src/lib/fresh-agent-ws.ts` | Dispatch the new action from the `freshAgent.session.init` (`:217-225`) and `freshAgent.session.metadata` (`:226-234`) cases when `provider === 'claude'` (covers freshclaude AND kilroy sessionTypes) and `cliSessionId` present; dispatch `flushPersistedLayoutNow()` after the init-case fold (mirrors the materialized case at `:171`). **Known race (validated 2026-07-27):** the server spawns the sidecar-stdout consumer BEFORE broadcasting `freshAgent.created` (`claude.rs:426-433` vs `:502-509`, multiple awaits between, no ordering gate), and the fake sidecar emits `sdk.session.init` immediately after `created` (`fake-claude-sidecar.mjs:96-98`) — so `session.init` can reach the client BEFORE `created`, when no pane yet carries the placeholder and the fold would silently no-op. Closed by the created-time catch-up (next two rows; Task 3 Step 3b). |
| `src/store/freshAgentSlice.ts` | Verify (and if needed make) the `sessionInit` reducer an UPSERT: when `session.init` arrives before `created`, the slice must still retain `cliSessionId` for the session so the created-time catch-up can read it. Unit-test pinned (Task 3 Step 3b). |
| `src/store/persistMiddleware.ts` | **No production change.** `sessionRef` already survives `stripTransientSessionFields` (`:245-268`). |
| `src/components/fresh-agent/FreshAgentView.tsx` | **One small production change (created-time catch-up, Task 3 Step 3b):** in the `freshAgent.created` handler that writes the placeholder into pane content (`:1432-1456`), after that write, if the freshAgent slice already holds a canonical claude `cliSessionId` for this session (init won the race), dispatch `adoptFreshAgentDurableIdentity` + `flushPersistedLayoutNow`. Everything else stays unchanged: post-reload, the mount create-effect guard (`:1200-1207`) already proceeds when `sessionRef` is present, and `buildCreateMessage` (`:951-972`) already derives `resumeSessionId` from `sessionRef` — the create becomes a create-with-resume with no new wire shape. `triggerRecovery` (`:1079-1130`) already prefers the durable id via `getCanonicalPaneResumeSessionId` (`:216-228`), which reads `pane.sessionRef` first — so respawn/recovery paths carry the durable id forward once it lives in pane content, instead of minting fresh placeholders. Genuinely-new panes (no learned id yet, or explicit `startNewConversation`) keep today's placeholder behavior. |
| `src/store/paneTypes.ts` | **No change.** `sessionRef?: SessionLocator` already exists on `FreshAgentPaneContent` (`paneTypes.ts:174-209`). (Keeps Lane D1's possible paneTypes edits conflict-free.) |
| `test/unit/client/store/panesSlice.test.ts` | New reducer tests (Task 2). |
| `test/unit/client/fresh-agent-ws.test.ts` + `test/unit/client/lib/fresh-agent-ws.test.ts` | New transport-dispatch tests — the two existing copies of this suite; add the case to BOTH (Task 3). |
| `test/unit/client/store/panesPersistence.test.ts` | Persist round-trip pin test (Task 4). |
| `test/e2e-browser/specs/freshclaude-identity-persistence-rust.spec.ts` | NEW spec: (1) reload → SIGKILL → same-conversation journey; (2) stale-sessionRef → loud `dead_session`, never silent wrong-session attach (Task 5). |
| `test/e2e-browser/playwright.config.ts` | Register the new spec in `RUST_ONLY_SPECS` (~`:89`) and `rust-chromium` `testMatch` (~`:265`) (Task 5). |
| `test/e2e-browser/specs/restore-contract-wall-rust.spec.ts` | Flip the P0.2 pin (delete `test.fail` at `:1165-1168`, rewrite comment to HISTORY note) + make `leafDurableIdentity` (`:245-251`) sessionRef-first (Task 6). |

**Why the wall's identity reader must change (and why that is not weakening the wall):** the wall's `leafDurableIdentity` reads `content.sessionId` FIRST — for claude that is the `fc-e2e-*` placeholder forever, a *live handle* that legitimately changes across respawn. The repo's own contract says durable identity IS `sessionRef`; the sibling spec `freshclaude-restart-parity-rust.spec.ts:140-148` already documents this exact correction ("Deliberately NOT the donor's leafDurableIdentity: its first fallback arm is content.sessionId, which for a live claude pane is the create-time fc-e2e-* nanoid forever") and uses `sessionRef.sessionId ?? resumeSessionId`. Reordering `leafDurableIdentity` to sessionRef-first is safe for the other fresh-agent legs (E freshcodex, F freshopencode): both compare pre/post through the SAME reader, and where `sessionRef` exists it equals the durable id. The full-wall run in Task 6 proves this.

**Watch items during Task 6's full-wall run:**
- Leg I "THE RULER" (`:1359`, pinned at `:1380`) — its pin text names the P0.2 gap. If closing P0.2 makes it *unexpectedly pass*, Playwright reports that as a hard failure: flip its pin too (documented coupling). If it stays red (remaining P1.x), leave its pin alone.
- Leg M "freshclaude busy-restart" (`:1989`) — green today partly *because* identity wasn't persisted; the wave-A attach arm should keep it green. Re-observe, don't assume.
- Legs J/K (creation-window pins, D3 territory) must NOT flip — if they do, stop and investigate before touching them.

---

### Task 1: Sync worktree to current origin/main, baseline green, pin inventory

**Files:**
- No source changes. (Working dir for every command: `/home/dan/code/freshell/.worktrees/freshclaude-identity-persistence`.)

**Interfaces:**
- Consumes: nothing.
- Produces: a worktree at current `origin/main`, a recorded green baseline, and confirmation the P0.2 pin still exists (all later tasks assume this).

- [ ] **Step 1: Fast-forward the branch to current origin/main**

```bash
cd /home/dan/code/freshell/.worktrees/freshclaude-identity-persistence
git fetch origin
git merge --ff-only origin/main
git log --oneline -3
```

Expected: branch fast-forwards (it has no local commits yet) to `origin/main` (`7508149b` or newer). If `--ff-only` fails because the branch has commits, use `git rebase origin/main` instead.

- [ ] **Step 2: Ensure node deps work in this worktree**

```bash
node --version && npm --version
ls node_modules/.bin/vitest >/dev/null 2>&1 || npm ci
```

If a later `npm test` complains about tsx: `ln -s ../node_modules/tsx node_modules/tsx`.

- [ ] **Step 3: Verify the P0.2 pin is still present (STOP-gate)**

```bash
grep -n "test.fail(" test/e2e-browser/specs/restore-contract-wall-rust.spec.ts
grep -n "narrowed 2026-07-26 by reconcile-completion" test/e2e-browser/specs/restore-contract-wall-rust.spec.ts
```

Expected: a `test.fail(` inside the `freshclaude: SIGKILL restore rebinds` test (~line 1165) whose reason string contains "narrowed 2026-07-26 by reconcile-completion". Record the full pin inventory (all `test.fail(` lines) in your notes — Task 6 compares against it. **If the freshclaude pin is gone or its reason changed materially, STOP and report** (another lane may have closed or reshaped P0.2).

- [ ] **Step 4: Baseline unit suite green (coordinator-gated)**

```bash
npm run test:status
FRESHELL_TEST_SUMMARY="lane D4 baseline" env -u FRESHELL_BIND_HOST npm test
```

Expected: PASS. If the coordinator gate is held by a sibling lane, WAIT (poll `npm run test:status`); never kill a foreign holder. If baseline is red, STOP and report — do not build on a red base.

- [ ] **Step 5: Baseline the pinned wall leg (records today's red)**

```bash
npm run test:e2e -- --project=rust-chromium specs/restore-contract-wall-rust.spec.ts -g 'freshclaude: SIGKILL restore rebinds'
```

Expected: 1 **expected-failure** (Playwright reports the `test.fail()`-annotated test as passing-the-suite because it failed as expected). Save the output — this is the red half of the red→green proof for the wall leg. (First e2e invocation also builds client+server via global-setup; allow ~10 min.)

- [ ] **Step 6: Baseline the FULL wall (Task 6's attribution baseline)**

```bash
npm run test:e2e -- --project=rust-chromium specs/restore-contract-wall-rust.spec.ts
```

Expected: all legs green, remaining pins failing-as-expected. Save the full output alongside the Step 3 pin inventory — **Task 6 Step 4's decision protocol compares against exactly this baseline** (3 sibling lanes are churning wall-adjacent territory; without a fresh baseline, a pre-existing red would be mis-attributed to our change, or a foreign flip mis-flipped). If any leg is ALREADY red/flipped in ways the pin inventory doesn't explain, record it and proceed — Task 6 treats those as pre-existing, not ours. (Allow 10–30 min.)

No commit for this task.

---

### Task 2: `adoptFreshAgentDurableIdentity` reducer in panesSlice (red → green)

**Files:**
- Modify: `src/store/panesSlice.ts` (new reducer next to `materializeFreshAgentSession` at `:1454`, new export next to the other action exports ~`:2217`)
- Test: `test/unit/client/store/panesSlice.test.ts` (append a new `describe` block at the end)

**Interfaces:**
- Consumes: `FreshAgentPaneContent` (`src/store/paneTypes.ts:174-209` — fields `sessionId?`, `sessionRef?: SessionLocator`, `resumeSessionId?`, `provider`, `sessionType`), `isValidClaudeSessionId` (`src/lib/claude-session-id.ts` — already imported by panesSlice), `PaneNode` tree walk pattern from `materializeFreshAgentSession` (`panesSlice.ts:1454-1481`).
- Produces: exported action creator `adoptFreshAgentDurableIdentity(payload: { sessionId: string; sessionType: FreshAgentSessionType; provider: FreshAgentRuntimeProvider; cliSessionId: string })` — Task 3 dispatches exactly this; its payload spreads the ws locator (`{ sessionId, sessionType, provider }`) plus `cliSessionId`.

- [ ] **Step 1: Write the failing tests**

Append to `test/unit/client/store/panesSlice.test.ts`. Use the same reducer/action import style and state-builder helpers already used by the test at/near the `'strips stale fresh-agent runtime identity while preserving durable resume options'` test (~`:4238`) — that test builds a layout containing a fresh-agent leaf; reuse its helper. If constructing state manually, this minimal shape works (cast via `as unknown as PanesState`; match the `sessionType` literal other fresh-agent tests in this file use — check with `grep -n "sessionType:" test/unit/client/store/panesSlice.test.ts | head`):

```ts
describe('adoptFreshAgentDurableIdentity', () => {
  const DURABLE = '55555555-5555-4555-8555-555555555555'
  const PLACEHOLDER = 'fc-e2e-12345-1785038517244'

  const freshclaudeLeaf = (content: Record<string, unknown>) => ({
    type: 'leaf' as const,
    id: 'pane-1',
    content: {
      kind: 'fresh-agent',
      provider: 'claude',
      sessionType: 'freshclaude',
      sessionId: PLACEHOLDER,
      createRequestId: 'req-1',
      status: 'connected',
      ...content,
    },
  })

  const stateWith = (leaf: ReturnType<typeof freshclaudeLeaf>) =>
    ({
      ...panesInitialStateForTest(), // or the file's existing builder; layouts is the field that matters
      layouts: { 'tab-1': leaf },
    }) as unknown as PanesState

  const adopt = (overrides: Partial<{ sessionId: string; cliSessionId: string; provider: string; sessionType: string }> = {}) =>
    adoptFreshAgentDurableIdentity({
      sessionId: PLACEHOLDER,
      sessionType: 'freshclaude',
      provider: 'claude',
      cliSessionId: DURABLE,
      ...overrides,
    } as Parameters<typeof adoptFreshAgentDurableIdentity>[0])

  it('folds the durable cliSessionId into sessionRef and resumeSessionId, keeping the live placeholder handle', () => {
    const next = panesReducer(stateWith(freshclaudeLeaf({})), adopt())
    const content = (next.layouts['tab-1'] as any).content
    expect(content.sessionRef).toEqual({ provider: 'claude', sessionId: DURABLE })
    expect(content.resumeSessionId).toBe(DURABLE)
    expect(content.sessionId).toBe(PLACEHOLDER) // live handle NOT re-keyed
  })

  it('refuses a non-canonical claude id (placeholder can never become durable identity)', () => {
    const next = panesReducer(stateWith(freshclaudeLeaf({})), adopt({ cliSessionId: 'fc-e2e-99999-not-a-uuid' }))
    const content = (next.layouts['tab-1'] as any).content
    expect(content.sessionRef).toBeUndefined()
    expect(content.resumeSessionId).toBeUndefined()
  })

  it('does not touch panes whose live locator does not match', () => {
    const before = stateWith(freshclaudeLeaf({ sessionId: 'fc-e2e-other' }))
    const next = panesReducer(before, adopt())
    expect((next.layouts['tab-1'] as any).content.sessionRef).toBeUndefined()
  })

  // Robustness pin, not the default path: real claude default resume REUSES the
  // same UUID (verified 2026-07-27 — new ids only via --fork-session, which our
  // sidecar does not pass). A newer id can still be learned via ledger
  // supersession (corrected verdicts) or future fork use; newest-wins covers it.
  it('updates an existing sessionRef to the newest learned durable id (supersession/fork robustness)', () => {
    const NEWER = '66666666-6666-4666-8666-666666666666'
    const withOld = freshclaudeLeaf({ sessionRef: { provider: 'claude', sessionId: DURABLE }, resumeSessionId: DURABLE })
    const next = panesReducer(stateWith(withOld), adopt({ cliSessionId: NEWER }))
    const content = (next.layouts['tab-1'] as any).content
    expect(content.sessionRef).toEqual({ provider: 'claude', sessionId: NEWER })
    expect(content.resumeSessionId).toBe(NEWER)
  })

  it('is a no-op (same state reference) when the identity is already adopted', () => {
    const adopted = freshclaudeLeaf({ sessionRef: { provider: 'claude', sessionId: DURABLE }, resumeSessionId: DURABLE })
    const before = stateWith(adopted)
    const next = panesReducer(before, adopt())
    expect(next.layouts['tab-1']).toBe(before.layouts['tab-1']) // unchanged node → no persist dirty-mark
  })
})
```

- [ ] **Step 2: Run to verify RED**

```bash
npm run test:vitest -- run test/unit/client/store/panesSlice.test.ts -t adoptFreshAgentDurableIdentity
```

Expected: FAIL — `adoptFreshAgentDurableIdentity` is not exported / not a reducer.

- [ ] **Step 3: Implement the reducer**

In `src/store/panesSlice.ts`, add inside `reducers: { ... }` adjacent to `materializeFreshAgentSession` (`:1454`), copying its tree-walk shape EXACTLY (same leaf/split recursion and `state.layouts[tabId] = ...` assignment discipline):

```ts
/**
 * Fold a fresh-agent pane's learned durable provider session id into its
 * persisted identity (sessionRef + resumeSessionId), matched by the LIVE
 * locator (placeholder sessionId + sessionType + provider). The live handle
 * in content.sessionId is deliberately left untouched: it is the wire key
 * for attach/events and is never persisted (2026-04-19 durable-session
 * contract). Canonical-gated for claude so a placeholder can never be
 * adopted as durable identity. Closes P0.2 (restore contract wall).
 */
adoptFreshAgentDurableIdentity: (
  state,
  action: PayloadAction<{
    sessionId: string
    sessionType: FreshAgentSessionType
    provider: FreshAgentRuntimeProvider
    cliSessionId: string
  }>,
) => {
  const { sessionId, sessionType, provider, cliSessionId } = action.payload
  if (!cliSessionId) return
  if (provider === 'claude' && !isValidClaudeSessionId(cliSessionId)) return
  for (const [tabId, root] of Object.entries(state.layouts)) {
    let changed = false
    const updateContent = (node: PaneNode): PaneNode => {
      if (node.type === 'leaf') {
        const content = node.content
        if (content.kind !== 'fresh-agent') return node
        if (content.provider !== provider) return node
        if (content.sessionType !== sessionType) return node
        if (content.sessionId !== sessionId) return node
        if (
          content.sessionRef?.provider === provider &&
          content.sessionRef.sessionId === cliSessionId &&
          content.resumeSessionId === cliSessionId
        ) {
          return node
        }
        changed = true
        return {
          ...node,
          content: {
            ...content,
            sessionRef: { provider, sessionId: cliSessionId },
            resumeSessionId: cliSessionId,
          },
        }
      }
      // Split-node recursion: copy the exact split-arm shape used by
      // materializeFreshAgentSession (panesSlice.ts:1454-1481).
      return { ...node, /* recurse children exactly as the donor does */ } as PaneNode
    }
    const nextRoot = updateContent(root)
    if (changed) state.layouts[tabId] = nextRoot
  }
},
```

Export the action creator alongside the others (same `export const { ... } = panesSlice.actions` block that exports `materializeFreshAgentSession`, ~`:2217`). `isValidClaudeSessionId`, `FreshAgentSessionType`, `FreshAgentRuntimeProvider`, `PaneNode` are all already imported/available in this module — verify, don't duplicate imports.

- [ ] **Step 4: Run to verify GREEN, then the surrounding suite**

```bash
npm run test:vitest -- run test/unit/client/store/panesSlice.test.ts
```

Expected: PASS (all, including the pre-existing fresh-agent identity tests — the strip tests at ~`:4238` must stay green: we did not change what is persisted for panes lacking a durable id).

- [ ] **Step 5: Commit**

```bash
git add src/store/panesSlice.ts test/unit/client/store/panesSlice.test.ts
git commit -m "feat(client): adopt freshclaude durable cliSessionId into pane sessionRef (P0.2 lane D4)"
```

---

### Task 3: Wire the fold from the ws transport learn sites (red → green)

**Files:**
- Modify: `src/lib/fresh-agent-ws.ts` (`freshAgent.session.init` case `:217-225`; `freshAgent.session.metadata` case `:226-234`)
- Modify: `src/components/fresh-agent/FreshAgentView.tsx` (created-time catch-up in the `freshAgent.created` handler, `:1432-1456` — Step 3b)
- Modify (verify-first): `src/store/freshAgentSlice.ts` (`sessionInit` reducer upsert — Step 3b)
- Test: `test/unit/client/fresh-agent-ws.test.ts` AND `test/unit/client/lib/fresh-agent-ws.test.ts` (two copies of this suite exist — add the cases to BOTH); the freshAgentSlice unit test file (find it: `grep -rln "sessionInit" test/unit/client/ --include='*freshAgent*'`; create one if absent)

**Interfaces:**
- Consumes: `adoptFreshAgentDurableIdentity` from Task 2 (payload `{ sessionId, sessionType, provider, cliSessionId }`); `flushPersistedLayoutNow` from `src/store/persistControl.ts` (already imported in this file for the materialized case, `:171`); the locator built at `fresh-agent-ws.ts:197-201` (`{ sessionId: msg.sessionId, sessionType, provider }` — the placeholder-keyed live locator).
- Produces: on every `freshAgent.session.init`/`freshAgent.session.metadata` for `provider === 'claude'` carrying a `cliSessionId`, the pane's persisted identity is updated and (for init) flushed to localStorage immediately.

- [ ] **Step 1: Write the failing tests (in BOTH test files)**

Follow the existing dispatch-capture harness in each file (they already test that `freshAgent.session.init` dispatches `sessionInit` — extend those cases). Add:

```ts
it('freshAgent.session.init (claude) also adopts the durable identity into pane content and flushes persistence', () => {
  // Arrange exactly like the existing session.init test in this file,
  // with provider 'claude' and event.cliSessionId set to a canonical UUID:
  const DURABLE = '55555555-5555-4555-8555-555555555555'
  // ...existing harness: deliver a freshAgent.event wrapping
  // { type: 'freshAgent.session.init', cliSessionId: DURABLE, ... } for
  // sessionId 'fc-placeholder-1', sessionType 'freshclaude', provider 'claude'

  const types = dispatched.map((a) => a.type)
  expect(types).toContain(adoptFreshAgentDurableIdentity.type)
  const adopt = dispatched.find((a) => a.type === adoptFreshAgentDurableIdentity.type)
  expect(adopt.payload).toEqual({
    sessionId: 'fc-placeholder-1',
    sessionType: 'freshclaude',
    provider: 'claude',
    cliSessionId: DURABLE,
  })
  expect(types).toContain(flushPersistedLayoutNow.type)
})

it('freshAgent.session.metadata (claude) adopts the durable identity into pane content', () => {
  // same harness, event type 'freshAgent.session.metadata'
  expect(dispatched.map((a) => a.type)).toContain(adoptFreshAgentDurableIdentity.type)
})

it('does not adopt for non-claude providers or when cliSessionId is absent', () => {
  // deliver session.init with provider 'opencode', and a claude init WITHOUT cliSessionId
  expect(dispatched.map((a) => a.type)).not.toContain(adoptFreshAgentDurableIdentity.type)
})
```

(`dispatched` is whatever the file's existing harness collects — match it exactly; both files already assert on dispatched action types for the init case.)

- [ ] **Step 2: Run to verify RED**

```bash
npm run test:vitest -- run test/unit/client/fresh-agent-ws.test.ts test/unit/client/lib/fresh-agent-ws.test.ts
```

Expected: the new cases FAIL (`adoptFreshAgentDurableIdentity` never dispatched).

- [ ] **Step 3: Implement the wiring**

In `src/lib/fresh-agent-ws.ts`, import `adoptFreshAgentDurableIdentity` from `../store/panesSlice` (the module already imports `materializeFreshAgentPaneSession` from there). Then, in `handleFreshAgentTransportEvent`:

In the `freshAgent.session.init` case (`:217-225`), after the existing `dispatch(sessionInit({...}))`:

```ts
if (locator.provider === 'claude' && typeof event.cliSessionId === 'string' && event.cliSessionId) {
  dispatch(adoptFreshAgentDurableIdentity({ ...locator, cliSessionId: event.cliSessionId }))
  // Durable identity must hit disk before any crash/reload window closes —
  // mirrors the session.materialized flush (:171).
  dispatch(flushPersistedLayoutNow())
}
```

In the `freshAgent.session.metadata` case (`:226-234`), after the existing `dispatch(sessionMetadataReceived({...}))`:

```ts
if (locator.provider === 'claude' && typeof event.cliSessionId === 'string' && event.cliSessionId) {
  dispatch(adoptFreshAgentDurableIdentity({ ...locator, cliSessionId: event.cliSessionId }))
}
```

(No explicit flush in the metadata case: the reducer is a no-op → same state reference → no dirty-mark when nothing changed; real changes ride the 500ms debounce + unload flush. `locator` here is the `{ sessionId: msg.sessionId, sessionType, provider }` object built at `:197-201`.)

- [ ] **Step 3b: Created-time catch-up for the init-before-created race (validated 2026-07-27)**

The server does NOT guarantee `freshAgent.created` reaches the client before `freshAgent.session.init`: the sidecar-stdout consumer is spawned (`claude.rs:426-433`) before the `created` broadcast (`:502-509`) with multiple awaits between and no ordering gate, and the fake sidecar emits `created` + `sdk.session.init` back-to-back (`fake-claude-sidecar.mjs:96-98`) — so in the plan's own e2e fixture, `session.init` can arrive FIRST, when no pane carries the placeholder and Step 3's fold silently no-ops. At init time there is no client-side breadcrumb mapping placeholder→pane (the init event carries no requestId; `pendingCreates` back-fill races identically), so the catch-up must run when `created` finally arrives:

1. **freshAgentSlice upsert (verify-first):** read the `sessionInit` reducer (the one writing `cliSessionId` at `freshAgentSlice.ts:256`). If it drops the event when no session record exists yet, make it upsert a minimal record retaining `cliSessionId` (in-fence; `freshAgentSlice.ts` is in the scope fence). Add a unit test either way: *"sessionInit retains cliSessionId when it arrives before the session record exists (init-before-created)"* — arrange an empty slice, dispatch `sessionInit` with a canonical claude `cliSessionId`, assert the id is readable afterward.
2. **FreshAgentView catch-up:** in the `freshAgent.created` handler that writes the placeholder into pane content (`FreshAgentView.tsx:1432-1456`, matched on `message.requestId === paneContentRef.current.createRequestId`), AFTER that write, read the freshAgent slice record for this session (same selector/key derivation the view already uses for its session state); if it already holds a `cliSessionId` that passes `isValidClaudeSessionId` and `provider === 'claude'`, dispatch `adoptFreshAgentDurableIdentity({ sessionId: message.sessionId, sessionType, provider, cliSessionId })` followed by `flushPersistedLayoutNow()`. This is idempotent with Step 3 (the reducer's already-adopted no-op arm makes double-dispatch harmless in the normal order).

End-to-end coverage: Task 5 test 1's FOLD poll exercises exactly this race under the tightest-timing fixture (plus the `--repeat-each=2` flake check).

- [ ] **Step 4: Run to verify GREEN**

```bash
npm run test:vitest -- run test/unit/client/fresh-agent-ws.test.ts test/unit/client/lib/fresh-agent-ws.test.ts test/unit/client/store/panesSlice.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/lib/fresh-agent-ws.ts test/unit/client/fresh-agent-ws.test.ts test/unit/client/lib/fresh-agent-ws.test.ts
git commit -m "feat(client): fold claude session.init/metadata cliSessionId into persisted pane identity (P0.2 lane D4)"
```

---

### Task 4: Persist round-trip pin test (contract documentation)

**Files:**
- Test: `test/unit/client/store/panesPersistence.test.ts` (append)

**Interfaces:**
- Consumes: the persist/reload seams this file already exercises (`loadInitialPanesState` / persisted-layout round-trip helpers — the test at `:46` "persist+restore across refresh" and `:333` "does not persist refreshRequestsByPane" are the structural donors).
- Produces: a pinned invariant later reviewers rely on: *adopted durable identity survives the persist round-trip; the live placeholder does not.*

- [ ] **Step 1: Write the test**

Following the round-trip harness of the `:46` test (build store state → trigger flush → re-load via the same seam that test uses):

```ts
it('round-trips an adopted freshclaude durable identity: sessionRef survives, live placeholder and resumeSessionId are stripped', () => {
  const DURABLE = '55555555-5555-4555-8555-555555555555'
  // Arrange: a fresh-agent leaf as the adopt fold leaves it:
  //   { kind: 'fresh-agent', provider: 'claude', sessionType: 'freshclaude',
  //     sessionId: 'fc-e2e-123', createRequestId: 'req-1', status: 'connected',
  //     sessionRef: { provider: 'claude', sessionId: DURABLE },
  //     resumeSessionId: DURABLE }
  // Act: flush persistence, then re-load panes state (same seam as the :46 test).
  // Assert on the restored fresh-agent leaf content:
  expect(restored.sessionRef).toEqual({ provider: 'claude', sessionId: DURABLE })
  expect(restored.sessionId).toBeUndefined() // live handle never persisted
  expect(restored.resumeSessionId).toBeUndefined() // always stripped; re-derived from sessionRef at create time
})
```

- [ ] **Step 2: Run it**

```bash
npm run test:vitest -- run test/unit/client/store/panesPersistence.test.ts
```

Expected: PASS — and that is fine here. **Honest note:** `sessionRef` round-tripping is pre-existing behavior (`stripTransientSessionFields` keeps sanitized `sessionRef`); this test is a *contract pin*, not a red-first driver — the red-first drivers for this feature are Task 2/3's unit tests and the standing wall pin (baselined red in Task 1 Step 5). If this test FAILS, `sanitizeSessionRef` or `normalizeFreshAgentContent` rejects the ref — investigate before proceeding (the canonical UUID above must pass `rejectNonCanonicalClaudeSessionRef`).

- [ ] **Step 3: Commit**

```bash
git add test/unit/client/store/panesPersistence.test.ts
git commit -m "test(client): pin freshclaude durable sessionRef persist round-trip (P0.2 lane D4)"
```

---

### Task 5: End-to-end proof + stale-identity hazard guard (new e2e spec)

**Files:**
- Create: `test/e2e-browser/specs/freshclaude-identity-persistence-rust.spec.ts`
- Modify: `test/e2e-browser/playwright.config.ts` — add `/freshclaude-identity-persistence-rust\.spec\.ts$/` to `RUST_ONLY_SPECS` (~`:89`) AND to the `rust-chromium` project's `testMatch` (~`:265`)

**Interfaces:**
- Consumes: `RustServer` (`test/e2e-browser/helpers/rust-server.ts:272` — `.start()`, `.restartAbrupt()` `:344`, `.stop()`, `info.homeDir`); `TestHarness` (`helpers/test-harness.ts` — `getState()`, `getPaneLayout(tabId)`, `getActiveTabId()`, `getSentWsMessages()`, `clearSentWsMessages()`); spec-local helper bodies COPIED (per this suite's per-spec-ownership convention, wall spec file doc `:47-51`) from `restore-contract-wall-rust.spec.ts`: `seedWallConfig` (`:131`), `bootWall` (`:156`), `flushPersistence` (`:118`), `reloadAndReconnect` (`:124`), `waitForWsReady` (`:108`), `findFreshAgentLeaf` (`:233`), `createFreshclaudePane` (`:436`), `sendFreshAgentTurn` (`:371`), plus the fake-sidecar env plumbing the wall's leg G uses (fixture `test/e2e-browser/fixtures/fake-claude-sidecar.mjs`; env keys `FRESHELL_CLAUDE_SIDECAR`, `FAKE_CLAUDE_SIDECAR_LOG`; durable UUID constant `44444444-4444-4444-8444-444444444444`; assistant reply text `'Fixture claude turn'`).
- Produces: the e2e evidence Task 7 reports; the stale-identity guard that pins the archaeology safety argument.

- [ ] **Step 1: Write the spec**

Create `test/e2e-browser/specs/freshclaude-identity-persistence-rust.spec.ts`. Skeleton (copy the named helpers verbatim from the wall spec at the cited lines; keep this spec self-contained per convention):

```ts
/**
 * FRESHCLAUDE CLIENT IDENTITY PERSISTENCE -- P0.2 close-out (lane D4).
 * Proves the browser's OWN persisted copy of a freshclaude pane's identity
 * carries the durable ref end-to-end:
 *   1. converse -> RELOAD (identity survives the browser's persisted state
 *      alone) -> server SIGKILL restart -> the SAME conversation resumes.
 *   2. HAZARD GUARD (the reason persistMiddleware stripped sessionId in the
 *      first place -- 2026-04-19 durable-session contract): a STALE persisted
 *      sessionRef (transcripts deleted server-side) yields the LOUD
 *      dead_session adjudication flow -- never a silent wrong-session attach
 *      and never a silent fresh.
 * Rust-only: registered in RUST_ONLY_SPECS + rust-chromium testMatch.
 * Helpers copied, not imported, per this suite's per-spec-ownership
 * convention (donor: restore-contract-wall-rust.spec.ts).
 */
import fs from 'node:fs/promises'
import path from 'node:path'
import { test, expect, type Page } from '../helpers/fixtures.js'
import { RustServer } from '../helpers/rust-server.js'
import { TestHarness } from '../helpers/test-harness.js'

const DURABLE_CLI_SESSION_ID = '44444444-4444-4444-8444-444444444444'

// The contract-correct identity reader: sessionRef IS the durable identity;
// content.sessionId is a live handle (precedent:
// freshclaude-restart-parity-rust.spec.ts:140-148).
const durableIdentity = (leaf: any): string =>
  leaf?.content?.sessionRef?.sessionId ?? leaf?.content?.resumeSessionId ?? ''

// [COPY VERBATIM from restore-contract-wall-rust.spec.ts]
// waitForWsReady (:108), flushPersistence (:118), reloadAndReconnect (:124),
// seedWallConfig (:131), bootWall (:156), findFreshAgentLeaf (:233),
// createFreshclaudePane (:436), sendFreshAgentTurn (:371)
// -- and the leg-G env/setupHome wiring for the fake claude sidecar
//    (fixture path + FAKE_CLAUDE_SIDECAR_LOG), including any setupHome
//    seeding leg G performs.

test.describe('Freshclaude identity persistence (P0.2)', () => {
  test.setTimeout(180_000)

  test('durable identity survives browser reload, then SIGKILL restart resumes the SAME conversation', async ({ page, e2eServerKind }) => {
    expect(e2eServerKind).toBe('rust')
    const { server, harness } = await bootWall(page, { /* leg-G env: fake sidecar + request log */ })
    try {
      const tabId = await createFreshclaudePane(page, harness, /* cwd per donor */)
      await sendFreshAgentTurn(page, harness, tabId, 'first turn before reload')
      await expect(page.locator('[data-context="fresh-agent"]').last()).toContainText('Fixture claude turn', { timeout: 30_000 })

      // THE FOLD (red without tasks 2-3): pane content carries the durable ref.
      await expect
        .poll(async () => durableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabId))), { timeout: 15_000 })
        .toBe(DURABLE_CLI_SESSION_ID)

      // RELOAD FIRST (browser-persisted identity alone, no server help).
      await flushPersistence(page)
      await reloadAndReconnect(page, harness)
      const tabIdAfterReload = await harness.getActiveTabId()
      await expect
        .poll(async () => durableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabIdAfterReload!))), { timeout: 30_000 })
        .toBe(DURABLE_CLI_SESSION_ID)

      // THEN the SIGKILL restart.
      await harness.clearSentWsMessages()
      await server.restartAbrupt()
      await waitForWsReady(page)

      // Identity held; conversation continues end-to-end.
      await expect
        .poll(async () => durableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabIdAfterReload!))), { timeout: 30_000 })
        .toBe(DURABLE_CLI_SESSION_ID)
      await sendFreshAgentTurn(page, harness, tabIdAfterReload!, 'second turn after restart')
      await expect(page.locator('[data-context="fresh-agent"]').last()).toContainText('Fixture claude turn', { timeout: 30_000 })
      await expect(page.locator('[data-context="fresh-agent"]').last()).toContainText('first turn before reload', { timeout: 30_000 })

      // Every create sent after the reload targeted the ORIGINAL session
      // (leg-E pattern, wall :624-631) -- no identity-losing re-create.
      const sent = await harness.getSentWsMessages()
      for (const create of sent.filter((m: any) => m?.type === 'freshAgent.create') as any[]) {
        expect(create.resumeSessionId ?? create.sessionRef?.sessionId, JSON.stringify(create)).toBe(DURABLE_CLI_SESSION_ID)
      }
      const finalLeaf = findFreshAgentLeaf(await harness.getPaneLayout(tabIdAfterReload!))
      expect(finalLeaf?.content?.status).not.toBe('error')
    } finally {
      await server.stop()
    }
  })

  test('HAZARD GUARD: stale persisted sessionRef yields loud dead_session, never silent wrong-session attach or silent fresh', async ({ page, e2eServerKind }) => {
    expect(e2eServerKind).toBe('rust')
    const { server, info, harness } = await bootWall(page, { /* same env */ })
    try {
      const tabId = await createFreshclaudePane(page, harness, /* cwd per donor */)
      await sendFreshAgentTurn(page, harness, tabId, 'turn that will become stale')
      await expect
        .poll(async () => durableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabId))), { timeout: 15_000 })
        .toBe(DURABLE_CLI_SESSION_ID)
      await flushPersistence(page)

      // Make the persisted identity STALE: delete every server-side artifact
      // naming the durable session (transcripts under the isolated HOME).
      const deleted = await deleteFilesNamed(info.homeDir, `${DURABLE_CLI_SESSION_ID}.jsonl`)
      expect(deleted.length, `expected transcript artifacts for ${DURABLE_CLI_SESSION_ID} under ${info.homeDir}`).toBeGreaterThan(0)

      await harness.clearSentWsMessages()
      await server.restartAbrupt()
      await waitForWsReady(page)
      await reloadAndReconnect(page, harness)
      const tabIdAfter = await harness.getActiveTabId()

      // LOUD: the dead-session adjudication surfaces the stale claim.
      await expect
        .poll(async () => {
          const state = await harness.getState()
          const entries = state?.panes?.deadSessionAdjudication ?? []
          return entries.some((e: any) => e?.kind === 'fresh-agent' && e?.sessionRef?.sessionId === DURABLE_CLI_SESSION_ID)
        }, { timeout: 30_000 })
        .toBe(true)
      const leaf = findFreshAgentLeaf(await harness.getPaneLayout(tabIdAfter!))
      expect(leaf?.content?.restoreError?.reason).toBe('durable_artifact_missing')
      // NEVER silent: identity not swapped, no fresh create fired for this pane.
      expect(leaf?.content?.sessionRef?.sessionId).toBe(DURABLE_CLI_SESSION_ID)
      const sent = await harness.getSentWsMessages()
      expect(sent.filter((m: any) => m?.type === 'freshAgent.create')).toEqual([])
    } finally {
      await server.stop()
    }
  })
})

/** Recursively delete files with the given basename under root; returns deleted paths. */
async function deleteFilesNamed(root: string, basename: string): Promise<string[]> {
  const hits: string[] = []
  async function walk(dir: string): Promise<void> {
    let entries
    try {
      entries = await fs.readdir(dir, { withFileTypes: true })
    } catch {
      return
    }
    for (const entry of entries) {
      const p = path.join(dir, entry.name)
      if (entry.isDirectory()) await walk(p)
      else if (entry.name === basename) {
        await fs.rm(p)
        hits.push(p)
      }
    }
  }
  await walk(root)
  return hits
}
```

Implementation notes for the copier:
- Match `bootWall`'s option plumbing to how the wall's leg G builds its env/`setupHome` (read leg G's body, `:1168` onward, for the exact fixture wiring — including any transcript/home seeding it performs so history/reconcile see disk truth; `restartAbrupt()` re-runs `setupHome` on every boot, so seeding must be idempotent, wall `:130-153`).
- If `deleteFilesNamed` finds nothing, the fixture stores transcripts under a different name — `find <homeDir> -name '*.jsonl'` (in a debug run) to locate the artifact naming the durable UUID and adjust the basename; the assertion `deleted.length > 0` keeps this honest.
- If the dead-session UI evolves field names, the authoritative sources are `foldFreshAgentVerdict` (`src/lib/pane-reconcile.ts:310-325`: pushes `DeadSessionEntry` + `setPaneRestoreError(buildRestoreError('durable_artifact_missing'))`) and `DeadSessionEntry` (`paneTypes.ts:286-295`).
- **cwd is REQUIRED on `createFreshclaudePane`** (the donor passes it — keep it). Background: the server records the durable identity binding only for non-default create settings (`claude.rs:1156-1159` no-laundering guard); the client always sends `effort` so binding occurs regardless, but the explicit cwd keeps the test aligned with the donor and with real usage.
- **History must be asserted via RENDERED pane content** (the `toContainText` polls), never via the create-response `session.snapshot` event — the fixture's resume snapshot is empty by design; rehydration flows through the server snapshot adapter reading the transcript (`snapshot.rs:134` → `locate_transcript`).
- **Zero-creates contingency (test 2, validated 2026-07-27):** `creates == []` holds via the ws-client sender-level create hold (`ws-client.ts:263-284, 687-698`) + fold retraction, bounded by a ~4s verdict window (`ws-client.ts:79`); a cold session index can also transiently yield a `respawn` (same ref) instead of `dead_session` on the first reconcile. If the zero-creates assertion proves flaky for exactly these reasons (verdict later than the hold window / transient respawn), the PRE-DECLARED fallback assertion is: every `freshAgent.create` sent MUST carry the stale durable ref (`resumeSessionId ?? sessionRef.sessionId === DURABLE_CLI_SESSION_ID`) — never a bare fresh create — and the `dead_session` adjudication poll stays as-is. Do NOT weaken further; if the verdict is `fresh`, that is the silent-data-loss hazard — STOP and investigate.

- [ ] **Step 2: Register the spec**

In `test/e2e-browser/playwright.config.ts`: add `/freshclaude-identity-persistence-rust\.spec\.ts$/` to the `RUST_ONLY_SPECS` array (~`:89`) AND to the `rust-chromium` project's `testMatch` array (~`:265`). Both are required (wall report convention: rust-only specs are testIgnored by the match-all project and testMatched by rust-chromium).

- [ ] **Step 3: Run the new spec**

```bash
npm run test:e2e -- --project=rust-chromium specs/freshclaude-identity-persistence-rust.spec.ts
```

Expected: 2 PASSED. (Red-first status honestly stated: test 1's journey was red before Tasks 2-3 — the standing wall pin baselined in Task 1 Step 5 is its long-lived red record; test 2 is red without the fold because the pane persists NO identity, so no dead_session claim ever surfaces — the adjudication-entry poll times out.) If test 1 fails on the FOLD poll, debug the Task 2/3 wiring before touching e2e code. If test 2 fails because the verdict is `fresh` instead of `dead_session`, that is the silent-data-loss hazard showing — STOP and investigate; do not weaken the assertion.

- [ ] **Step 4: Flake check (this suite's convention for new e2e)**

```bash
npm run test:e2e -- --project=rust-chromium specs/freshclaude-identity-persistence-rust.spec.ts --repeat-each=2
```

Expected: all green twice.

- [ ] **Step 5: Commit**

```bash
git add test/e2e-browser/specs/freshclaude-identity-persistence-rust.spec.ts test/e2e-browser/playwright.config.ts
git commit -m "test(e2e): freshclaude identity survives reload+SIGKILL; stale sessionRef dies loud (P0.2 lane D4)"
```

---

### Task 6: Flip the P0.2 wall pin and prove the FULL wall

**Files:**
- Modify: `test/e2e-browser/specs/restore-contract-wall-rust.spec.ts` — the P0.2 pin block (`:1144-1168` region) and `leafDurableIdentity` (`:245-251`)

**Interfaces:**
- Consumes: green implementation from Tasks 2-3; the pin inventory recorded in Task 1 Step 3.
- Produces: wall leg G green as a normal expectation; full-wall run evidence for Task 7's report.

- [ ] **Step 1: Make `leafDurableIdentity` sessionRef-first**

At `restore-contract-wall-rust.spec.ts:245-251`, reorder the reader so `sessionRef` (the contract's durable identity) wins over the live handle, keeping the existing fallback arms in their current relative order after it:

```ts
// Durable identity reader: sessionRef IS the durable identity per the
// 2026-04-19 durable-session contract; content.sessionId is a live handle
// (for claude, the create-time fc-e2e-* placeholder forever -- see
// freshclaude-restart-parity-rust.spec.ts:140-148). sessionRef-first keeps
// this reader reload-symmetric for every fresh-agent provider.
const leafDurableIdentity = (leaf: any): string =>
  leaf?.content?.sessionRef?.sessionId ?? /* then the pre-existing arms, e.g. */ leaf?.content?.sessionId ?? leaf?.content?.resumeSessionId ?? ''
```

(Read the current body first and preserve its exact fallback arms — only hoist `sessionRef?.sessionId` to the front.)

- [ ] **Step 2: Flip the pin**

(Order matters: the flip is only valid AFTER Step 1's reader reorder — the pin's 2026-07-26 "server must expose the durable id as the primary handle" theory presumed today's sessionId-first reader; with sessionRef-first + persisted identity, the closure is client-side. Validated 2026-07-27.)

In the `freshclaude: SIGKILL restore rebinds with history rehydrated and status not wedged` test (`:1144`):
1. **Delete** the entire `test.fail(...)` call (`:1165-1168`, the one whose reason begins `EXPECTED-FAIL WALL PIN (narrowed 2026-07-26 by reconcile-completion)`).
2. **Replace** the pin comment block above it (`:1149-1164`) with a HISTORY note (in-repo precedent: legs F `:1026-1044` and L `:1899-1903`):

```ts
    // HISTORY: the P0.2 pin was FLIPPED 2026-07-27 by lane D4
    // (freshclaude-identity-persistence). The client now folds the sidecar
    // session.init/metadata cliSessionId into pane content.sessionRef
    // (adoptFreshAgentDurableIdentity in panesSlice, dispatched from
    // fresh-agent-ws) -- the field persistMiddleware already round-trips --
    // so the durable identity survives reload while the live placeholder in
    // content.sessionId stays unpersisted per the 2026-04-19 durable-session
    // contract. leafDurableIdentity is sessionRef-first accordingly. The
    // stale-claim hazard that motivated the original strip is pinned by
    // specs/freshclaude-identity-persistence-rust.spec.ts (dead_session,
    // never silent).
```

3. Leave the leg's assertions otherwise intact — with `leafDurableIdentity` now sessionRef-first, the pre-kill capture and post-reload comparison both read the durable UUID. If any *additional* leg-G assertion still hardcodes `content.sessionId` semantics, fix it to the sessionRef-first reader — never delete an assertion.

- [ ] **Step 3: Run leg G alone — GREEN**

```bash
npm run test:e2e -- --project=rust-chromium specs/restore-contract-wall-rust.spec.ts -g 'freshclaude: SIGKILL restore rebinds'
```

Expected: 1 passed (as a NORMAL expectation now). This plus Task 1 Step 5's saved output is the wall leg's red→green proof.

- [ ] **Step 4: Run the FULL wall — prove no other pin flips unexpectedly**

```bash
npm run test:e2e -- --project=rust-chromium specs/restore-contract-wall-rust.spec.ts
```

Decision protocol against the Task 1 pin inventory:
- All green with remaining pins failing-as-expected → done.
- **Leg I "THE RULER" unexpectedly passes** (Playwright hard-fails an unexpected pass): this is the documented coupling — its pin text names the P0.2 gap. Flip its pin too: delete its `test.fail(...)` (`:1380` region) and add a HISTORY note crediting lane D4 for closing the final gap. Re-run the full wall.
- **Legs J/K or any other pin unexpectedly passes**: NOT ours (D3/other-lane territory) — STOP, capture the output, and report it in the final summary instead of flipping.
- **Leg M (busy-restart) or legs E/F regress**: our identity-persistence or reader change interacting badly — debug ours (first suspects: the sessionRef-first reader; an unexpected attach-arm interaction now that identity persists). Do not paper over with pin edits.

Expected end state: full wall green (any remaining `test.fail` pins failing as expected; per the campaign, if this was the last gap, ZERO pins remain and every test passes plainly).

- [ ] **Step 5: Run the sibling parity spec (adjacent coverage)**

```bash
npm run test:e2e -- --project=rust-chromium specs/freshclaude-restart-parity-rust.spec.ts
```

Expected: PASS (its reconnect-leg semantics are untouched; it already reads sessionRef-first).

- [ ] **Step 6: Commit**

```bash
git add test/e2e-browser/specs/restore-contract-wall-rust.spec.ts
git commit -m "test(e2e): flip P0.2 wall pin -- freshclaude identity persists client-side (lane D4)"
```

(If Step 4 flipped the ruler pin as well, include that in this commit and name it in the message body.)

---

### Task 7: Full gates, push, STOP before PR

**Files:**
- No new source changes (fix-ups only if gates fail).

**Interfaces:**
- Consumes: everything above.
- Produces: pushed branch + final report. **NO PR** (policy: not approved — stop before `gh pr create`).

- [ ] **Step 1: Lint clean**

```bash
npm run lint
```

Expected: clean. Fix any findings in OUR files only; re-run.

- [ ] **Step 2: Typecheck + full coordinated suite**

```bash
npm run test:status
FRESHELL_TEST_SUMMARY="lane D4 freshclaude identity persistence" env -u FRESHELL_BIND_HOST npm run check
```

Expected: PASS. Wait on the coordinator gate if held (3 sibling lanes). Watch for cross-cutting fallout our change could legitimately cause: `crossTabSync` fresh-agent merge tests (`shouldPreferLocalAgentPaneDuringHydration` gates on `isValidClaudeSessionId(localContent.resumeSessionId)` — now truthy for adopted panes) and `tabRegistrySync.test.ts:204` (publishes materialized refs — must still hold since we never persist the placeholder). Fix root causes in our fenced files; if a failure demands a change OUTSIDE the fence, STOP and report.

- [ ] **Step 3: Final e2e sweep**

```bash
npm run test:e2e -- --project=rust-chromium specs/restore-contract-wall-rust.spec.ts specs/freshclaude-identity-persistence-rust.spec.ts specs/freshclaude-restart-parity-rust.spec.ts
```

Expected: all green. Save the full-wall output — it is the headline evidence.

- [ ] **Step 4: Push the branch — and STOP**

```bash
git log --oneline origin/main..HEAD
git push -u origin freshclaude-identity-persistence
```

**Do NOT run `gh pr create`.** PR is not approved for this lane.

- [ ] **Step 5: Final report (deliverable text, not a commit)**

Report, in this order:
1. Branch: `freshclaude-identity-persistence` (pushed), base commit it sits on.
2. **The archaeology finding** (condense the plan's Archaeology section): strip introduced by `976d3d48` implementing the 2026-04-19 exact-durable-session contract (live handles are never durable restore targets; hazard: stale identity → wrong-session attach); fix persists only the contract-designated durable `sessionRef`, canonical-gated, and is safe NOW because the §4.2 authority chain (verdicts live since wave C) treats any persisted client identity as a proposal — stale claims get `corrected:true`/loud `dead_session`, pinned by the new hazard-guard e2e.
3. **Red→green proof**: Task 1 Step 5 output (leg G failing-as-expected under the pin) → Task 6 Step 3 output (leg G green, pin deleted); Task 2/3 unit red→green; **the full-wall-green run from Step 3 above**, noting every remaining pin's status (expected: zero pins remaining, or explicitly list any still-pinned legs owned by other lanes).
4. Any observations for sibling lanes (e.g. ruler pin flipped here, or unexpected passes left un-flipped per protocol).

---

## Self-Review (performed at plan-writing time)

**1. Spec coverage.**
- Archaeology first, rationale + why-safe-now (§4.2 authority chain answer shape) → plan's Archaeology section + report item 2 (Task 7). ✔
- Persist durable identity via sessionRef from the sdk.session.init/session.materialized fold sites; respawn/recovery carries it forward; placeholder kept for genuinely-new panes → Tasks 2-4 (+ design-overview mapping of `triggerRecovery`/`getCanonicalPaneResumeSessionId`/`startNewConversation`, which already do the carrying/keeping once pane content holds `sessionRef`). Note: for claude there is no `session.materialized` (server sends `session_ref: None`) — the durable-id learn sites are `session.init`/`session.metadata`, which is where the fold is wired; the materialized path (opencode) already folds today. ✔
- Flip the pin with a commit note naming the lane; run FULL wall to prove no unexpected flips → Task 6 (+ decision protocol for the coupled ruler pin). ✔
- Stale-sessionRef guard test (loud dead_session/corrected, never silent wrong-session attach) → Task 5 test 2. ✔
- TDD red-first; e2e with own RustServer on ephemeral ports, never 3001/3002; reload (not just restart) in the journey; SIGKILL restart; full wall green → Tasks 1/5/6; reload happens BEFORE the SIGKILL in the new e2e, and the wall leg G covers the flush→SIGKILL→reload ordering. ✔
- Repo rules (worktree, baseline green first, coordinator gate, npm ci/tsx, lint, never restart live server, no broad kills, PR policy stop) → Global Constraints + Tasks 1 and 7. ✔

**1b. No silent deferrals.** No stubs or seams stand in for behavior: the e2e journey uses the production persistence path, real RustServer SIGKILL, and a real browser reload; the fake claude sidecar is the suite's established production-seam fixture (same one the wall itself uses), not a new test double. Every user-facing requirement has an observable outcome test (wall leg G; new spec tests 1-2). No known-limitations deferrals. No unresolved coverage gaps.

**2. Placeholder scan.** Two deliberate copy-by-reference instructions remain (spec-local e2e helpers "copy verbatim from restore-contract-wall-rust.spec.ts:<line>" and the split-arm recursion "copy the exact shape from panesSlice.ts:1454-1481"): these point at exact existing code by file:line per the suite's own per-spec-ownership convention — the engineer copies mechanically rather than inventing. No TBD/TODO/"handle edge cases" items.

**3. Type consistency.** `adoptFreshAgentDurableIdentity` payload `{ sessionId: string; sessionType: FreshAgentSessionType; provider: FreshAgentRuntimeProvider; cliSessionId: string }` is identical in Task 2 (reducer + tests), Task 3 (dispatch `{ ...locator, cliSessionId }` where locator = `{ sessionId, sessionType, provider }`, and ws tests). `sessionRef` shape `{ provider, sessionId }` matches `SessionLocator` usage in `applyFreshAgentReconcileAttach` (`panesSlice.ts:1997-2024`) and `materializeFreshAgentPaneSession`. e2e reader `durableIdentity` matches the parity spec's proven expression. Consistent throughout.

## Self-Review addendum (2026-07-27 load-bearing validation pass)

The plan was hardened after an 11-assumption validation review (ledger: `.worktrees/.the-usual-logs/freshclaude-identity-persistence/load-bearing-ledger.md` — 9 verified, 2 falsified-and-fixed, 3 accepted residuals). Re-review of the edited tasks:

- **Spec coverage.** Both falsified assumptions are now handled in-plan: the init-before-created race (Task 3 Step 3b: freshAgentSlice upsert + created-time catch-up, all in-fence) and the resume-id-semantics correction (Task 2 test-4 comment + Archaeology scope notes). Task 1 Step 6 gives Task 6's decision protocol a real attribution baseline. ✔
- **1b. No silent deferrals.** Step 3b's slice upsert has a unit test; the FreshAgentView catch-up's observable outcome is Task 5 test 1's FOLD poll under the tightest-timing fixture (init emitted back-to-back with created) plus the `--repeat-each=2` flake check — the race path is exercised end-to-end, not stubbed. The zero-creates contingency in Task 5 is a PRE-DECLARED fallback assertion (still loud, still stale-ref-carrying), not a weakening escape hatch: `fresh` verdicts remain a STOP. ✔
- **Placeholder scan.** Step 3b's "same selector/key derivation the view already uses" is a verify-first pointer at existing code (same convention as the plan's other copy-by-reference instructions), with the reducer's no-op arm making double-dispatch safe regardless. No TBDs added. ✔
- **Type consistency.** The catch-up dispatches the SAME payload shape `{ sessionId, sessionType, provider, cliSessionId }` — no payload or reducer change was needed for the race fix; Task 2's reducer and tests are untouched by the addendum. ✔
