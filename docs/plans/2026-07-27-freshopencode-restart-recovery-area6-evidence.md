# zrrj Area 6 verdict: COUPLED (fixed in this change)

- retention_lost itself is log-only in current main (post-ff8589bb, 2026-06-25) and emits
  zero WS frames — proven by test 'EVIDENCE: retention_lost is log-only and emits zero WS
  frames in the current build' (test/unit/server/ws-handler-fresh-agent-backpressure.test.ts).
  The incident's 10k+/10min retention_lost count is incompatible with the current
  1/s/terminal rate limit, so the incident build predated ff8589bb, when retention loss
  also pushed a terminal.stream.changed frame per attached client (a direct WS write
  amplifier).
- The load-bearing coupling: terminal output and freshAgent lifecycle events share one
  socket with asymmetric backpressure policy (broker ungated vs WsHandler drop+close at
  2 MiB). Proven by test 'delivers freshAgent.turn.complete while flooding terminal output
  (broker self-throttles below the kill line)' — RED before the fix: an ungated ~5 MiB
  live-output flood inflated bufferedAmount to 5,363,909 bytes, past the 2 MiB kill line,
  so the freshAgent.turn.complete frame was dropped and the socket closed 4008.
- Fix shipped: TERMINAL_STREAM_FOREGROUND_PAUSE_BUFFERED_BYTES = 1 MiB broker pause below
  the 2 MiB kill line (server/terminal-stream/constants.ts, flushAttachment gate in
  server/terminal-stream/broker.ts), reusing the background path's 100 ms retry mechanic
  (TERMINAL_BACKGROUND_RETRY_FLUSH_MS). The incident's 'WebSocket send callback reported
  failure' rows match ws-send.ts:167-174 in the same congestion regime.
