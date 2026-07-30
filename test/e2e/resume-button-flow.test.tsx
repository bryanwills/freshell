import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { Provider } from 'react-redux'
import { configureStore } from '@reduxjs/toolkit'
import Sidebar from '@/components/Sidebar'
import tabsReducer from '@/store/tabsSlice'
import panesReducer from '@/store/panesSlice'
import settingsReducer from '@/store/settingsSlice'
import connectionReducer from '@/store/connectionSlice'
import sessionsReducer from '@/store/sessionsSlice'
import sessionActivityReducer from '@/store/sessionActivitySlice'
import type { ResumeResolveResponse } from '@/lib/api'

// Scope queries to the dialog: the sidebar itself may render other role=status nodes.
function dialog() {
  return within(screen.getByRole('dialog', { name: /resume a session/i }))
}

const mockResolve = vi.fn<(input: string) => Promise<ResumeResolveResponse>>()
vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>()
  return { ...actual, resolveResumeInput: (input: string) => mockResolve(input) }
})

vi.mock('@/lib/ws-client', () => ({
  getWsClient: () => ({
    send: vi.fn(),
    onMessage: vi.fn(() => () => {}),
    connect: vi.fn().mockResolvedValue(undefined),
  }),
}))

const CLAUDE_V4 = 'ed2afda6-a340-443e-ba60-024a1b3554b4'
const AMPLIFIER_FULL = '417e8345-90ab-4cde-8f01-234567890abc'

function response(overrides: Partial<ResumeResolveResponse> = {}): ResumeResolveResponse {
  return { indexState: 'ready', tokens: [CLAUDE_V4], agentHint: null, homeDir: '/home/t', providerErrors: [], unsearchedProviders: [], matches: [], ...overrides }
}

function claudeMatch() {
  return {
    provider: 'claude' as const, sessionId: CLAUDE_V4, cwd: '/home/u/proj', projectPath: '/home/u/proj',
    sessionType: 'claude', title: 'claude one', lastActivityAt: 111,
    matchType: 'exact' as const, matchedToken: CLAUDE_V4,
  }
}

function makeStore() {
  return configureStore({
    reducer: {
      tabs: tabsReducer,
      panes: panesReducer,
      settings: settingsReducer,
      connection: connectionReducer,
      sessions: sessionsReducer,
      sessionActivity: sessionActivityReducer,
    },
    // sessions.expandedProjects is a Set in the real store too — the app's own
    // store config ignores it the same way (see e.g. mobile-sidebar-fullwidth-flow).
    middleware: (getDefault) =>
      getDefault({
        serializableCheck: {
          ignoredPaths: ['sessions.expandedProjects'],
        },
      }),
  })
}

// Render on a NON-terminal view: resume must both open the tab AND navigate
// to the terminal view (Sidebar convention), which view="terminal" would hide.
function renderApp() {
  const store = makeStore()
  const onNavigate = vi.fn()
  render(
    <Provider store={store}>
      <Sidebar view="tabs" onNavigate={onNavigate} />
    </Provider>,
  )
  return { store, onNavigate }
}

// The resumed TUPLE must be verified — not just tabs.length (an implementation
// that always opened a claude pane would otherwise pass every flow). The tuple
// includes sessionType: dropping/replacing the resolved sessionType would make
// openSessionTab create the wrong terminal runtime instead of a fresh-agent
// pane (src/lib/session-type-utils.ts buildPaneInput), so every flow asserts
// it. Verify the exact field placement against tabsSlice.openSessionTab
// (~lines 585-615: `resolvedSessionType = sessionType || provider` flows into
// the tab's session metadata and the pane content) and mirror how
// test/e2e/sidebar-click-opens-pane.test.tsx asserts pane/tab shape — if the
// sessionType lands on the PANE CONTENT rather than the tab, assert it there.
function expectResumedTab(
  store: ReturnType<typeof makeStore>,
  expected: { provider: string; sessionId: string; cwd?: string; sessionType?: string },
) {
  const tabs = store.getState().tabs.tabs
  expect(tabs).toHaveLength(1)
  expect(tabs[0]).toMatchObject({
    codingCliProvider: expected.provider,
    mode: expected.provider,
    ...(expected.cwd !== undefined ? { initialCwd: expected.cwd } : {}),
    sessionRef: { provider: expected.provider, sessionId: expected.sessionId },
  })
  expectTabSessionType(store, expected.sessionType ?? expected.provider)
}

// Assert the resolved sessionType survived into the opened tab/pane content
// (see comment above for where it lives — adjust the accessor, NOT the
// expectation, if the store shape differs).
function expectTabSessionType(store: ReturnType<typeof makeStore>, sessionType: string) {
  // Verified against tabsSlice.openSessionTab: the resolved sessionType lands in
  // the tab's sessionMetadataByKey entries, and pane contents live in
  // state.panes.layouts[tabId] (a PaneNode tree whose fresh-agent leaves carry
  // content.sessionType). Accessor matches the real store shape; the
  // expectation (the resolved sessionType survives) is non-negotiable.
  const state = store.getState()
  const tab = state.tabs.tabs[0] as any
  const metadataTypes = Object.values(tab.sessionMetadataByKey ?? {})
    .map((m: any) => m?.sessionType)
  const paneContentTypes: unknown[] = []
  const visit = (node: any) => {
    if (!node) return
    if (node.type === 'leaf') {
      paneContentTypes.push(node.content?.sessionType)
      return
    }
    visit(node.children?.[0])
    visit(node.children?.[1])
  }
  visit(state.panes?.layouts?.[tab.id])
  expect([tab.sessionType, ...metadataTypes, ...paneContentTypes]).toContain(sessionType)
}

async function openDialogAndResolve(text: string) {
  fireEvent.click(screen.getByTestId('sidebar-resume-button'))
  const input = screen.getByLabelText(/resume string/i)
  fireEvent.change(input, { target: { value: text } })
  fireEvent.keyDown(input, { key: 'Enter' })
  await act(async () => { await Promise.resolve(); await Promise.resolve() })
}

beforeEach(() => mockResolve.mockReset())
afterEach(() => cleanup())

describe('resume button end-to-end flows (spec acceptance)', () => {
  it('paste claude UUID with no hint → resolve finds claude → a claude tab opens AND navigates to terminal view', async () => {
    mockResolve.mockResolvedValue(response({ matches: [claudeMatch()] }))
    const { store, onNavigate } = renderApp()
    await openDialogAndResolve(CLAUDE_V4)
    expectResumedTab(store, { provider: 'claude', sessionId: CLAUDE_V4, cwd: '/home/u/proj' })
    expect(mockResolve).toHaveBeenCalledWith(CLAUDE_V4)
    // Sidebar convention: the resumed tab must become VISIBLE (we rendered view="tabs").
    expect(onNavigate).toHaveBeenCalledWith('terminal')
  })

  it('codex resume command → resolve finds codex → a CODEX tab opens (full id, correct provider)', async () => {
    const CODEX_V7 = '019fac27-69d7-78a0-b972-b339d551042e'
    mockResolve.mockResolvedValue(response({
      tokens: [CODEX_V7],
      agentHint: { provider: 'codex', source: 'command' },
      matches: [{ ...claudeMatch(), provider: 'codex' as const, sessionId: CODEX_V7, sessionType: 'codex', matchedToken: CODEX_V7 }],
    }))
    const { store } = renderApp()
    await openDialogAndResolve(`codex resume ${CODEX_V7}`)
    expect(mockResolve).toHaveBeenCalledWith(`codex resume ${CODEX_V7}`)
    expectResumedTab(store, { provider: 'codex', sessionId: CODEX_V7, cwd: '/home/u/proj' })
  })

  it('quoted claude --resume with a TRUNCATED id (spec: ed2afda6-…) and picker set to codex → prefix evidence wins → CLAUDE tab with the FULL id + note', async () => {
    // The spec's acceptance row pastes a truncated id — the flow must prove
    // prefix evidence resolves to the full session AND overrides the codex
    // picker. Using the full UUID here would prove neither.
    const TRUNCATED = 'ed2afda6-'
    mockResolve.mockResolvedValue(response({
      tokens: ['ed2afda6'],
      agentHint: { provider: 'claude', source: 'command' },
      matches: [{ ...claudeMatch(), matchType: 'prefix' as const, matchedToken: 'ed2afda6' }],
    }))
    const { store } = renderApp()
    fireEvent.click(screen.getByTestId('sidebar-resume-button'))
    fireEvent.change(screen.getByLabelText(/agent/i), { target: { value: 'codex' } })
    const input = screen.getByLabelText(/resume string/i)
    fireEvent.change(input, { target: { value: `"claude --resume ${TRUNCATED}"` } })
    fireEvent.keyDown(input, { key: 'Enter' })
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    expect(mockResolve).toHaveBeenCalledWith(`"claude --resume ${TRUNCATED}"`)
    expect(dialog().getByRole('status')).toHaveTextContent(/found in claude/i)
    expectResumedTab(store, { provider: 'claude', sessionId: CLAUDE_V4, cwd: '/home/u/proj' })
  })

  it('bare ses_ id with picker set to claude → evidence wins → opencode resume + note', async () => {
    const ocMatch = {
      provider: 'opencode' as const, sessionId: 'ses_root0000000000000000000000',
      cwd: '/home/u/oc', projectPath: '/home/u/oc', sessionType: 'opencode',
      title: 'oc root', lastActivityAt: 5, matchType: 'exact' as const,
      matchedToken: 'ses_root0000000000000000000000',
    }
    mockResolve.mockResolvedValue(response({ tokens: [ocMatch.sessionId], matches: [ocMatch] }))
    const { store } = renderApp()
    fireEvent.click(screen.getByTestId('sidebar-resume-button'))
    fireEvent.change(screen.getByLabelText(/agent/i), { target: { value: 'claude' } })
    const input = screen.getByLabelText(/resume string/i)
    fireEvent.change(input, { target: { value: ocMatch.sessionId } })
    fireEvent.keyDown(input, { key: 'Enter' })
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    expect(dialog().getByRole('status')).toHaveTextContent(/found in opencode/i)
    // Evidence wins over the picker: the opened tab must be the OPENCODE tuple.
    expectResumedTab(store, { provider: 'opencode', sessionId: ocMatch.sessionId, cwd: '/home/u/oc' })
  })

  it('prefix 417e8345 resolves to the amplifier session (FULL id, amplifier tuple)', async () => {
    mockResolve.mockResolvedValue(response({
      tokens: ['417e8345'],
      matches: [{ ...claudeMatch(), provider: 'amplifier' as const, sessionId: AMPLIFIER_FULL, sessionType: 'amplifier', matchType: 'prefix' as const, matchedToken: '417e8345' }],
    }))
    const { store } = renderApp()
    await openDialogAndResolve('417e8345')
    expectResumedTab(store, { provider: 'amplifier', sessionId: AMPLIFIER_FULL, cwd: '/home/u/proj' })
  })

  it('a FRESH-AGENT sessionType survives resume end to end: freshclaude match opens a freshclaude pane, not a bare claude terminal', async () => {
    // Hard requirement: sessions opened via freshclaude/freshopencode/kilroy
    // must reopen through that runtime. Every other flow has
    // sessionType === provider, so ONLY this test would catch an
    // implementation that drops or replaces the resolved sessionType
    // (openSessionTab would then build the wrong pane runtime — see
    // src/lib/session-type-utils.ts buildPaneInput).
    mockResolve.mockResolvedValue(response({
      matches: [{ ...claudeMatch(), sessionType: 'freshclaude' }],
    }))
    const { store } = renderApp()
    await openDialogAndResolve(CLAUDE_V4)
    expectResumedTab(store, {
      provider: 'claude', sessionId: CLAUDE_V4, cwd: '/home/u/proj', sessionType: 'freshclaude',
    })
    // Belt-and-braces: the fresh-agent runtime, not a plain terminal pane.
    expectTabSessionType(store, 'freshclaude')
  })

  it('DEGRADED response with a single full match: NO auto-resume, NO tab — match listed for manual confirmation', async () => {
    // A failed provider means a higher-priority exact match may have been
    // missed; auto-resuming the surviving match could open the WRONG session.
    mockResolve.mockResolvedValue(response({
      indexState: 'degraded', providerErrors: ['claude'], matches: [claudeMatch()],
    }))
    const { store } = renderApp()
    await openDialogAndResolve(CLAUDE_V4)
    expect(store.getState().tabs.tabs).toHaveLength(0)
    expect(dialog().getByRole('status')).toHaveTextContent(/could not be searched/i)
    expect(dialog().getByRole('list', { name: /matching sessions/i })).toBeInTheDocument()
  })

  it('session already open in a pane → focuses it, no duplicate tab', async () => {
    mockResolve.mockResolvedValue(response({ matches: [claudeMatch()] }))
    const { store } = renderApp()
    await openDialogAndResolve(CLAUDE_V4)
    expectResumedTab(store, { provider: 'claude', sessionId: CLAUDE_V4 })
    const firstTabId = store.getState().tabs.tabs[0].id
    // Resume the SAME session again.
    cleanup()
    render(
      <Provider store={store}>
        <Sidebar view="tabs" onNavigate={vi.fn()} />
      </Provider>,
    )
    await openDialogAndResolve(CLAUDE_V4)
    const tabs = store.getState().tabs.tabs
    expect(tabs).toHaveLength(1)
    expect(store.getState().tabs.activeTabId).toBe(firstTabId)
  })

  it('garbage with no id-like token → inline error, NO tab created', async () => {
    mockResolve.mockResolvedValue(response({ tokens: [], matches: [] }))
    const { store } = renderApp()
    await openDialogAndResolve('garbage text with no ids')
    expect(dialog().getByRole('alert')).toHaveTextContent(/no session id/i)
    expect(store.getState().tabs.tabs).toHaveLength(0)
  })

  it('valid id while index warming → retry state, no error, NO tab', async () => {
    mockResolve.mockResolvedValue(response({ indexState: 'warming', matches: [] }))
    const { store } = renderApp()
    await openDialogAndResolve(CLAUDE_V4)
    expect(dialog().queryByRole('alert')).toBeNull()
    expect(dialog().getByRole('status')).toHaveTextContent(/warming/i)
    expect(store.getState().tabs.tabs).toHaveLength(0)
  })
})
