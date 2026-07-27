// @vitest-environment node
// Terminal-stream <-> freshAgent backpressure coupling (kata zrrj, Tasks 19+20).
//
// Terminal output and freshAgent lifecycle events share one WebSocket with
// asymmetric backpressure policy: WsHandler.send drops the message AND closes
// the socket with 4008 when bufferedAmount > 2 MiB, while the broker's live
// output path historically wrote with no foreground buffered-amount gate (its
// own kill line is the 16 MiB/10 s catastrophic path). Task 20 adds a 1 MiB
// foreground pause in the broker so terminal traffic alone can never inflate
// bufferedAmount past the handler's kill line.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import http from 'http'
import WebSocket from 'ws'
import { WsHandler } from '../../../server/ws-handler'
import { TerminalRegistry } from '../../../server/terminal-registry'
import { TerminalStreamBroker } from '../../../server/terminal-stream/broker'
import {
  createMockWs,
  FakeBrokerRegistry,
  structuredLogsFrom,
  type MockWs,
} from '../../helpers/ws-backpressure'

const loggerMocks = vi.hoisted(() => {
  const logger = {
    child: vi.fn(),
    debug: vi.fn(),
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  }
  logger.child.mockReturnValue(logger)
  return { logger }
})

vi.mock('../../../server/logger', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../../server/logger')>()
  return {
    ...actual,
    logger: loggerMocks.logger,
    sessionLifecycleLogger: loggerMocks.logger,
    freshAgentObservabilityLogger: loggerMocks.logger,
  }
})

vi.mock('node-pty', () => ({
  spawn: vi.fn(),
}))

const TEST_AUTH_TOKEN = 'testtoken-testtoken'
const KILL_LINE_BYTES = 2 * 1024 * 1024

const structuredLogs = (level: 'debug' | 'info' | 'warn' | 'error', event: string) =>
  structuredLogsFrom(loggerMocks.logger, level, event)

function sentPayloads(ws: MockWs): Record<string, unknown>[] {
  return ws.send.mock.calls
    .map(([raw]) => (typeof raw === 'string' ? JSON.parse(raw) : raw))
    .filter((payload): payload is Record<string, unknown> => !!payload && typeof payload === 'object')
}

function sentTypes(ws: MockWs): unknown[] {
  return sentPayloads(ws).map((payload) => payload.type)
}

function terminalOutputFrames(ws: MockWs): Record<string, unknown>[] {
  return sentPayloads(ws).filter((payload) => payload.type === 'terminal.output')
}

async function flushMicrotasks(times = 6): Promise<void> {
  for (let i = 0; i < times; i += 1) {
    await Promise.resolve()
  }
}

type CapturingRuntimeManager = {
  manager: { subscribe: ReturnType<typeof vi.fn> }
  listeners: Map<string, (event: unknown) => void>
  off: ReturnType<typeof vi.fn>
}

function createCapturingRuntimeManager(): CapturingRuntimeManager {
  const listeners = new Map<string, (event: unknown) => void>()
  const off = vi.fn()
  const manager = {
    subscribe: vi.fn().mockImplementation(async (locator: unknown, listener: (event: unknown) => void) => {
      listeners.set(JSON.stringify(locator), listener)
      return off
    }),
  }
  return { manager, listeners, off }
}

/** Just the ClientState fields the freshAgent subscription path touches. */
function freshAgentClientStateStub() {
  return {
    freshAgentSubscriptions: new Map(),
    freshAgentAuthorizations: new Map(),
    pendingFreshAgentAttachByKey: new Map(),
  }
}

const LOCATOR = { sessionId: 'ses_1', sessionType: 'freshclaude', provider: 'claude' }

describe('terminal output vs freshAgent lifecycle on one socket (zrrj)', () => {
  let originalAuthToken: string | undefined
  let server: http.Server | undefined
  let terminalRegistry: TerminalRegistry | undefined
  let handler: WsHandler | undefined
  let broker: TerminalStreamBroker | undefined

  beforeEach(() => {
    originalAuthToken = process.env.AUTH_TOKEN
    process.env.AUTH_TOKEN = TEST_AUTH_TOKEN
    loggerMocks.logger.debug.mockClear()
    loggerMocks.logger.info.mockClear()
    loggerMocks.logger.warn.mockClear()
    loggerMocks.logger.error.mockClear()
    vi.useFakeTimers()
  })

  afterEach(async () => {
    broker?.close()
    broker = undefined
    handler?.close()
    handler = undefined
    terminalRegistry?.shutdown()
    terminalRegistry = undefined
    if (server?.listening) {
      await new Promise<void>((resolve) => server!.close(() => resolve()))
    }
    server = undefined
    vi.clearAllTimers()
    vi.useRealTimers()
    if (originalAuthToken === undefined) {
      delete process.env.AUTH_TOKEN
    } else {
      process.env.AUTH_TOKEN = originalAuthToken
    }
  })

  async function createHandlerWithFreshAgentSubscription(ws: MockWs) {
    const { manager, listeners, off } = createCapturingRuntimeManager()
    server = http.createServer()
    await new Promise<void>((resolve) => server!.listen(0, '127.0.0.1', () => resolve()))
    terminalRegistry = new TerminalRegistry()
    handler = new WsHandler(server, terminalRegistry, { freshAgentRuntimeManager: manager } as never)

    const state = freshAgentClientStateStub()
    ;(handler as any).ensureFreshAgentSubscription(ws, state, LOCATOR)
    await flushMicrotasks()
    const fireEvent = listeners.get(JSON.stringify(LOCATOR))
    expect(fireEvent).toBeTypeOf('function')
    return { state, off, fireEvent: fireEvent! }
  }

  it('delivers freshAgent.turn.complete while flooding terminal output (broker self-throttles below the kill line)', async () => {
    // DESIRED behavior (RED until Task 20): the buffered pressure is PRODUCED
    // BY THE BROKER (the thing Task 20 gates) - never seeded statically,
    // because WsHandler.send keeps its 4008 close after the fix.
    const ws = createMockWs()
    // Accumulate-bytes idiom: the socket never drains.
    ws.send.mockImplementation((frame: string) => {
      ws.bufferedAmount += Buffer.byteLength(frame, 'utf8')
    })

    const { fireEvent } = await createHandlerWithFreshAgentSubscription(ws)

    // Foreground attachment on an EMPTY terminal (no replay cursor - the
    // replay path is already paced at 576 KiB and would make this vacuous).
    const brokerRegistry = new FakeBrokerRegistry()
    broker = new TerminalStreamBroker(brokerRegistry as any, vi.fn())
    brokerRegistry.createTerminal('term-flood')
    const attached = await broker.attach(
      ws as any,
      'term-flood',
      'viewport_hydrate',
      80,
      24,
      0,
      'flood-attach',
      undefined,
      'foreground',
    )
    expect(attached).toBe('attached')

    // Flood > 4 MiB of live output.
    const chunk = 'x'.repeat(8 * 1024)
    for (let i = 0; i < 640; i += 1) {
      brokerRegistry.emit('terminal.output.raw', { terminalId: 'term-flood', data: chunk, at: Date.now() })
    }
    for (let i = 0; i < 900; i += 1) {
      vi.runOnlyPendingTimers()
    }

    // RED today: broker live sends bypass any foreground buffered gate
    // (safeSendPrepared passes no options), so bufferedAmount inflates to the
    // full ~5 MiB flood - past the handler's 2 MiB kill line.
    // GREEN after Task 20: the broker pauses at 1 MiB, so bufferedAmount stays
    // below the kill line and lifecycle frames survive.
    expect(ws.bufferedAmount).toBeLessThan(KILL_LINE_BYTES)

    fireEvent({ type: 'freshAgent.turn.complete', sessionId: 'ses_1', at: Date.now() })
    expect(sentTypes(ws)).toContain('freshAgent.event')
    expect(ws.close).not.toHaveBeenCalledWith(4008, expect.anything())
  })

  it('EVIDENCE: retention_lost is log-only and emits zero WS frames in the current build', async () => {
    const brokerRegistry = new FakeBrokerRegistry()
    brokerRegistry.setReplayRingMaxBytes(1024)
    broker = new TerminalStreamBroker(brokerRegistry as any, vi.fn())
    brokerRegistry.createTerminal('term-retention')

    const ws = createMockWs()
    const attached = await broker.attach(ws as any, 'term-retention', 'viewport_hydrate', 80, 24, 0, 'retention-attach')
    expect(attached).toBe('attached')

    // Flood terminal.output.raw past the 1024-byte ring. Retention loss is
    // handled synchronously inside the append path; no timers run here, so any
    // WS frame observed below could only have come from retention itself.
    const sendCallsBefore = ws.send.mock.calls.length
    for (let i = 0; i < 8; i += 1) {
      brokerRegistry.emit('terminal.output.raw', { terminalId: 'term-retention', data: 'y'.repeat(512), at: Date.now() })
    }

    const retentionLogs = structuredLogs('warn', 'terminal.replay.retention')
    expect(retentionLogs.length).toBeGreaterThanOrEqual(1)
    expect(retentionLogs[0]).toMatchObject({
      terminalId: 'term-retention',
      reason: 'retention_lost',
    })
    // Log-only: zero send-count delta from retention itself...
    expect(ws.send.mock.calls.length - sendCallsBefore).toBe(0)

    // ...and even after the queued output flushes normally, retention never
    // produced a terminal.stream.changed frame (the pre-ff8589bb amplifier).
    for (let i = 0; i < 50; i += 1) {
      vi.runOnlyPendingTimers()
    }
    expect(sentTypes(ws)).not.toContain('terminal.stream.changed')
    expect(ws.close).not.toHaveBeenCalled()
  })

  it('pauses a foreground live flush at 3 MiB bufferedAmount and retries on the 100 ms background-style timer', async () => {
    // Task 20's flipped test 3. RED today: the broker flushes to a foreground
    // attachment with NO bufferedAmount gate, so the frame is sent straight
    // into a 3 MiB-deep socket. GREEN after Task 20: the flush pauses below
    // the handler kill line and retries via the 100 ms background-pause
    // mechanic (TERMINAL_BACKGROUND_RETRY_FLUSH_MS), NOT the generic 50 ms
    // TERMINAL_STREAM_RETRY_FLUSH_MS.
    const brokerRegistry = new FakeBrokerRegistry()
    broker = new TerminalStreamBroker(brokerRegistry as any, vi.fn())
    brokerRegistry.createTerminal('term-fg-pause')

    const ws = createMockWs({ bufferedAmount: 3 * 1024 * 1024 })
    const attached = await broker.attach(ws as any, 'term-fg-pause', 'viewport_hydrate', 80, 24, 0, 'fg-pause-attach', undefined, 'foreground')
    expect(attached).toBe('attached')

    brokerRegistry.emit('terminal.output.raw', { terminalId: 'term-fg-pause', data: 'fg-paused-output;', at: Date.now() })
    // Run the immediate (0 ms) flush: it must pause, not send.
    vi.runOnlyPendingTimers()
    expect(terminalOutputFrames(ws)).toHaveLength(0)
    expect(ws.close).not.toHaveBeenCalled()

    // Drain the socket right away: any retry timer shorter than 100 ms would
    // now flush early. 99 ms in, nothing may have been sent.
    ws.bufferedAmount = 0
    vi.advanceTimersByTime(99)
    expect(terminalOutputFrames(ws)).toHaveLength(0)

    // At exactly 100 ms the paused flush retries and delivers the output.
    vi.advanceTimersByTime(1)
    const outputs = terminalOutputFrames(ws)
    expect(outputs.length).toBeGreaterThanOrEqual(1)
    expect(outputs.map((payload) => String(payload.data)).join('')).toContain('fg-paused-output;')
    expect(ws.close).not.toHaveBeenCalled()
  })

  it('EVIDENCE: after a 4008 backpressure close, freshAgent events are dropped with no replay', async () => {
    const ws = createMockWs({ bufferedAmount: 3 * 1024 * 1024 })
    // Real sockets leave OPEN once close() is initiated.
    ws.close.mockImplementation(() => {
      ws.readyState = WebSocket.CLOSED
    })

    const { state, off, fireEvent } = await createHandlerWithFreshAgentSubscription(ws)

    // A freshAgent lifecycle event into a socket already past the 2 MiB kill
    // line: WsHandler.send drops the frame AND closes the socket with 4008.
    fireEvent({ type: 'freshAgent.turn.complete', sessionId: 'ses_1', at: Date.now() })
    expect(ws.close).toHaveBeenCalledWith(4008, 'Backpressure')
    expect(ws.send).not.toHaveBeenCalled()

    // The connection-close path cancels every freshAgent subscription
    // (WsHandler.onClose -> cancelAllFreshAgentSubscriptions) - nothing is
    // queued or retained for the dead socket.
    ;(handler as any).cancelAllFreshAgentSubscriptions(state)
    expect(off).toHaveBeenCalledTimes(1)
    expect(state.freshAgentSubscriptions.size).toBe(0)

    // Later events are dropped outright: no backlog, no replay, no reconnect
    // redelivery for this socket.
    fireEvent({ type: 'freshAgent.turn.complete', sessionId: 'ses_1', at: Date.now() + 1 })
    await flushMicrotasks()
    expect(ws.send).not.toHaveBeenCalled()
    expect(ws.close).toHaveBeenCalledTimes(1)
    expect(sentTypes(ws)).not.toContain('freshAgent.event')
  })
})
