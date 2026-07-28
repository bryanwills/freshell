import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { TerminalExitBanner } from '@/components/TerminalExitBanner'

describe('TerminalExitBanner', () => {
  // This repo's vitest setup does not auto-cleanup between tests (globals off);
  // sibling suites (DeadSessionPanel.test.tsx) call cleanup() explicitly.
  afterEach(() => cleanup())

  it('renders a loud error bar with the exit code and an accessible relaunch button', () => {
    const onRelaunch = vi.fn()
    render(<TerminalExitBanner mode="claude" exitCode={1} notice={null} onRelaunch={onRelaunch} />)
    const bar = screen.getByRole('alert')
    expect(bar).toHaveTextContent('process exited (code 1)')
    const btn = screen.getByRole('button', { name: 'Relaunch claude session' })
    fireEvent.click(btn)
    expect(onRelaunch).toHaveBeenCalledTimes(1)
  })

  it('renders without a code when the exit code is unknown (post-reload)', () => {
    render(<TerminalExitBanner mode="codex" exitCode={null} notice={null} onRelaunch={() => {}} />)
    expect(screen.getByRole('alert')).toHaveTextContent('process exited')
    expect(screen.getByRole('alert')).not.toHaveTextContent('(code')
  })

  it('renders a recovering notice instead of the error bar while auto-resume is in flight', () => {
    render(<TerminalExitBanner mode="claude" exitCode={1}
      notice={{ kind: 'recovering', attempt: 1, maxAttempts: 2, exitCode: 1, at: Date.now() }}
      onRelaunch={() => {}} />)
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.getByRole('status')).toHaveTextContent('claude crashed (exit 1) — auto-resuming, attempt 1/2')
  })

  it('renders a resumed notice', () => {
    render(<TerminalExitBanner mode="claude" exitCode={null}
      notice={{ kind: 'resumed', attempt: 2, maxAttempts: 2, exitCode: 1, at: Date.now() }}
      onRelaunch={() => {}} />)
    expect(screen.getByRole('status')).toHaveTextContent('claude crashed (exit 1) — auto-resumed, attempt 2/2')
  })
})
