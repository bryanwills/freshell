# Restart Resumable Pane Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a context menu restart only its clicked resumable coding-agent pane, ending its live runtime and reopening the same durable conversation through the existing provider resume path.

**Architecture:** Add a pane-scoped restart intent with a snapshot of the matched pane’s canonical provider/session identity. Terminal and fresh-agent lifecycle code will consume it through an acknowledged server operation that terminates the current runtime before re-minting that same leaf’s create request; their existing creation paths remain responsible for provider-specific resume. Menu definitions select this intent only when an agent pane has a provider-matching durable session reference; all other panes keep the non-destructive refresh action.

**Tech Stack:** React 18, Redux Toolkit, TypeScript, WebSocket, node-pty, Vitest, Testing Library.

## Global Constraints

- The action affects exactly the right-clicked pane, never its tab or sibling panes.
- `Restart pane` appears only for terminal and fresh-agent coding-agent panes with a provider-matching canonical durable session reference and provider resume support.
- Shutdown must finish before the normal resume create path begins; resume arguments/adapters must not be duplicated.
- Shells, browsers, editors, extensions, non-resumable providers, and panes without canonical identity retain `Refresh pane`.
- Preserve existing `Refresh pane` behavior.
- Cover menu selection, pane isolation, terminal restart, fresh-agent restart, and non-eligible fallback behavior.

---

### Task 1: Represent and queue a pane-scoped restart request

**Files:**
- Modify: `src/store/paneTypes.ts`
- Modify: `src/lib/pane-utils.ts`
- Modify: `src/store/panesSlice.ts`
- Test: `test/unit/client/store/panesSlice.restart-pane.test.ts`

**Interfaces:**
- Produces `PaneRestartTarget`, `buildPaneRestartTarget(content, extensions)`, `paneRestartTargetMatchesContent(target, content)`, `requestPaneRestart({ tabId, paneId })`, and `consumePaneRestartRequest({ tabId, paneId, requestId })`.

- [ ] **Step 1: Write failing target/reducer tests**

```ts
it('queues a restart only for the requested matching terminal leaf', () => {
  const next = reducer(twoPaneState(), requestPaneRestart({ tabId: 'tab-1', paneId: 'pane-2' }))
  expect(next.restartRequestsByPane['tab-1']['pane-2'].target).toMatchObject({
    kind: 'terminal', provider: 'claude', sessionId: 'session-2', terminalId: 'term-2',
  })
  expect(next.restartRequestsByPane['tab-1']['pane-1']).toBeUndefined()
})
```

- [ ] **Step 2: Verify red**

Run: `npm run test:vitest -- run test/unit/client/store/panesSlice.restart-pane.test.ts --config config/vitest/vitest.config.ts`

Expected: FAIL because restart targets/actions do not exist.

- [ ] **Step 3: Implement target and reducer**

```ts
type PaneRestartTarget = {
  kind: 'terminal' | 'fresh-agent'
  createRequestId: string
  provider: string
  sessionId: string
  terminalId?: string
  liveSessionId?: string
}
// Require matching sessionRef plus live identity, queue it under
// restartRequestsByPane[tabId][paneId], and clear stale requests on content changes.
```

- [ ] **Step 4: Verify green**

Run: `npm run test:vitest -- run test/unit/client/store/panesSlice.restart-pane.test.ts --config config/vitest/vitest.config.ts`

Expected: PASS.

- [ ] **Step 5: Commit**

Run: `git add src/store/paneTypes.ts src/lib/pane-utils.ts src/store/panesSlice.ts test/unit/client/store/panesSlice.restart-pane.test.ts && git commit -m "feat: queue pane-scoped restart requests"`

### Task 2: Make runtime termination and resume creation one ordered protocol

**Files:**
- Modify: `src/lib/ws-client.ts`
- Modify: `server/ws-handler.ts`
- Modify: `server/terminal-registry.ts` (if a registry helper is needed)
- Modify: `server/fresh-agent/runtime-manager.ts` (if a manager helper is needed)
- Test: `test/integration/server/pane-restart-protocol.test.ts`

**Interfaces:**
- Consumes an authenticated pane identity, current terminal/fresh-agent live identity, and canonical session reference.
- Produces a success/failure acknowledgement only after terminal kill or fresh-agent interruption has completed.

- [ ] **Step 1: Write failing protocol tests**

```ts
it('kills the named terminal before acknowledging restart', async () => {
  await sendRestart({ tabId: 'tab-1', paneId: 'pane-2', terminalId: 'term-2' })
  expect(events).toEqual(['kill:term-2', 'restart-ack'])
  expect(registry.killAndWait).not.toHaveBeenCalledWith('term-1')
})
```

- [ ] **Step 2: Verify red**

Run: `npm run test:vitest -- run test/integration/server/pane-restart-protocol.test.ts --config config/vitest/vitest.server.config.ts`

Expected: FAIL because the restart protocol does not exist.

- [ ] **Step 3: Implement an acknowledged restart operation**

```ts
// Validate and authorize the immutable target using existing ownership checks.
// Await registry.killAndWait or runtimeManager.interrupt, log JSONL outcome,
// then send pane.restart.ready with the restart request ID.
```

- [ ] **Step 4: Verify green**

Run: `npm run test:vitest -- run test/integration/server/pane-restart-protocol.test.ts --config config/vitest/vitest.server.config.ts`

Expected: PASS.

- [ ] **Step 5: Commit**

Run: `git add src/lib/ws-client.ts server/ws-handler.ts server/terminal-registry.ts server/fresh-agent/runtime-manager.ts test/integration/server/pane-restart-protocol.test.ts && git commit -m "feat: add ordered pane restart protocol"`

### Task 3: Consume a ready restart in terminal and fresh-agent lifecycle code

**Files:**
- Modify: `src/components/TerminalView.tsx`
- Modify: `src/components/fresh-agent/FreshAgentView.tsx`
- Modify: `src/store/panesSlice.ts`
- Test: `test/unit/client/components/TerminalView.restart-pane.test.tsx`
- Test: `test/unit/client/components/fresh-agent/FreshAgentView.restart-pane.test.tsx`

**Interfaces:**
- Consumes the matched restart request and `pane.restart.ready` acknowledgement.
- Produces a new `createRequestId`, clears only the live runtime identity, retains `sessionRef`/resume identity, and consumes the request.

- [ ] **Step 1: Write failing view tests**

```tsx
it('recreates only the restarted terminal with its same canonical session reference', async () => {
  dispatch(requestPaneRestart({ tabId: 'tab-1', paneId: 'pane-2' }))
  deliverRestartReady('pane-2')
  await waitFor(() => expect(lastTerminalCreate()).toMatchObject({
    mode: 'claude', sessionRef: { provider: 'claude', sessionId: 'session-2' }, restore: true,
  }))
  expect(lastTerminalCreate()).not.toMatchObject({ paneId: 'pane-1' })
})
```

- [ ] **Step 2: Verify red**

Run: `npm run test:vitest -- run test/unit/client/components/TerminalView.restart-pane.test.tsx test/unit/client/components/fresh-agent/FreshAgentView.restart-pane.test.tsx --config config/vitest/vitest.config.ts`

Expected: FAIL because views only handle refresh requests.

- [ ] **Step 3: Implement matched, resume-preserving replacement**

```ts
// On pane.restart.ready, match target/current content, mint a new createRequestId,
// clear terminalId or fresh sessionId, set status:'creating' and
// pendingReconcile:'respawn', retaining canonical sessionRef. Existing creation
// functions then use their normal provider resume routes.
```

- [ ] **Step 4: Verify green**

Run: `npm run test:vitest -- run test/unit/client/components/TerminalView.restart-pane.test.tsx test/unit/client/components/fresh-agent/FreshAgentView.restart-pane.test.tsx --config config/vitest/vitest.config.ts`

Expected: PASS.

- [ ] **Step 5: Commit**

Run: `git add src/components/TerminalView.tsx src/components/fresh-agent/FreshAgentView.tsx src/store/panesSlice.ts test/unit/client/components/TerminalView.restart-pane.test.tsx test/unit/client/components/fresh-agent/FreshAgentView.restart-pane.test.tsx && git commit -m "feat: resume panes after ordered restart"`

### Task 4: Replace the eligible context-menu item and document it

**Files:**
- Modify: `src/components/context-menu/menu-defs.ts`
- Modify: `src/components/context-menu/ContextMenuProvider.tsx`
- Modify: `test/e2e/pane-context-menu-stability.test.tsx`
- Create: `test/unit/client/components/context-menu/restart-pane-actions.test.ts`
- Modify: `docs/index.html`

**Interfaces:**
- Consumes `buildPaneRestartTarget` and `requestPaneRestart`.
- Produces `MenuActions.restartPane(tabId, paneId)` and `restart-pane`/`Restart pane` for eligible pane, terminal, and fresh-agent contexts.

- [ ] **Step 1: Write failing menu tests**

```ts
it('replaces Refresh pane with Restart pane for resumable terminal and fresh-agent panes', () => {
  expect(labelsFor(resumableClaudeTerminal)).toContain('Restart pane')
  expect(labelsFor(resumableFreshCodex)).toContain('Restart pane')
  expect(labelsFor(resumableClaudeTerminal)).not.toContain('Refresh pane')
})

it('keeps Refresh pane for shell, browser, and unbound/non-resumable agent panes', () => {
  expect(labelsFor(shellPane)).toContain('Refresh pane')
  expect(labelsFor(browserPane)).toContain('Refresh pane')
  expect(labelsFor(unboundAgentPane)).toContain('Refresh pane')
})
```

- [ ] **Step 2: Verify red**

Run: `npm run test:vitest -- run test/unit/client/components/context-menu/restart-pane-actions.test.ts test/e2e/pane-context-menu-stability.test.tsx --config config/vitest/vitest.config.ts`

Expected: FAIL because menu definitions always select Refresh pane.

- [ ] **Step 3: Implement shared lifecycle-item selection**

```ts
const lifecycleItem = restartTarget
  ? { type: 'item', id: 'restart-pane', label: 'Restart pane', onSelect: () => actions.restartPane(tabId, paneId) }
  : { type: 'item', id: 'refresh-pane', label: 'Refresh pane', onSelect: () => actions.refreshPane(tabId, paneId), disabled: !canRefreshPane }
```

Use the helper in pane header, terminal body, and fresh-agent menu targets. Add the equivalent static user-facing note to `docs/index.html`.

- [ ] **Step 4: Verify green and lint**

Run: `npm run test:vitest -- run test/unit/client/components/context-menu/restart-pane-actions.test.ts test/e2e/pane-context-menu-stability.test.tsx --config config/vitest/vitest.config.ts && npm run lint`

Expected: PASS with no lint errors.

- [ ] **Step 5: Commit**

Run: `git add src/components/context-menu/menu-defs.ts src/components/context-menu/ContextMenuProvider.tsx test/e2e/pane-context-menu-stability.test.tsx test/unit/client/components/context-menu/restart-pane-actions.test.ts docs/index.html && git commit -m "feat: show restart action for resumable panes"`

### Task 5: Verify the integrated feature

**Files:**
- Test: all restart tests above

- [ ] **Step 1: Run focused regression coverage**

Run client: `npm run test:vitest -- run test/unit/client/store/panesSlice.restart-pane.test.ts test/unit/client/components/TerminalView.restart-pane.test.tsx test/unit/client/components/fresh-agent/FreshAgentView.restart-pane.test.tsx test/unit/client/components/context-menu/restart-pane-actions.test.ts test/e2e/pane-context-menu-stability.test.tsx --config config/vitest/vitest.config.ts`

Run server: `npm run test:vitest -- run test/integration/server/pane-restart-protocol.test.ts --config config/vitest/vitest.server.config.ts`

Expected: PASS.

- [ ] **Step 2: Run coordinated suite and build**

Run: `FRESHELL_TEST_SUMMARY='restart resumable pane feature' npm test && npm run build && git diff --check origin/main...HEAD`

Expected: all commands succeed and diff check is silent.

- [ ] **Step 3: Commit verification corrections if needed**

Run: `git add -A && git commit -m "test: verify resumable pane restart flow"`
