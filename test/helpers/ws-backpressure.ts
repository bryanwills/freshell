import { EventEmitter } from 'events'
import WebSocket from 'ws'
import { vi } from 'vitest'

/** Mock WebSocket that extends EventEmitter (like real ws WebSockets). */
export type MockWs = EventEmitter & {
  bufferedAmount: number
  readyState: number
  send: ReturnType<typeof vi.fn>
  close: ReturnType<typeof vi.fn>
  connectionId?: string
  sessionUpdateGeneration?: number
}

/** Create a mock WebSocket with settable bufferedAmount and spied send/close. */
export function createMockWs(overrides: Record<string, unknown> = {}): MockWs {
  const ws = new EventEmitter() as MockWs
  ws.bufferedAmount = 0
  ws.readyState = WebSocket.OPEN
  ws.send = vi.fn()
  ws.close = vi.fn()
  Object.assign(ws, overrides)
  return ws
}

type MockedLogFn = { mock: { calls: unknown[][] } }

export type MockedStructuredLogger = Record<'debug' | 'info' | 'warn' | 'error', MockedLogFn>

/** Filter a mocked logger's structured payloads down to a single event name. */
export function structuredLogsFrom(
  logger: MockedStructuredLogger,
  level: 'debug' | 'info' | 'warn' | 'error',
  event: string,
): Record<string, unknown>[] {
  return logger[level].mock.calls
    .map(([payload]) => payload)
    .filter((payload): payload is Record<string, unknown> => (
      !!payload
      && typeof payload === 'object'
      && (payload as { event?: unknown }).event === event
    ))
}

/**
 * Minimal registry double for TerminalStreamBroker tests: emits
 * 'terminal.output.raw' and satisfies the attach/get/replay-budget surface
 * the broker consumes.
 */
export class FakeBrokerRegistry extends EventEmitter {
  private records = new Map<string, { terminalId: string; mode: string; buffer: { snapshot: () => string } }>()
  private replayRingMaxChars: number | undefined

  createTerminal(terminalId: string, mode = 'shell') {
    this.records.set(terminalId, {
      terminalId,
      mode,
      buffer: { snapshot: () => '' },
    })
  }

  attach(terminalId: string) {
    return this.records.get(terminalId) ?? null
  }

  resize(_terminalId: string, _cols: number, _rows: number) {
    return true
  }

  detach(_terminalId: string) {
    return true
  }

  setReplayRingMaxBytes(next: number | undefined) {
    this.replayRingMaxChars = next
  }

  getReplayRingMaxChars() {
    return this.replayRingMaxChars
  }

  get(terminalId: string) {
    return this.records.get(terminalId)
  }
}
