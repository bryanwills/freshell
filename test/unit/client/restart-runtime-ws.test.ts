import { configureStore } from '@reduxjs/toolkit'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import panesReducer from '@/store/panesSlice'
import freshAgentReducer from '@/store/freshAgentSlice'
import { WsClient } from '@/lib/ws-client'

class MockWebSocket {
  static OPEN = 1
  static instances: MockWebSocket[] = []

  readyState = MockWebSocket.OPEN
  onopen: null | (() => void) = null
  onmessage: null | ((ev: { data: string }) => void) = null
  onclose: null | ((ev: { code: number; reason: string }) => void) = null
  onerror: null | (() => void) = null
  sent: string[] = []

  constructor(_url: string) {
    MockWebSocket.instances.push(this)
  }

  send(data: unknown) {
    this.sent.push(String(data))
  }

  close() {
    this.onclose?.({ code: 1000, reason: '' })
  }

  message(message: unknown) {
    this.onmessage?.({ data: JSON.stringify(message) })
  }

  drop() {
    this.onclose?.({ code: 1006, reason: 'dropped' })
  }
}

function sent(socket: MockWebSocket) {
  return socket.sent.map((frame) => JSON.parse(frame))
}

function makeStore() {
  return configureStore({
    reducer: {
      panes: panesReducer,
      freshAgent: freshAgentReducer,
    },
    preloadedState: {
      panes: {
        layouts: {
          tab1: {
            type: 'leaf' as const,
            id: 'pane-1',
            content: {
              kind: 'terminal' as const,
              createRequestId: 'create-1',
              status: 'running' as const,
              mode: 'claude' as const,
              shell: 'system' as const,
              terminalId: 'terminal-old',
              runtimeId: 'terminal-old',
              runtimeGeneration: 7,
              sessionRef: { provider: 'claude', sessionId: 's1' },
            },
          },
        },
        activePane: { tab1: 'pane-1' },
        paneTitles: {},
        paneTitleSetByUser: {},
        renameRequestTabId: null,
        renameRequestPaneId: null,
        zoomedPane: {},
        refreshRequestsByPane: {},
        restoreFallbackAttemptsByPane: {},
      },
      freshAgent: {
        sessions: {},
        pendingCreates: {},
        pendingCreateFailures: {},
        availableModels: [],
      },
    },
  })
}

const request = {
  type: 'agent.restart' as const,
  requestId: 'restart-1',
  provider: 'claude',
  sessionId: 's1',
  kind: 'terminal' as const,
  liveId: 'terminal-old',
  expectedGeneration: 7,
}
const replaced = {
  type: 'agent.restart.replaced' as const,
  requestId: 'restart-1',
  provider: 'claude',
  sessionId: 's1',
  kind: 'terminal' as const,
  oldRuntimeId: 'terminal-old',
  oldGeneration: 7,
  runtimeId: 'terminal-new',
  generation: 8,
}

describe('WsClient restart transaction folding', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    MockWebSocket.instances = []
    // @ts-expect-error test transport
    globalThis.WebSocket = MockWebSocket
    localStorage.setItem('freshell.auth-token', 'token')
    ;(window as any).setTimeout = globalThis.setTimeout
    ;(window as any).clearTimeout = globalThis.clearTimeout
  })

  afterEach(() => {
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  it('folds one replacement centrally and does not create for a duplicate event', async () => {
    const store = makeStore()
    const client = new WsClient('ws://example/ws')
    client.bindAgentRestartStore(store)
    const promise = client.connect()
    const socket = MockWebSocket.instances[0]
    socket.onopen?.()
    socket.message({ type: 'ready', capabilities: { agentRestartV1: true } })
    await promise

    socket.message(replaced)
    socket.message(replaced)

    const content = store.getState().panes.layouts.tab1
    expect(content.type).toBe('leaf')
    if (content.type !== 'leaf' || content.content.kind !== 'terminal') throw new Error('expected terminal')
    expect(content.content.terminalId).toBe('terminal-new')
    expect(sent(socket).filter((frame) => frame.type === 'terminal.create')).toHaveLength(0)
  })

  it('resends an in-flight restart after ready and folds the stored terminal result once', async () => {
    const store = makeStore()
    const client = new WsClient('ws://example/ws')
    client.bindAgentRestartStore(store)
    const onReplacement = vi.fn()
    client.onMessage((message) => {
      if (message.type === 'agent.restart.replaced') {
        const content = store.getState().panes.layouts.tab1
        if (content.type !== 'leaf' || content.content.kind !== 'terminal') throw new Error('expected terminal')
        onReplacement(content.content.runtimeGeneration)
      }
    })

    const firstConnect = client.connect()
    const first = MockWebSocket.instances[0]
    first.onopen?.()
    first.message({ type: 'ready', capabilities: { agentRestartV1: true } })
    await firstConnect

    client.requestAgentRestart(request)
    expect(sent(first).filter((frame) => frame.type === 'agent.restart')).toEqual([request])
    first.drop()

    const reconnect = client.connect()
    const second = MockWebSocket.instances[1]
    second.onopen?.()
    second.message({ type: 'ready', capabilities: { agentRestartV1: true } })
    await reconnect
    expect(sent(second).filter((frame) => frame.type === 'agent.restart')).toEqual([request])

    second.message(replaced)
    second.message(replaced)
    expect(onReplacement).toHaveBeenCalledOnce()
    expect(onReplacement).toHaveBeenCalledWith(8)
    const content = store.getState().panes.layouts.tab1
    if (content.type !== 'leaf' || content.content.kind !== 'terminal') throw new Error('expected terminal')
    expect(content.content.runtimeGeneration).toBe(8)
  })

  it('does not replay a restart to a reconnected server that omitted the capability', async () => {
    const client = new WsClient('ws://example/ws')
    const firstConnect = client.connect()
    const first = MockWebSocket.instances[0]
    first.onopen?.()
    first.message({ type: 'ready', capabilities: { agentRestartV1: true } })
    await firstConnect
    client.requestAgentRestart(request)
    first.drop()

    const reconnect = client.connect()
    const downgraded = MockWebSocket.instances[1]
    downgraded.onopen?.()
    downgraded.message({ type: 'ready' })
    await reconnect

    expect(sent(downgraded).filter((frame) => frame.type === 'agent.restart')).toHaveLength(0)
  })

  it('refuses a new restart when the ready frame did not negotiate support', async () => {
    const client = new WsClient('ws://example/ws')
    const promise = client.connect()
    const socket = MockWebSocket.instances[0]
    socket.onopen?.()
    socket.message({ type: 'ready' })
    await promise

    expect(() => client.requestAgentRestart(request)).toThrow(/does not support/i)
    expect(sent(socket).filter((frame) => frame.type === 'agent.restart')).toHaveLength(0)
  })

  it('drops old-generation runtime frames after the replacement commits', async () => {
    const store = makeStore()
    const client = new WsClient('ws://example/ws')
    client.bindAgentRestartStore(store)
    const delivered = vi.fn()
    client.onMessage(delivered)
    const promise = client.connect()
    const socket = MockWebSocket.instances[0]
    socket.onopen?.()
    socket.message({ type: 'ready', capabilities: { agentRestartV1: true } })
    await promise
    delivered.mockClear()

    socket.message(replaced)
    socket.message({
      type: 'terminal.output',
      terminalId: 'terminal-old',
      data: 'stale',
      seqStart: 1,
      seqEnd: 1,
      streamId: 'old-stream',
      runtime: { runtimeId: 'terminal-old', generation: 7 },
    })
    socket.message({
      type: 'terminal.output',
      terminalId: 'terminal-new',
      data: 'current',
      seqStart: 1,
      seqEnd: 1,
      streamId: 'new-stream',
      runtime: { runtimeId: 'terminal-new', generation: 8 },
    })

    expect(delivered.mock.calls.map(([message]) => message.type)).toEqual([
      'agent.restart.replaced',
      'terminal.output',
    ])
    expect(delivered.mock.calls[1][0].data).toBe('current')
  })
})
