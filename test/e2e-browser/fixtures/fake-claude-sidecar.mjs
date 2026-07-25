#!/usr/bin/env node
// Fake Claude SDK-bridge sidecar for e2e (freshclaude). Enabled via the
// production env seam: FRESHELL_CLAUDE_SIDECAR=<this file>. Speaks the
// newline-JSON stdio protocol from crates/freshell-freshagent/src/claude.rs:
//   in : {"type":"create",requestId,cwd,model,permissionMode,effort,resumeSessionId}
//        {"type":"send",sessionId,text} {"type":"interrupt",sessionId} {"type":"shutdown"}
//   out: {"type":"created","sessionId"} FIRST (any earlier sdk.* line is
//        DISCARDED by read_created, claude.rs:551; 45s budget claude.rs:71),
//        then sdk.* event lines (renamed sdk.X -> freshAgent.X server-side).
// FIELD shapes come from the REAL sidecar (crates/freshell-claude-sidecar/
// index.mjs:15-30) + the client consumer (src/lib/fresh-agent-ws.ts:195-284):
//   - sdk.assistant content MUST be an ARRAY of blocks (fresh-agent-ws.ts:260-265);
//   - sdk.turn.complete MUST carry a numeric `at` (fresh-agent-ws.ts:233-240);
//   - sdk.session.init cliSessionId MUST be a canonical UUID
//     (shared/session-contract.ts:34) or no durable sessionRef ever lands.
// The process MUST stay alive (no EOF) until shutdown/kill -- an early exit
// stops the server's consumer.
// FAKE_CLAUDE_SIDECAR_HOLD_TURN=1 -> a send starts running and never
// completes (busy-restart wedge scenario).
import readline from 'node:readline'

const HOLD_TURN = process.env.FAKE_CLAUDE_SIDECAR_HOLD_TURN === '1'
const CLI_SESSION_ID =
  process.env.FAKE_CLAUDE_SIDECAR_CLI_SESSION_ID ?? '44444444-4444-4444-8444-444444444444'

function emit(obj) {
  process.stdout.write(`${JSON.stringify(obj)}\n`)
}

const rl = readline.createInterface({ input: process.stdin })
rl.on('line', (line) => {
  let msg
  try {
    msg = JSON.parse(line)
  } catch {
    return
  }
  if (msg.type === 'create') {
    const sessionId = msg.resumeSessionId ?? `fc-e2e-${process.pid}-${Date.now()}`
    // `created` FIRST -- pre-created sdk.* lines are discarded (claude.rs:551).
    emit({ type: 'created', requestId: msg.requestId, sessionId })
    // cliSessionId MUST match the canonical Claude UUID regex
    // (shared/session-contract.ts:34) or the client never derives a durable
    // sessionRef/resumeSessionId for the pane.
    emit({
      type: 'sdk.session.init',
      sessionId,
      cliSessionId: CLI_SESSION_ID,
      model: msg.model ?? 'claude-opus-4-6',
      cwd: msg.cwd ?? process.cwd(),
      tools: [],
    })
    if (msg.resumeSessionId) {
      // Resume creates set expectsHistoryHydration (fresh-agent-ws.ts:86-107,
      // freshAgentSlice.ts:230-231) -- emit a snapshot so the restored pane
      // (Tasks 7/9/10 restore halves) leaves isRestoring.
      emit({ type: 'sdk.session.snapshot', sessionId, messages: [] })
    }
    // Required for pane status 'idle' (created alone only yields 'connected').
    emit({ type: 'sdk.status', sessionId, status: 'idle' })
  } else if (msg.type === 'send') {
    emit({ type: 'sdk.status', sessionId: msg.sessionId, status: 'running' })
    if (!HOLD_TURN) {
      // content MUST be an ARRAY of blocks: the client renders assistant
      // messages only from event.content arrays (fresh-agent-ws.ts:260-265);
      // a bare `text` field is silently dropped.
      emit({
        type: 'sdk.assistant',
        sessionId: msg.sessionId,
        content: [{ type: 'text', text: 'Fixture claude turn' }],
        model: 'claude-opus-4-6',
      })
      // turn.complete without a NUMERIC `at` is dropped by the client
      // (fresh-agent-ws.ts:233-240).
      emit({
        type: 'sdk.turn.complete',
        sessionId: msg.sessionId,
        subtype: 'success',
        at: Date.now(),
      })
      emit({ type: 'sdk.status', sessionId: msg.sessionId, status: 'idle' })
    }
  } else if (msg.type === 'shutdown') {
    process.exit(0)
  }
})
