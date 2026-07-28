// Ephemeral crash/auto-resume presentation state (Lane D1). Deliberately a
// separate slice: pane persistence shapes are owned by Lane D4 and the
// persistMiddleware strip is a denylist — a new pane field would persist by
// default. (Two layers, both true: store.ts allowlists which SLICES persist;
// within an allowlisted slice, the strip deny-removes pane FIELDS.) This
// slice is never added to that allowlist, so it is never persisted.
import { createSlice, type PayloadAction } from '@reduxjs/toolkit'

export const AUTO_RESUME_NOTICE_TTL_MS = 30_000

export interface TerminalExitRecord { exitCode: number; at: number }
export interface AutoResumeNotice {
  kind: 'recovering' | 'resumed'
  attempt: number
  maxAttempts: number
  exitCode: number
  at: number
}

export interface PaneLifecycleEntry {
  lastTerminalId?: string
  exit?: TerminalExitRecord
  notice?: AutoResumeNotice
}

interface TerminalLifecycleState {
  byPaneId: Record<string, PaneLifecycleEntry>
}

const initialState: TerminalLifecycleState = { byPaneId: {} }

const entry = (state: TerminalLifecycleState, paneId: string) =>
  (state.byPaneId[paneId] ??= {})

const slice = createSlice({
  name: 'terminalLifecycle',
  initialState,
  reducers: {
    // Dispatched by the pane's own terminal.exit handler BEFORE it clears
    // paneContent.terminalId (TerminalView.tsx:4141-4148) — this is the only
    // moment both paneId and the dying terminalId are simultaneously known.
    recordTerminalExit(state, a: PayloadAction<{ paneId: string; terminalId: string; exitCode: number; at: number }>) {
      const e = entry(state, a.payload.paneId)
      e.lastTerminalId = a.payload.terminalId
      e.exit = { exitCode: a.payload.exitCode, at: a.payload.at }
      // Fresh-eyes fix: an exit is always NEWER truth than any notice. Without
      // this, the exhaustion path (last crash -> settle, which emits no frame)
      // leaves the previous 'resumed' notice masking the role=alert error bar
      // for the 30s TTL — a success-toned banner on a dead pane. Clearing here
      // makes the alert show immediately on the final crash; a genuine
      // in-flight resume re-sets the notice when its `recovering` frame lands
      // (which always follows the exit, per Task 5's emit order).
      delete e.notice
    },
    recordAutoResumeRecovering(state, a: PayloadAction<{ paneId: string; attempt: number; maxAttempts: number; exitCode: number; at: number }>) {
      const { paneId, ...n } = a.payload
      entry(state, paneId).notice = { kind: 'recovering', ...n }
    },
    foldTerminalReplacement(state, a: PayloadAction<{ paneId: string; newTerminalId: string; exitCode: number; attempt: number; maxAttempts: number; at: number }>) {
      const { paneId, newTerminalId, exitCode, attempt, maxAttempts, at } = a.payload
      const e = entry(state, paneId)
      delete e.exit // pane is alive again — no error bar
      e.notice = { kind: 'resumed', attempt, maxAttempts, exitCode, at }
      e.lastTerminalId = newTerminalId
    },
    clearTerminalLifecycle(state, a: PayloadAction<{ paneId: string }>) {
      delete state.byPaneId[a.payload.paneId]
    },
  },
})

export const { recordTerminalExit, recordAutoResumeRecovering, foldTerminalReplacement, clearTerminalLifecycle } = slice.actions
export default slice.reducer

// Selectors tolerate an absent slice state (`s?.`): many pre-existing client
// tests build partial Redux stores without this reducer and render
// TerminalView, which calls these on every render. Mirrors the defensive
// access convention of paneRuntimeActivity consumers
// (`s.paneRuntimeActivity?.byPaneId ?? EMPTY`). Production stores always
// include the reducer (store.ts), so this never changes runtime behavior.
export const selectExitRecordFrom = (s: TerminalLifecycleState | undefined, paneId: string) => s?.byPaneId[paneId]?.exit
export const selectLastTerminalIdFrom = (s: TerminalLifecycleState | undefined, paneId: string) => s?.byPaneId[paneId]?.lastTerminalId
export const selectActiveNoticeFrom = (s: TerminalLifecycleState | undefined, paneId: string, now: number) => {
  const n = s?.byPaneId[paneId]?.notice
  return n && now - n.at <= AUTO_RESUME_NOTICE_TTL_MS ? n : undefined
}
// Root-state wrappers — match the RootState typing convention of the sibling
// selectors in this directory (see turnCompletionSlice.ts for the pattern):
export const selectExitRecord = (root: { terminalLifecycle?: TerminalLifecycleState }, paneId: string) =>
  selectExitRecordFrom(root.terminalLifecycle, paneId)
export const selectActiveNotice = (root: { terminalLifecycle?: TerminalLifecycleState }, paneId: string, now: number) =>
  selectActiveNoticeFrom(root.terminalLifecycle, paneId, now)
