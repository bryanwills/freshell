import { MAX_REALTIME_MESSAGE_BYTES } from '../../shared/read-models.js'

export const TERMINAL_STREAM_BATCH_MAX_BYTES = Math.max(
  1024,
  Number(process.env.TERMINAL_STREAM_BATCH_MAX_BYTES || MAX_REALTIME_MESSAGE_BYTES),
)

export const TERMINAL_WS_CATASTROPHIC_BUFFERED_BYTES = Math.max(
  1024,
  Number(process.env.TERMINAL_WS_CATASTROPHIC_BUFFERED_BYTES || 16 * 1024 * 1024),
)

export const TERMINAL_WS_CATASTROPHIC_STALL_MS = Math.max(
  1,
  Number(process.env.TERMINAL_WS_CATASTROPHIC_STALL_MS || 10_000),
)

export const TERMINAL_STREAM_RETRY_FLUSH_MS = Math.max(
  1,
  Number(process.env.TERMINAL_STREAM_RETRY_FLUSH_MS || 50),
)

export const TERMINAL_BACKGROUND_BUFFERED_PAUSE_BYTES = Math.max(
  1024,
  Number(process.env.TERMINAL_BACKGROUND_BUFFERED_PAUSE_BYTES || 512 * 1024),
)

export const TERMINAL_BACKGROUND_RETRY_FLUSH_MS = Math.max(
  1,
  Number(process.env.TERMINAL_BACKGROUND_RETRY_FLUSH_MS || 100),
)

/**
 * Foreground attachments pause flushing when the socket has this much
 * unflushed data. MUST stay below WsHandler's maxWsBufferedAmount (2 MiB)
 * kill line: terminal output must self-throttle before lifecycle messages
 * (freshAgent.*, session updates) start being dropped with a 4008 close. (zrrj)
 */
export const TERMINAL_STREAM_FOREGROUND_PAUSE_BUFFERED_BYTES = 1 * 1024 * 1024
