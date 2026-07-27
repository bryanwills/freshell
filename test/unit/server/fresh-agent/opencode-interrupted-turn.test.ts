import { describe, it, expect } from 'vitest'
import {
  detectInterruptedTurn,
  INTERRUPTED_TURN_STABILITY_MS,
} from '../../../../server/fresh-agent/adapters/opencode/interrupted-turn.js'

const NOW = 1_000_000_000
const OLD = NOW - INTERRUPTED_TURN_STABILITY_MS - 1

function assistant(info: Record<string, any>, parts: Record<string, any>[] = []) {
  return { info: { id: 'm2', role: 'assistant', time: { created: OLD, updated: OLD }, ...info }, parts }
}
// realistic: user messages never carry time.completed (verified, V2)
const user = { info: { id: 'm1', role: 'user', time: { created: OLD - 10 } }, parts: [] }

describe('detectInterruptedTurn', () => {
  it('flags missing completion + running tool part as interrupted', () => {
    const verdict = detectInterruptedTurn(
      [user, assistant({}, [{ type: 'tool', state: { status: 'running' } }])],
      { nowMs: NOW },
    )
    expect(verdict).toEqual({ interrupted: true, messageId: 'm2', evidence: ['missing_completion', 'running_tool_part'] })
  })

  it('flags an explicit MessageAbortedError alone', () => {
    // realistic: aborted turns DO carry time.completed (all 203 live MessageAbortedError rows)
    const verdict = detectInterruptedTurn(
      [user, assistant({ time: { created: OLD, completed: OLD + 1 }, error: { name: 'MessageAbortedError' } })],
      { nowMs: NOW },
    )
    expect(verdict.interrupted).toBe(true)
    if (verdict.interrupted) expect(verdict.evidence).toContain('aborted_error')
  })

  it('does not flag missing completion alone (insufficient evidence)', () => {
    const verdict = detectInterruptedTurn([user, assistant({})], { nowMs: NOW })
    expect(verdict).toEqual({ interrupted: false, reason: 'insufficient_evidence' })
  })

  it('respects the stability window for a still-writing row', () => {
    const fresh = {
      info: { id: 'm2', role: 'assistant', time: { created: NOW - 1_000 } },
      parts: [{ type: 'tool', state: { status: 'running' } }],
    }
    expect(detectInterruptedTurn([user, fresh], { nowMs: NOW })).toEqual({
      interrupted: false,
      reason: 'within_stability_window',
    })
  })

  it('never flags when the user already typed a follow-up', () => {
    const trailingUser = { info: { id: 'm3', role: 'user', time: { created: OLD + 5 } }, parts: [] }
    const verdict = detectInterruptedTurn(
      [user, assistant({}, [{ type: 'tool', state: { status: 'running' } }]), trailingUser],
      { nowMs: NOW },
    )
    expect(verdict).toEqual({ interrupted: false, reason: 'last_message_not_assistant' })
  })

  it('counts missing step-finish and zero output tokens as corroborating evidence', () => {
    const verdict = detectInterruptedTurn(
      [user, assistant({ tokens: { input: 50, output: 0 } }, [{ type: 'step-start' }])],
      { nowMs: NOW },
    )
    expect(verdict.interrupted).toBe(true)
    if (verdict.interrupted) {
      expect(verdict.evidence).toEqual(
        expect.arrayContaining(['missing_completion', 'zero_output_tokens', 'missing_step_finish']),
      )
    }
  })

  it('does not flag a cleanly completed turn', () => {
    const done = assistant(
      { time: { created: OLD, completed: OLD + 2 }, tokens: { input: 10, output: 40 } },
      [{ type: 'step-start' }, { type: 'step-finish' }],
    )
    expect(detectInterruptedTurn([user, done], { nowMs: NOW }).interrupted).toBe(false)
  })

  it('returns empty_transcript for no messages', () => {
    expect(detectInterruptedTurn([], { nowMs: NOW })).toEqual({ interrupted: false, reason: 'empty_transcript' })
  })

  it('detects aborted_error via the defensive info.error.data.name fallback', () => {
    const verdict = detectInterruptedTurn(
      [user, assistant({ time: { created: OLD, completed: OLD + 1 }, error: { data: { name: 'MessageAbortedError' } } })],
      { nowMs: NOW },
    )
    expect(verdict.interrupted).toBe(true)
    if (verdict.interrupted) expect(verdict.evidence).toContain('aborted_error')
  })

  it('does not count zero output tokens without any other evidence beside it', () => {
    // completed turn, tool-only, 0 output tokens — legitimate, must not flag
    const toolOnly = assistant(
      { time: { created: OLD, completed: OLD + 2 }, tokens: { input: 30, output: 0 } },
      [{ type: 'step-start' }, { type: 'step-finish' }, { type: 'tool', state: { status: 'completed' } }],
    )
    expect(detectInterruptedTurn([user, toolOnly], { nowMs: NOW })).toEqual({
      interrupted: false,
      reason: 'insufficient_evidence',
    })
  })

  it('uses info.time.updated for the stability window when newer than created', () => {
    const stillWriting = {
      info: { id: 'm2', role: 'assistant', time: { created: OLD - 60_000, updated: NOW - 1_000 } },
      parts: [{ type: 'tool', state: { status: 'running' } }],
    }
    expect(detectInterruptedTurn([user, stillWriting], { nowMs: NOW })).toEqual({
      interrupted: false,
      reason: 'within_stability_window',
    })
  })

  it('honors a custom stabilityMs override', () => {
    const recent = {
      info: { id: 'm2', role: 'assistant', time: { created: NOW - 1_000 } },
      parts: [{ type: 'tool', state: { status: 'running' } }],
    }
    const verdict = detectInterruptedTurn([user, recent], { nowMs: NOW, stabilityMs: 500 })
    expect(verdict).toEqual({ interrupted: true, messageId: 'm2', evidence: ['missing_completion', 'running_tool_part'] })
  })
})
