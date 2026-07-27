/** Evidence-based interrupted-turn detector for OpenCode transcripts (kata zrrj).
 *
 * The OpenCode DB has NO persisted "interrupted" status field — interruption is
 * detected purely from transcript/tool evidence over the raw sidecar messages
 * (`{ info, parts }` shapes, same as normalize.ts reads). Pure function: no I/O,
 * no timers — the stability window is data-driven via `options.nowMs`.
 *
 * Validated against live opencode 1.18.x data (94k-assistant-message census,
 * 0 false positives for this gate). Two load-bearing semantics from that census:
 * - Aborted turns DO carry `time.completed` (all 203 MessageAbortedError rows),
 *   so `missing_completion` will NOT fire for them — the `aborted_error`-alone
 *   definitive branch is required, not optional.
 * - `info.error.data.name` never occurs on 1.18.x; the second read is a
 *   defensive fallback for future sidecar drift. Keep it, expect it dead.
 */

export type InterruptedTurnVerdict =
  | { interrupted: false; reason: string }
  | {
      interrupted: true
      messageId: string
      /** Which evidence fired — for the audit log and the transcript event. */
      evidence: Array<
        'missing_completion' | 'aborted_error' | 'running_tool_part' | 'zero_output_tokens' | 'missing_step_finish'
      >
    }

export const INTERRUPTED_TURN_STABILITY_MS = 15_000

export function detectInterruptedTurn(
  messages: Array<{ info: Record<string, any>; parts: Array<Record<string, any>> }>,
  options: { nowMs: number; stabilityMs?: number },
): InterruptedTurnVerdict {
  // Rule 1: only a trailing assistant message can be an interrupted turn. A
  // trailing user message means the user already typed a follow-up — never
  // auto-recover then.
  const last = messages.at(-1)
  if (!last) return { interrupted: false, reason: 'empty_transcript' }
  const info = last.info ?? {}
  if (info?.role !== 'assistant') return { interrupted: false, reason: 'last_message_not_assistant' }

  // Rule 2: stability window — a still-writing DB/sidecar row must not be
  // recovered prematurely.
  const stabilityMs = options.stabilityMs ?? INTERRUPTED_TURN_STABILITY_MS
  const created = Number.isFinite(info?.time?.created) ? Number(info.time.created) : 0
  const updated = Number.isFinite(info?.time?.updated) ? Number(info.time.updated) : 0
  if (options.nowMs - Math.max(created, updated) < stabilityMs) {
    return { interrupted: false, reason: 'within_stability_window' }
  }

  // Rule 3: collect evidence on the trailing assistant message.
  const parts = Array.isArray(last.parts) ? last.parts : []
  const missingCompletion = !Number.isFinite(info?.time?.completed)
  const abortedError =
    info?.error?.name === 'MessageAbortedError' || info?.error?.data?.name === 'MessageAbortedError'
  const runningToolPart = parts.some((part) => part?.type === 'tool' && part?.state?.status === 'running')
  let stepStartCount = 0
  let stepFinishCount = 0
  for (const part of parts) {
    if (part?.type === 'step-start') stepStartCount += 1
    if (part?.type === 'step-finish') stepFinishCount += 1
  }
  const missingStepFinish = stepStartCount > 0 && stepFinishCount === 0
  // Zero output tokens alone is too weak — a legitimate tool-only turn can have
  // 0 output tokens — so it only counts alongside at least one other item.
  const zeroOutputTokens =
    info?.tokens?.output === 0 && (missingCompletion || abortedError || runningToolPart || missingStepFinish)

  // Stable order keeps consumers/tests deterministic.
  const evidence = [
    ...(missingCompletion ? (['missing_completion'] as const) : []),
    ...(abortedError ? (['aborted_error'] as const) : []),
    ...(runningToolPart ? (['running_tool_part'] as const) : []),
    ...(zeroOutputTokens ? (['zero_output_tokens'] as const) : []),
    ...(missingStepFinish ? (['missing_step_finish'] as const) : []),
  ]

  // Rule 4: high-confidence gate — missing_completion plus at least one more
  // item, or an explicit abort record alone (definitive).
  const highConfidence = (missingCompletion && evidence.length >= 2) || abortedError
  if (!highConfidence) return { interrupted: false, reason: 'insufficient_evidence' }
  return {
    interrupted: true,
    messageId: String(info?.id ?? ''),
    evidence: [...evidence],
  }
}
