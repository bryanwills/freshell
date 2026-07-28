import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act, render, screen, fireEvent, cleanup } from '@testing-library/react'
import { configureStore } from '@reduxjs/toolkit'
import { Provider } from 'react-redux'
import tabsReducer from '@/store/tabsSlice'
import panesReducer from '@/store/panesSlice'
import settingsReducer, { defaultSettings } from '@/store/settingsSlice'
import connectionReducer from '@/store/connectionSlice'
import terminalLifecycleReducer, { AUTO_RESUME_NOTICE_TTL_MS, selectExitRecordFrom } from '@/store/terminalLifecycleSlice'
import { updatePaneContent } from '@/store/panesSlice'
import { resetPersistedLayoutCacheForTests, resetPersistFlushListenersForTests } from '@/store/persistMiddleware'
import type { PaneNode, TerminalPaneContent } from '@/store/paneTypes'
import { __resetTerminalCursorCacheForTests } from '@/lib/terminal-cursor'
import { resetHydrationQueueForTests } from '@/lib/hydration-queue'
import { installPerfAuditBridge } from '@/lib/perf-audit-bridge'
import {
  composeResolvedSettings,
  createDefaultServerSettings,
  resolveLocalSettings,
} from '@shared/settings'

// Store + render harness mirrored from TerminalView.launchRetry.test.tsx
// (hoisted ws/xterm/lucide mocks, beforeEach/afterEach resets), trimmed to
// what an already-settled pane needs: these suites never replay attach
// streams, so the attachRequestId bookkeeping is omitted.

const wsMocks = vi.hoisted(() => ({
  send: vi.fn(),
  connect: vi.fn().mockResolvedValue(undefined),
  onMessage: vi.fn(),
  onReconnect: vi.fn().mockReturnValue(() => {}),
}))

const terminalThemeMocks = vi.hoisted(() => ({
  getTerminalTheme: vi.fn(() => ({})),
}))

vi.mock('@/lib/ws-client', () => ({
  getWsClient: () => ({
    send: wsMocks.send,
    connect: wsMocks.connect,
    onMessage: wsMocks.onMessage,
    onReconnect: wsMocks.onReconnect,
  }),
}))

vi.mock('@/lib/terminal-themes', () => ({
  getTerminalTheme: terminalThemeMocks.getTerminalTheme,
}))

vi.mock('lucide-react', () => ({
  Loader2: ({ className }: { className?: string }) => <svg data-testid="loader" className={className} />,
}))

vi.mock('@xterm/xterm', () => {
  class MockTerminal {
    options: Record<string, unknown> = {}
    cols = 80
    rows = 24
    open = vi.fn()
    loadAddon = vi.fn()
    registerLinkProvider = vi.fn(() => ({ dispose: vi.fn() }))
    write = vi.fn((_data: string, onWritten?: () => void) => {
      onWritten?.()
    })
    writeln = vi.fn()
    clear = vi.fn()
    dispose = vi.fn()
    onData = vi.fn()
    onTitleChange = vi.fn(() => ({ dispose: vi.fn() }))
    attachCustomKeyEventHandler = vi.fn()
    attachCustomWheelEventHandler = vi.fn()
    getSelection = vi.fn(() => '')
    focus = vi.fn()
  }

  return { Terminal: MockTerminal }
})

vi.mock('@xterm/addon-fit', () => ({
  FitAddon: class {
    fit = vi.fn()
  },
}))

vi.mock('@xterm/xterm/css/xterm.css', () => ({}))

import TerminalView, { __resetLastSentViewportCacheForTests } from '@/components/TerminalView'
import { resetEnsureExtensionsRegistryCacheForTests } from '@/hooks/useEnsureExtensionsRegistry'

class MockResizeObserver {
  observe = vi.fn()
  disconnect = vi.fn()
  unobserve = vi.fn()
}

let messageHandler: ((msg: any) => void) | null = null
let requestAnimationFrameSpy: ReturnType<typeof vi.spyOn> | null = null
let cancelAnimationFrameSpy: ReturnType<typeof vi.spyOn> | null = null

const REQ = 'req-exit-banner'
const TAB = 'tab-exit-banner'
const PANE = 'pane-exit-banner'
const SESSION_ID = 'sess-keep'

function createSettingsState() {
  const serverSettings = createDefaultServerSettings({ loggingDebug: defaultSettings.logging.debug })
  const localSettings = resolveLocalSettings()
  return {
    serverSettings,
    localSettings,
    settings: composeResolvedSettings(serverSettings, localSettings),
    loaded: true,
    lastSavedAt: undefined,
  }
}

interface StoreOptions {
  mode?: string
  status?: TerminalPaneContent['status']
  withSessionRef?: boolean
  lifecycle?: {
    lastTerminalId?: string
    exit?: { exitCode: number; at: number }
    notice?: { kind: 'recovering' | 'resumed'; attempt: number; maxAttempts: number; exitCode: number; at: number }
  }
}

function makeStore(opts: StoreOptions = {}) {
  const mode = opts.mode ?? 'claude'
  const paneContent: TerminalPaneContent = {
    kind: 'terminal',
    createRequestId: REQ,
    status: opts.status ?? 'exited',
    mode: mode as TerminalPaneContent['mode'],
    shell: 'system',
    ...(opts.withSessionRef === false
      ? {}
      : { sessionRef: { provider: mode, sessionId: SESSION_ID } }),
  }
  const root: PaneNode = { type: 'leaf', id: PANE, content: paneContent }
  const store = configureStore({
    reducer: {
      tabs: tabsReducer,
      panes: panesReducer,
      settings: settingsReducer,
      connection: connectionReducer,
      terminalLifecycle: terminalLifecycleReducer,
    },
    preloadedState: {
      tabs: {
        tabs: [{
          id: TAB, mode, status: paneContent.status, title: 'Agent',
          titleSetByUser: false, createRequestId: REQ,
        }],
        activeTabId: TAB,
      },
      panes: { layouts: { [TAB]: root }, activePane: { [TAB]: PANE }, paneTitles: {} },
      settings: createSettingsState(),
      connection: { status: 'connected', error: null },
      terminalLifecycle: {
        byPaneId: opts.lifecycle ? { [PANE]: opts.lifecycle } : {},
      },
    } as any,
  })
  return { store, paneContent }
}

function paneState(store: ReturnType<typeof makeStore>['store']) {
  const layout = store.getState().panes.layouts[TAB] as { type: 'leaf'; content: any }
  return layout.content
}

async function renderPane(store: any, paneContent: TerminalPaneContent) {
  render(
    <Provider store={store}>
      <TerminalView tabId={TAB} paneId={PANE} paneContent={paneContent} />
    </Provider>
  )
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  expect(messageHandler).not.toBeNull()
}

describe('TerminalView exited-pane error banner', () => {
  beforeEach(() => {
    __resetTerminalCursorCacheForTests()
    __resetLastSentViewportCacheForTests()
    resetHydrationQueueForTests()
    resetPersistedLayoutCacheForTests()
    resetPersistFlushListenersForTests()
    wsMocks.send.mockClear()
    terminalThemeMocks.getTerminalTheme.mockReset()
    terminalThemeMocks.getTerminalTheme.mockReturnValue({})
    resetEnsureExtensionsRegistryCacheForTests()
    wsMocks.onMessage.mockImplementation((callback: (msg: any) => void) => {
      messageHandler = callback
      return () => { messageHandler = null }
    })
    requestAnimationFrameSpy = vi.spyOn(window, 'requestAnimationFrame').mockImplementation((cb: FrameRequestCallback) => {
      cb(0)
      return 1
    })
    cancelAnimationFrameSpy = vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => {})
    vi.stubGlobal('ResizeObserver', MockResizeObserver)
    installPerfAuditBridge(null)
  })

  afterEach(() => {
    cleanup()
    vi.useRealTimers()
    vi.unstubAllGlobals()
    __resetTerminalCursorCacheForTests()
    resetHydrationQueueForTests()
    requestAnimationFrameSpy?.mockRestore()
    cancelAnimationFrameSpy?.mockRestore()
    requestAnimationFrameSpy = null
    cancelAnimationFrameSpy = null
    installPerfAuditBridge(null)
  })

  it('shows the alert error bar for an agent pane settled exited with a non-zero exit record', async () => {
    const { store, paneContent } = makeStore({
      mode: 'claude',
      status: 'exited',
      lifecycle: { exit: { exitCode: 1, at: Date.now() } },
    })
    await renderPane(store, paneContent)

    const bar = screen.getByRole('alert')
    expect(bar).toHaveTextContent('process exited (code 1)')
    expect(screen.getByRole('button', { name: 'Relaunch claude session' })).toBeInTheDocument()
  })

  it('Relaunch resets the pane for a respawn create with the SAME sessionRef', async () => {
    const { store, paneContent } = makeStore({
      mode: 'claude',
      status: 'exited',
      lifecycle: { exit: { exitCode: 1, at: Date.now() } },
    })
    await renderPane(store, paneContent)

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Relaunch claude session' }))
    })

    const content = paneState(store)
    expect(content.status).toBe('creating')
    expect(content.pendingReconcile).toBe('respawn')
    expect(content.sessionRef?.sessionId).toBe(SESSION_ID) // unchanged from seed
    expect(content.terminalId).toBeUndefined()
  })

  it('Relaunch clears the stale exit record so a failed relaunch does not resurrect the old crash banner', async () => {
    const { store, paneContent } = makeStore({
      mode: 'claude',
      status: 'exited',
      lifecycle: { exit: { exitCode: 1, at: Date.now() } },
    })
    const { rerender } = render(
      <Provider store={store}>
        <TerminalView tabId={TAB} paneId={PANE} paneContent={paneContent} />
      </Provider>
    )
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    expect(messageHandler).not.toBeNull()

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Relaunch claude session' }))
    })

    // The click must discard the PREVIOUS crash's record — a genuine
    // crash-during-relaunch still repopulates via recordTerminalExit.
    expect(selectExitRecordFrom(store.getState().terminalLifecycle, PANE)).toBeUndefined()

    // Relaunch create rejected: the pane settles 'error' with NO new
    // terminal.exit. The stale "process exited (code 1)" must not linger.
    act(() => {
      store.dispatch(updatePaneContent({
        tabId: TAB,
        paneId: PANE,
        content: { ...paneState(store), terminalId: undefined, streamId: undefined, status: 'error' },
      }))
    })
    rerender(
      <Provider store={store}>
        <TerminalView tabId={TAB} paneId={PANE} paneContent={paneState(store)} />
      </Provider>
    )
    await act(async () => {
      await Promise.resolve()
    })

    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('keeps shell panes quiet: no alert for an exited shell even with a non-zero exit record', async () => {
    const { store, paneContent } = makeStore({
      mode: 'shell',
      status: 'exited',
      withSessionRef: false,
      lifecycle: { exit: { exitCode: 1, at: Date.now() } },
    })
    await renderPane(store, paneContent)

    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('keeps clean exits quiet: no alert for an agent pane with exit code 0 (D-3)', async () => {
    const { store, paneContent } = makeStore({
      mode: 'claude',
      status: 'exited',
      lifecycle: { exit: { exitCode: 0, at: Date.now() } },
    })
    await renderPane(store, paneContent)

    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('shows a codeless alert with Relaunch for an exited agent pane with NO exit record (post-reload)', async () => {
    const { store, paneContent } = makeStore({
      mode: 'claude',
      status: 'exited',
      // ephemeral slice is empty after a page reload
    })
    await renderPane(store, paneContent)

    const bar = screen.getByRole('alert')
    expect(bar).toHaveTextContent('process exited')
    expect(bar).not.toHaveTextContent('(code')
    expect(screen.getByRole('button', { name: 'Relaunch claude session' })).toBeInTheDocument()
  })

  it("treats a status-'error' agent pane WITH a non-zero exit record as a crash (alert + Relaunch); without a record it stays quiet", async () => {
    // Crash before terminal.attach.ready settles via failLaunch as 'error'
    // (crash-during-launch) — same user situation as an 'exited' crash.
    const crashed = makeStore({
      mode: 'claude',
      status: 'error',
      lifecycle: { exit: { exitCode: 1, at: Date.now() } },
    })
    await renderPane(crashed.store, crashed.paneContent)

    expect(screen.getByRole('alert')).toHaveTextContent('process exited (code 1)')
    expect(screen.getByRole('button', { name: 'Relaunch claude session' })).toBeInTheDocument()

    cleanup()
    messageHandler = null

    // Plain launch failure (create rejected — no exit record) keeps today's
    // presentation: no alert.
    const plainFailure = makeStore({ mode: 'claude', status: 'error' })
    await renderPane(plainFailure.store, plainFailure.paneContent)

    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('degrades an orphaned recovering notice to the alert deterministically at TTL expiry (silent settle backstop)', async () => {
    vi.useFakeTimers()
    const at = Date.now()
    const { store, paneContent } = makeStore({
      mode: 'claude',
      status: 'exited',
      lifecycle: {
        exit: { exitCode: 1, at },
        notice: { kind: 'recovering', attempt: 1, maxAttempts: 2, exitCode: 1, at },
      },
    })
    await renderPane(store, paneContent)

    // While the notice is active: notice strip, no alert. (Text-anchored:
    // TerminalView also renders an unrelated role='status' offline strip in
    // this harness; the banner's role='status' semantics are covered by
    // TerminalExitBanner.test.tsx.)
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.getByText('claude crashed (exit 1) — auto-resuming, attempt 1/2')).toBeInTheDocument()

    // No frame ever arrives (respawn_failed / lease-held / owned-live settle
    // silently). The scheduled re-render must flip notice → alert on its own.
    await act(async () => {
      vi.advanceTimersByTime(AUTO_RESUME_NOTICE_TTL_MS + 2)
    })

    expect(screen.queryByText('claude crashed (exit 1) — auto-resuming, attempt 1/2')).toBeNull()
    expect(screen.getByRole('alert')).toHaveTextContent('process exited (code 1)')
  })

  it('renders the recovering notice from the frame FIELDS — prose is presentational, never parsed', async () => {
    // Council MEDIUM fix (7w4h/xkhx review): the client must read
    // attempt/maxAttempts/exitCode from the terminal.status frame's typed
    // fields. The reason prose here is DELIBERATELY reworded so any regex
    // parse of it ("attempt n/m", "exit N") finds nothing — if the banner
    // still shows invented defaults (1/2, exit 1), prose is load-bearing.
    const at = Date.now()
    const { store, paneContent } = makeStore({
      mode: 'claude',
      status: 'exited',
      lifecycle: {
        lastTerminalId: 'term-crashed',
        exit: { exitCode: 137, at },
      },
    })
    await renderPane(store, paneContent)

    act(() => {
      messageHandler!({
        type: 'terminal.status',
        terminalId: 'term-crashed',
        status: 'recovering',
        attempt: 1,
        maxAttempts: 3,
        exitCode: 137,
        reason: 'claude quit unexpectedly and is being brought back',
      })
    })

    expect(
      screen.getByText('claude crashed (exit 137) — auto-resuming, attempt 1/3')
    ).toBeInTheDocument()
  })
})
