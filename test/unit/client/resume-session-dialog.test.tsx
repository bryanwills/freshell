import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, fireEvent, render, screen, act } from '@testing-library/react'
import ResumeSessionDialog from '@/components/ResumeSessionDialog'
import { ApiError, type ResumeResolveResponse } from '@/lib/api'

const mockResolve = vi.fn<(input: string) => Promise<ResumeResolveResponse>>()
vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>()
  return { ...actual, resolveResumeInput: (input: string) => mockResolve(input) }
})

const CLAUDE_V4 = 'ed2afda6-a340-443e-ba60-024a1b3554b4'

function response(overrides: Partial<ResumeResolveResponse>): ResumeResolveResponse {
  return {
    indexState: 'ready',
    tokens: [CLAUDE_V4],
    agentHint: null,
    homeDir: '/home/testuser',
    providerErrors: [],
    unsearchedProviders: [],
    matches: [],
    ...overrides,
  }
}

function match(overrides: Record<string, unknown> = {}) {
  return {
    provider: 'claude' as const,
    sessionId: CLAUDE_V4,
    cwd: '/home/u/proj',
    projectPath: '/home/u/proj',
    sessionType: 'claude',
    title: 'claude one',
    lastActivityAt: 111,
    matchType: 'exact' as const,
    matchedToken: CLAUDE_V4,
    ...overrides,
  }
}

function renderDialog(onResume = vi.fn(), onClose = vi.fn()) {
  render(<ResumeSessionDialog open onClose={onClose} onResume={onResume} />)
  return { onResume, onClose }
}

async function pasteAndResolve(text: string) {
  const input = screen.getByLabelText(/resume string/i)
  fireEvent.change(input, { target: { value: text } })
  fireEvent.keyDown(input, { key: 'Enter' })
  // Two flushes: runResolve awaits the API promise, then updates state.
  await act(async () => { await Promise.resolve(); await Promise.resolve() })
}

beforeEach(() => {
  vi.useFakeTimers()
  mockResolve.mockReset()
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('ResumeSessionDialog', () => {
  it('is an accessible modal dialog with picker and paste field', () => {
    renderDialog()
    const dialog = screen.getByRole('dialog', { name: /resume a session/i })
    expect(dialog).toHaveAttribute('aria-modal', 'true')
    expect(screen.getByLabelText(/agent/i)).toBeInTheDocument()
    expect(screen.getByLabelText(/resume string/i)).toBeInTheDocument()
  })

  it('Escape closes the dialog via the DOCUMENT-level listener', () => {
    const { onClose } = renderDialog()
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(onClose).toHaveBeenCalled()
  })

  it('traps Tab focus: wraps from the last focusable to the first and back (Shift+Tab)', () => {
    renderDialog()
    const dialog = screen.getByRole('dialog')
    const picker = screen.getByLabelText(/agent/i)
    // With an empty input, "Find session" is disabled, so Cancel is the last focusable.
    const cancel = screen.getByRole('button', { name: /cancel/i })
    cancel.focus()
    fireEvent.keyDown(dialog, { key: 'Tab' })
    expect(document.activeElement).toBe(picker)
    fireEvent.keyDown(dialog, { key: 'Tab', shiftKey: true })
    expect(document.activeElement).toBe(cancel)
  })

  it('locks background scroll while open; restores scroll and focus on close', () => {
    const outside = document.createElement('button')
    document.body.appendChild(outside)
    outside.focus()
    const onClose = vi.fn()
    const onResume = vi.fn()
    const { rerender } = render(<ResumeSessionDialog open onClose={onClose} onResume={onResume} />)
    expect(document.body.style.overflow).toBe('hidden')
    rerender(<ResumeSessionDialog open={false} onClose={onClose} onResume={onResume} />)
    expect(document.body.style.overflow).toBe('')
    expect(document.activeElement).toBe(outside)
    outside.remove()
  })

  it('single match: resumes with the STORE provider even when the picker disagrees, and shows a note', async () => {
    mockResolve.mockResolvedValue(response({ matches: [match()] }))
    const { onResume } = renderDialog()
    fireEvent.change(screen.getByLabelText(/agent/i), { target: { value: 'codex' } })
    await pasteAndResolve(CLAUDE_V4)
    expect(onResume).toHaveBeenCalledWith(expect.objectContaining({
      provider: 'claude', sessionId: CLAUDE_V4, sessionType: 'claude', cwd: '/home/u/proj',
    }))
    expect(screen.getByRole('status')).toHaveTextContent(/found in claude/i)
  })

  it('auto-resolves on paste (paste-then-Enter fast path)', async () => {
    mockResolve.mockResolvedValue(response({ matches: [match()] }))
    const { onResume } = renderDialog()
    const input = screen.getByLabelText(/resume string/i)
    fireEvent.paste(input, { clipboardData: { getData: () => CLAUDE_V4 } })
    await act(async () => { vi.advanceTimersByTime(1); await Promise.resolve(); await Promise.resolve() })
    expect(mockResolve).toHaveBeenCalled()
    expect(onResume).toHaveBeenCalled()
  })

  it('multiple matches: shows a disambiguation list, one click resumes', async () => {
    mockResolve.mockResolvedValue(response({
      matches: [
        match({ sessionId: 'aaa11111-1111-4111-8111-111111111111', title: 'newer', lastActivityAt: 2 }),
        match({ provider: 'codex', sessionId: 'bbb22222-2222-4222-8222-222222222222', sessionType: 'codex', title: 'older', lastActivityAt: 1 }),
      ],
    }))
    const { onResume } = renderDialog()
    await pasteAndResolve('aaa')
    const options = screen.getAllByRole('button', { name: /resume .*(newer|older)/i })
    expect(options).toHaveLength(2)
    fireEvent.click(options[1])
    expect(onResume).toHaveBeenCalledWith(expect.objectContaining({ provider: 'codex' }))
  })

  it('zero matches + ready: inline error, input preserved, resume-anyway with editable cwd prefilled to home', async () => {
    mockResolve.mockResolvedValue(response({ matches: [] }))
    const { onResume } = renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    expect(screen.getByRole('alert')).toHaveTextContent(/no matching session/i)
    expect(screen.getByLabelText(/resume string/i)).toHaveValue(CLAUDE_V4)
    const cwdInput = screen.getByLabelText(/working directory/i)
    expect(cwdInput).toHaveValue('/home/testuser')
    fireEvent.change(cwdInput, { target: { value: '/home/testuser/elsewhere' } })
    fireEvent.click(screen.getByRole('button', { name: /resume anyway/i }))
    expect(onResume).toHaveBeenCalledWith(expect.objectContaining({
      sessionId: CLAUDE_V4, cwd: '/home/testuser/elsewhere',
    }))
  })

  it('resume-anyway uses the picker-selected agent', async () => {
    mockResolve.mockResolvedValue(response({ matches: [] }))
    const { onResume } = renderDialog()
    fireEvent.change(screen.getByLabelText(/agent/i), { target: { value: 'amplifier' } })
    await pasteAndResolve('417e8345aa')
    fireEvent.click(screen.getByRole('button', { name: /resume anyway/i }))
    expect(onResume).toHaveBeenCalledWith(expect.objectContaining({ provider: 'amplifier', sessionType: 'amplifier' }))
  })

  it('single match WITHOUT a cwd does NOT auto-resume: asks for a working directory instead', async () => {
    // Exact-id fallback hits can lack a recorded cwd; the spec requires a
    // concrete working directory before opening — never auto-open without one.
    mockResolve.mockResolvedValue(response({ matches: [match({ cwd: undefined, projectPath: '' })] }))
    const { onResume } = renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    expect(onResume).not.toHaveBeenCalled()
    const cwdInput = screen.getByLabelText(/working directory/i)
    expect(cwdInput).toHaveValue('/home/testuser')
    fireEvent.change(cwdInput, { target: { value: '/home/u/somewhere' } })
    fireEvent.click(screen.getByRole('button', { name: /resume claude code session/i }))
    expect(onResume).toHaveBeenCalledWith(expect.objectContaining({
      provider: 'claude', sessionId: CLAUDE_V4, cwd: '/home/u/somewhere',
    }))
  })

  it('EDITING the input invalidates the previous result: stale resume-anyway/matches cannot act', async () => {
    // Without this, resolve(A) -> "not found" -> replace text with B ->
    // "Resume anyway" would resume STALE id A via result.tokens[0].
    mockResolve.mockResolvedValue(response({ matches: [] }))
    renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    expect(screen.getByRole('button', { name: /resume anyway/i })).toBeInTheDocument()
    fireEvent.change(screen.getByLabelText(/resume string/i), { target: { value: 'ses_other0000000000000000000000' } })
    expect(screen.queryByRole('button', { name: /resume anyway/i })).toBeNull()
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('names DISABLED (unsearched) providers in the no-match message', async () => {
    mockResolve.mockResolvedValue(response({ matches: [], unsearchedProviders: ['codex', 'amplifier'] }))
    renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    expect(screen.getByRole('alert')).toHaveTextContent(/not searched \(disabled\): codex, amplifier/i)
    // Resume-anyway stays available.
    expect(screen.getByRole('button', { name: /resume anyway/i })).toBeInTheDocument()
  })

  it('warming: shows retry state, NOT "not found", and re-resolves on retry', async () => {
    mockResolve.mockResolvedValueOnce(response({ indexState: 'warming', matches: [] }))
    mockResolve.mockResolvedValueOnce(response({ matches: [match()] }))
    const { onResume } = renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.getByRole('status')).toHaveTextContent(/index is still warming/i)
    fireEvent.click(screen.getByRole('button', { name: /retry/i }))
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    expect(onResume).toHaveBeenCalled()
  })

  it('degraded (provider unavailable): retry state, NOT "no matching session"', async () => {
    mockResolve.mockResolvedValueOnce(response({ indexState: 'degraded', providerErrors: ['opencode'], matches: [] }))
    mockResolve.mockResolvedValueOnce(response({ matches: [match()] }))
    const { onResume } = renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.getByRole('status')).toHaveTextContent(/could not be searched/i)
    fireEvent.click(screen.getByRole('button', { name: /retry/i }))
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    expect(onResume).toHaveBeenCalled()
  })

  it('ignores STALE responses: a late first response cannot override or auto-resume', async () => {
    let resolveFirst!: (r: ResumeResolveResponse) => void
    mockResolve.mockReturnValueOnce(new Promise<ResumeResolveResponse>((r) => { resolveFirst = r }))
    mockResolve.mockResolvedValueOnce(response({ matches: [] }))
    const { onResume } = renderDialog()
    const input = screen.getByLabelText(/resume string/i)
    fireEvent.change(input, { target: { value: 'ed2afda6' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    fireEvent.change(input, { target: { value: CLAUDE_V4 } })
    fireEvent.keyDown(input, { key: 'Enter' })
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    expect(screen.getByRole('alert')).toHaveTextContent(/no matching session/i)
    // The stale FIRST response now arrives with a single match: it must be
    // ignored — a stale auto-resume would open the WRONG session.
    await act(async () => {
      resolveFirst(response({ matches: [match()] }))
      await Promise.resolve(); await Promise.resolve()
    })
    expect(onResume).not.toHaveBeenCalled()
    expect(screen.getByRole('alert')).toHaveTextContent(/no matching session/i)
  })

  it('resolve 404 (endpoint absent, e.g. Rust-served client): explicit unsupported message', async () => {
    mockResolve.mockRejectedValue(new ApiError(404, 'Not Found'))
    const { onResume } = renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    expect(screen.getByRole('alert')).toHaveTextContent(/does not support resume-by-id/i)
    expect(onResume).not.toHaveBeenCalled()
  })

  it('garbage input: inline error, no resume', async () => {
    mockResolve.mockResolvedValue(response({ tokens: [], matches: [] }))
    const { onResume } = renderDialog()
    await pasteAndResolve('total garbage')
    expect(screen.getByRole('alert')).toHaveTextContent(/no session id/i)
    expect(onResume).not.toHaveBeenCalled()
  })

  it('pre-fills the picker from the advisory hint when untouched', async () => {
    mockResolve.mockResolvedValue(response({
      agentHint: { provider: 'opencode', source: 'id-format' }, matches: [],
      tokens: ['ses_root0000000000000000000000'],
    }))
    renderDialog()
    await pasteAndResolve('ses_root0000000000000000000000')
    expect(screen.getByLabelText(/agent/i)).toHaveValue('opencode')
  })

  it('DEGRADED single match with cwd: NO auto-resume — match listed with degraded notice + Retry', async () => {
    // A failed provider means a higher-priority exact match may have been
    // missed: auto-opening the surviving match could open the WRONG session.
    mockResolve.mockResolvedValue(response({
      indexState: 'degraded', providerErrors: ['claude'], matches: [match()],
    }))
    const { onResume } = renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    expect(onResume).not.toHaveBeenCalled()
    expect(screen.getByRole('status')).toHaveTextContent(/could not be searched/i)
    expect(screen.getByRole('button', { name: /retry/i })).toBeInTheDocument()
    expect(screen.getByRole('list', { name: /matching sessions/i })).toBeInTheDocument()
  })

  it('cwd-less match with a BLANK working-directory field: confirm is blocked with an inline error, no resume', async () => {
    mockResolve.mockResolvedValue(response({ matches: [{ ...match(), cwd: undefined }] }))
    const { onResume } = renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    fireEvent.change(screen.getByLabelText(/working directory/i), { target: { value: '   ' } })
    fireEvent.click(screen.getByRole('button', { name: /resume claude code session/i }))
    expect(onResume).not.toHaveBeenCalled()
    expect(screen.getByRole('alert')).toHaveTextContent(/working directory/i)
  })

  it('"Resume anyway" is DISABLED while the working-directory field is blank (never launch a cwd-less tuple)', async () => {
    mockResolve.mockResolvedValue(response({ matches: [] }))
    const { onResume } = renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    fireEvent.change(screen.getByLabelText(/working directory/i), { target: { value: '  ' } })
    const anyway = screen.getByRole('button', { name: /resume anyway/i })
    expect(anyway).toBeDisabled()
    fireEvent.click(anyway)
    expect(onResume).not.toHaveBeenCalled()
  })
})
