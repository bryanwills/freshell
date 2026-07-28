// Lane D1: loud exited-pane presentation for coding-agent terminals.
// - recovering/resumed notice (server-driven auto-resume in flight/succeeded)
// - error bar + Relaunch after the pane settles exited (non-zero exit).
// Pure presentational: props in, callbacks out — TerminalView owns the render
// conditions and the relaunch dispatch.
import type { AutoResumeNotice } from '../store/terminalLifecycleSlice'

export interface TerminalExitBannerProps {
  mode: string
  exitCode: number | null
  notice: AutoResumeNotice | null
  onRelaunch: () => void
}

export function TerminalExitBanner({ mode, exitCode, notice, onRelaunch }: TerminalExitBannerProps) {
  if (notice) {
    const verb = notice.kind === 'recovering' ? 'auto-resuming' : 'auto-resumed'
    return (
      <div
        role="status"
        className="flex items-center gap-2 border-t border-amber-500/30 bg-amber-500/15 px-3 py-1.5 text-sm text-amber-600 dark:text-amber-400"
      >
        <span>
          {mode} crashed (exit {notice.exitCode}) — {verb}, attempt {notice.attempt}/{notice.maxAttempts}
        </span>
      </div>
    )
  }
  return (
    <div
      role="alert"
      className="flex items-center justify-between gap-2 border-t border-destructive/30 bg-destructive/15 px-3 py-1.5 text-sm text-destructive"
    >
      <span>process exited{exitCode !== null ? ` (code ${exitCode})` : ''}</span>
      <button
        type="button"
        aria-label={`Relaunch ${mode} session`}
        className="shrink-0 rounded border border-destructive/40 px-2 py-0.5 text-xs font-medium hover:bg-destructive/20"
        onClick={onRelaunch}
      >
        Relaunch
      </button>
    </div>
  )
}
