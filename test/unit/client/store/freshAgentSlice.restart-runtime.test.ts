import { describe, expect, it } from 'vitest'
import { makeFreshAgentSessionKey } from '@shared/fresh-agent'
import reducer, {
  addPermissionRequest,
  addQuestionRequest,
  applyAgentRestartReplaced,
  sessionSnapshotReceived,
  setSessionStatus,
  setStreaming,
} from '@/store/freshAgentSlice'

const locator = {
  sessionId: 'fresh-old',
  sessionType: 'freshcodex' as const,
  provider: 'codex' as const,
}
const key = makeFreshAgentSessionKey(locator)
const replacementKey = makeFreshAgentSessionKey({ ...locator, sessionId: 'fresh-new' })
const oldRuntime = { runtimeId: 'fresh-old', generation: 7 }

function activeState() {
  let state = reducer(undefined, sessionSnapshotReceived({
    ...locator,
    latestTurnId: 'turn-1',
    status: 'running',
    revision: 3,
    historySessionId: 's1',
    streamingActive: true,
    streamingText: 'partial',
    runtime: oldRuntime,
  }))
  state = reducer(state, addPermissionRequest({
    ...locator,
    requestId: 'approval-1',
    toolName: 'Bash',
    input: {},
    runtime: oldRuntime,
  } as never))
  state = reducer(state, addQuestionRequest({
    ...locator,
    requestId: 'question-1',
    questions: [],
    runtime: oldRuntime,
  } as never))
  return state
}

describe('freshAgentSlice agent restart replacement', () => {
  it('clears stale snapshot, approval, question, stream, and activity state before accepting the replacement generation', () => {
    const state = activeState()
    const next = reducer(state, applyAgentRestartReplaced({
      type: 'agent.restart.replaced',
      requestId: 'restart-1',
      provider: 'codex',
      sessionId: 's1',
      kind: 'fresh-agent',
      oldRuntimeId: 'fresh-old',
      oldGeneration: 7,
      runtimeId: 'fresh-new',
      generation: 8,
    }))

    expect(next.sessions[key]).toBeUndefined()
    expect(next.sessions[replacementKey]).toMatchObject({
      sessionId: 'fresh-new',
      runtimeId: 'fresh-new',
      runtimeGeneration: 8,
      status: 'starting',
      streamingText: '',
      streamingActive: false,
      pendingPermissions: {},
      pendingQuestions: {},
    })
    expect(next.sessions[replacementKey].snapshot).toBeUndefined()
    expect(next.sessions[replacementKey].latestTurnId).toBeUndefined()
  })

  it('rejects old-runtime transport events after replacement but accepts the replacement generation', () => {
    let state = reducer(activeState(), applyAgentRestartReplaced({
      type: 'agent.restart.replaced',
      requestId: 'restart-1',
      provider: 'codex',
      sessionId: 's1',
      kind: 'fresh-agent',
      oldRuntimeId: 'fresh-old',
      oldGeneration: 7,
      runtimeId: 'fresh-new',
      generation: 8,
    }))

    state = reducer(state, setStreaming({ ...locator, active: true, runtime: oldRuntime }))
    state = reducer(state, setSessionStatus({ ...locator, status: 'running', runtime: oldRuntime }))
    expect(state.sessions[replacementKey].streamingActive).toBe(false)
    expect(state.sessions[replacementKey].status).toBe('starting')

    const replacementRuntime = { runtimeId: 'fresh-new', generation: 8 }
    const replacementLocator = { ...locator, sessionId: 'fresh-new' }
    state = reducer(state, setStreaming({ ...replacementLocator, active: true, runtime: replacementRuntime }))
    state = reducer(state, setSessionStatus({ ...replacementLocator, status: 'running', runtime: replacementRuntime }))
    expect(state.sessions[replacementKey].streamingActive).toBe(true)
    expect(state.sessions[replacementKey].status).toBe('running')
  })
})
