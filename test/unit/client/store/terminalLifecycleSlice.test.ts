import { describe, it, expect } from 'vitest'
import reducer, {
  recordTerminalExit, recordAutoResumeRecovering, foldTerminalReplacement,
  clearTerminalLifecycle, selectExitRecordFrom, selectActiveNoticeFrom,
  selectLastTerminalIdFrom, selectExitRecord, selectActiveNotice,
  AUTO_RESUME_NOTICE_TTL_MS,
} from '@/store/terminalLifecycleSlice'

const empty = reducer(undefined, { type: '@@init' })

describe('terminalLifecycleSlice', () => {
  it('records an exit code + lastTerminalId per paneId', () => {
    const s = reducer(empty, recordTerminalExit({ paneId: 'p1', terminalId: 't1', exitCode: 1, at: 1000 }))
    expect(selectExitRecordFrom(s, 'p1')).toEqual({ exitCode: 1, at: 1000 })
    expect(selectLastTerminalIdFrom(s, 'p1')).toBe('t1') // frame-matching key survives TerminalView clearing its own terminalId
  })

  it('records a recovering notice and expires it after the TTL', () => {
    const s = reducer(empty, recordAutoResumeRecovering({ paneId: 'p1', attempt: 1, maxAttempts: 2, exitCode: 1, at: 1000 }))
    expect(selectActiveNoticeFrom(s, 'p1', 1000 + AUTO_RESUME_NOTICE_TTL_MS - 1)?.kind).toBe('recovering')
    expect(selectActiveNoticeFrom(s, 'p1', 1000 + AUTO_RESUME_NOTICE_TTL_MS + 1)).toBeUndefined()
  })

  it('fold clears the exit record, sets a resumed notice, and advances lastTerminalId', () => {
    let s = reducer(empty, recordTerminalExit({ paneId: 'p1', terminalId: 't1', exitCode: 1, at: 1000 }))
    s = reducer(s, recordAutoResumeRecovering({ paneId: 'p1', attempt: 1, maxAttempts: 2, exitCode: 1, at: 1000 }))
    s = reducer(s, foldTerminalReplacement({ paneId: 'p1', newTerminalId: 't2', exitCode: 1, attempt: 1, maxAttempts: 2, at: 2000 }))
    expect(selectExitRecordFrom(s, 'p1')).toBeUndefined() // pane is alive again — no error bar
    expect(selectActiveNoticeFrom(s, 'p1', 2000)).toEqual({ kind: 'resumed', attempt: 1, maxAttempts: 2, exitCode: 1, at: 2000 })
    expect(selectLastTerminalIdFrom(s, 'p1')).toBe('t2')
  })

  it('a later exit clears any active notice (exhaustion must not be masked by a stale resumed strip)', () => {
    // fold sets a 'resumed' notice; the replacement then crashes and the hub
    // settles retries_exhausted WITHOUT emitting any frame — the exit record
    // must surface the alert immediately, not after the 30s TTL.
    let s = reducer(empty, foldTerminalReplacement({ paneId: 'p1', newTerminalId: 't2', exitCode: 1, attempt: 2, maxAttempts: 2, at: 1000 }))
    s = reducer(s, recordTerminalExit({ paneId: 'p1', terminalId: 't2', exitCode: 1, at: 2000 }))
    expect(selectActiveNoticeFrom(s, 'p1', 2000)).toBeUndefined()
    expect(selectExitRecordFrom(s, 'p1')).toEqual({ exitCode: 1, at: 2000 })
  })

  it('selectors tolerate a root store without the slice (partial test stores must not crash)', () => {
    // Regression pin: 44 pre-existing client test files build partial Redux
    // stores (no terminalLifecycle reducer) and render TerminalView, which
    // calls these selectors on every render. They must degrade to undefined,
    // mirroring the paneRuntimeActivity defensive-access convention.
    const bare = {} as Parameters<typeof selectExitRecord>[0]
    expect(selectExitRecord(bare, 'p1')).toBeUndefined()
    expect(selectActiveNotice(bare, 'p1', Date.now())).toBeUndefined()
    expect(selectExitRecordFrom(undefined, 'p1')).toBeUndefined()
    expect(selectLastTerminalIdFrom(undefined, 'p1')).toBeUndefined()
    expect(selectActiveNoticeFrom(undefined, 'p1', 0)).toBeUndefined()
  })

  it('clearTerminalLifecycle wipes the pane entry', () => {
    let s = reducer(empty, recordTerminalExit({ paneId: 'p1', terminalId: 't1', exitCode: 7, at: 1 }))
    s = reducer(s, clearTerminalLifecycle({ paneId: 'p1' }))
    expect(selectExitRecordFrom(s, 'p1')).toBeUndefined()
    expect(selectLastTerminalIdFrom(s, 'p1')).toBeUndefined()
  })
})
