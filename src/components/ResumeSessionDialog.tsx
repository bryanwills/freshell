import { useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { Loader2 } from 'lucide-react'
import { OVERLAY_Z } from '@/components/ui/overlay'
import { ApiError, resolveResumeInput, type ResumeResolveMatch, type ResumeResolveResponse } from '@/lib/api'
import { parseResumeInput, type ResumeProviderName } from '@shared/resume-input-parser'
import { DEFAULT_ENABLED_CLI_PROVIDERS } from '@shared/coding-cli-defaults'

// Local copy of Sidebar.tsx's module-private formatRelativeTime (verified NOT
// exported anywhere; importing from Sidebar would create a circular import
// since Sidebar renders this dialog).
function formatRelativeTime(timestamp: number): string {
  const now = Date.now()
  const diff = now - timestamp
  const minutes = Math.floor(diff / 60000)
  const hours = Math.floor(diff / 3600000)
  const days = Math.floor(diff / 86400000)

  if (minutes < 1) return 'now'
  if (minutes < 60) return `${minutes}m`
  if (hours < 24) return `${hours}h`
  if (days < 7) return `${days}d`
  return new Date(timestamp).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

// Focus-trap helper — same pattern as src/components/ui/confirm-modal.tsx
// (repo modal a11y convention: trap Tab, restore focus, lock scroll, doc-level Escape).
function getFocusable(container: HTMLElement): HTMLElement[] {
  const selectors = [
    'button',
    '[href]',
    'input',
    'select',
    'textarea',
    '[tabindex]:not([tabindex="-1"])',
  ]
  return Array.from(container.querySelectorAll<HTMLElement>(selectors.join(',')))
    .filter((el) => !el.hasAttribute('disabled') && !el.getAttribute('aria-hidden'))
}

export type ResumeSessionDialogProps = {
  open: boolean
  onClose: () => void
  onResume: (opts: {
    provider: ResumeProviderName
    sessionId: string
    sessionType: string
    cwd?: string
    title?: string
    firstUserMessage?: string
  }) => void
}

const PROVIDER_LABELS: Record<ResumeProviderName, string> = {
  claude: 'Claude Code',
  codex: 'Codex',
  opencode: 'OpenCode',
  amplifier: 'Amplifier',
}

const CLOSE_AFTER_RESUME_MS = 1500

export default function ResumeSessionDialog({ open, onClose, onResume }: ResumeSessionDialogProps) {
  const [inputValue, setInputValue] = useState('')
  const [picker, setPicker] = useState<ResumeProviderName>('claude')
  const [pickerTouched, setPickerTouched] = useState(false)
  const [resolving, setResolving] = useState(false)
  const [result, setResult] = useState<ResumeResolveResponse | null>(null)
  const [errorText, setErrorText] = useState<string | null>(null)
  const [note, setNote] = useState<string | null>(null)
  const [anywayCwd, setAnywayCwd] = useState('')
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const dialogRef = useRef<HTMLDivElement>(null)
  const closeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const previousFocusRef = useRef<HTMLElement | null>(null)
  const previousOverflowRef = useRef<string | null>(null)
  // Stale-response guard: only the LATEST resolve request may mutate state.
  const resolveSeqRef = useRef(0)

  useEffect(() => {
    if (!open) {
      setInputValue(''); setResult(null); setErrorText(null); setNote(null)
      setResolving(false); setPickerTouched(false)
      resolveSeqRef.current += 1 // invalidate any in-flight resolve
    }
    return () => { if (closeTimerRef.current) clearTimeout(closeTimerRef.current) }
  }, [open])

  // Modal a11y (mirrors src/components/ui/confirm-modal.tsx): capture + restore
  // the previously focused element, lock background scroll, focus the paste field.
  useEffect(() => {
    if (!open) return
    previousFocusRef.current = document.activeElement as HTMLElement | null
    previousOverflowRef.current = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    inputRef.current?.focus()
    return () => {
      document.body.style.overflow = previousOverflowRef.current || ''
      previousFocusRef.current?.focus()
    }
  }, [open])

  // Document-level Escape (works regardless of where focus sits).
  useEffect(() => {
    if (!open) return
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') { e.preventDefault(); onClose() }
    }
    document.addEventListener('keydown', handleKey)
    return () => document.removeEventListener('keydown', handleKey)
  }, [open, onClose])

  const finishResume = useCallback((m: ResumeResolveMatch) => {
    // A match without a recorded cwd never auto-resumes (see runResolve);
    // when the user confirms it from the match list, the editable
    // working-directory field below supplies the concrete cwd (spec). A
    // session must NEVER open without a concrete cwd — an empty/whitespace
    // field blocks the confirm with an inline error instead of silently
    // launching a cwd-less tuple.
    const cwd = m.cwd ?? anywayCwd.trim()
    if (!cwd) {
      setErrorText('Enter a working directory to open this session.')
      return
    }
    onResume({
      provider: m.provider,
      sessionId: m.sessionId,
      sessionType: m.sessionType,
      cwd,
      title: m.title,
      firstUserMessage: m.firstUserMessage,
    })
    setNote(`Found in ${PROVIDER_LABELS[m.provider] ?? m.provider}`)
    closeTimerRef.current = setTimeout(onClose, CLOSE_AFTER_RESUME_MS)
  }, [anywayCwd, onClose, onResume])

  const runResolve = useCallback(async (text: string) => {
    const trimmed = text.trim()
    if (!trimmed) return
    // Stale-response guard: bump the sequence; only the LATEST request may
    // mutate state. A stale single-match response must NEVER auto-resume —
    // it could open the WRONG session.
    const seq = ++resolveSeqRef.current
    setResolving(true); setErrorText(null); setNote(null); setResult(null)
    // Live local hint (advisory only — server evidence decides).
    const localParse = parseResumeInput(trimmed)
    if (localParse.agentHint && !pickerTouched) setPicker(localParse.agentHint.provider)
    let response: ResumeResolveResponse
    try {
      response = await resolveResumeInput(trimmed)
    } catch (err) {
      if (seq !== resolveSeqRef.current) return // stale — ignore
      setResolving(false)
      // Deployment degradation contract (Task 1): the canonical Rust-served
      // production has no resolve endpoint — a 404 gets an explicit message.
      setErrorText(err instanceof ApiError && err.status === 404
        ? 'This server build does not support resume-by-id yet.'
        : 'Could not reach the server. Try again.')
      return
    }
    if (seq !== resolveSeqRef.current) return // stale — ignore
    setResolving(false)
    setResult(response)
    setAnywayCwd(response.homeDir)
    if (response.agentHint && !pickerTouched) setPicker(response.agentHint.provider)
    if (response.tokens.length === 0) {
      setErrorText('No session id found in the pasted text.')
      return
    }
    // Auto-resume ONLY on a fully-healthy response: a 'degraded' response
    // means some provider FAILED, so a higher-priority exact match may have
    // been missed — auto-opening the surviving match could open the WRONG
    // session (hard requirement). Degraded matches render in the list below
    // with the degraded notice + Retry instead.
    if (response.matches.length === 1 && response.matches[0].cwd && response.indexState === 'ready') {
      finishResume(response.matches[0])
      return
    }
    // A single match WITHOUT a recorded cwd (exact-id fallback hit) must NOT
    // auto-open — the spec requires a concrete working directory. It renders
    // in the match list below alongside an editable working-directory field.
    if (response.matches.length === 0 && response.indexState === 'ready') {
      // Absence claims must name what was NOT searched (disabled providers) —
      // otherwise "not found" implies the id does not exist anywhere.
      setErrorText(response.unsearchedProviders.length > 0
        ? `No matching session found. Not searched (disabled): ${response.unsearchedProviders.join(', ')}.`
        : 'No matching session found in any agent.')
    }
  }, [finishResume, pickerTouched])

  const handleResumeAnyway = useCallback(() => {
    // CURRENT input first: result is cleared on edit, but never act on a
    // stale token when the user has typed something new.
    const token = parseResumeInput(inputValue).candidates[0] ?? result?.tokens[0]
    // A concrete cwd is REQUIRED (spec: never open without one) — the button
    // below is disabled when the field is blank; this guard is the backstop.
    const cwd = anywayCwd.trim()
    if (!token || !cwd) return
    onResume({ provider: picker, sessionId: token, sessionType: picker, cwd })
    onClose()
  }, [anywayCwd, inputValue, onClose, onResume, picker, result])

  if (!open) return null

  // warming AND degraded are retry states — NEITHER is "not found" (spec:
  // absence needs evidence; provider unavailable gets loading/retry). The
  // retry notice shows for EVERY warming/degraded response — with OR without
  // matches — because degraded matches may be incomplete/lower-priority.
  const retryState = result !== null && result.tokens.length > 0
    && (result.indexState === 'warming' || result.indexState === 'degraded')
    ? result.indexState
    : null
  const matchesToShow = result?.matches ?? []
  // >1 = disambiguation; ==1 without cwd = needs-working-directory
  // confirmation; ==1 on a DEGRADED response = manual confirmation required
  // (a lone match WITH cwd on a READY response auto-resumed in runResolve
  // and never reaches here).
  const showMatchList = matchesToShow.length > 1
    || (matchesToShow.length === 1 && (!matchesToShow[0].cwd || result?.indexState === 'degraded'))
  const showResumeAnyway = errorText !== null && errorText.startsWith('No matching session')
  // Editable working directory: shown for resume-anyway AND for listed matches
  // lacking a recorded cwd (spec: never open without a concrete cwd).
  const showCwdInput = showResumeAnyway || (showMatchList && matchesToShow.some((m) => !m.cwd))

  return createPortal(
    <div
      className={`fixed inset-0 bg-black/50 flex items-center justify-center p-4 ${OVERLAY_Z.modal}`}
      role="presentation"
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose() }}
    >
      {/* eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions */}
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="resume-session-dialog-title"
        data-testid="resume-session-dialog"
        className="bg-card border border-border rounded-lg shadow-lg w-full max-w-lg p-4 flex flex-col gap-3"
        onKeyDown={(e) => {
          if (e.key === 'Escape') { e.stopPropagation(); onClose(); return }
          if (e.key !== 'Tab') return
          // Focus trap — repo modal pattern (see src/components/ui/confirm-modal.tsx).
          const dialog = dialogRef.current
          if (!dialog) return
          const focusables = getFocusable(dialog)
          if (focusables.length === 0) { e.preventDefault(); return }
          const first = focusables[0]
          const last = focusables[focusables.length - 1]
          const active = document.activeElement as HTMLElement | null
          if (e.shiftKey) {
            if (active === first || !dialog.contains(active)) {
              e.preventDefault()
              last.focus()
            }
          } else if (active === last) {
            e.preventDefault()
            first.focus()
          }
        }}
      >
        <h2 id="resume-session-dialog-title" className="text-sm font-medium">Resume a session</h2>

        <div className="flex flex-col gap-1">
          <label htmlFor="resume-agent-picker" className="text-xs text-muted-foreground">Agent (auto-detected — used only when no match is found)</label>
          <select
            id="resume-agent-picker"
            className="bg-background border border-border rounded px-2 py-1.5 text-sm"
            value={picker}
            onChange={(e) => { setPicker(e.target.value as ResumeProviderName); setPickerTouched(true) }}
          >
            {DEFAULT_ENABLED_CLI_PROVIDERS.map((p) => (
              <option key={p} value={p}>{PROVIDER_LABELS[p as ResumeProviderName] ?? p}</option>
            ))}
          </select>
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="resume-input" className="text-xs text-muted-foreground">Resume string (paste anything containing a session id)</label>
          <textarea
            id="resume-input"
            ref={inputRef}
            rows={3}
            className="bg-background border border-border rounded px-2 py-1.5 text-sm font-mono resize-none"
            placeholder='e.g. "codex resume 019fac27-…" or 417e8345'
            value={inputValue}
            onChange={(e) => {
              setInputValue(e.target.value)
              // EDITING invalidates everything derived from the previous
              // text: bump the sequence so in-flight responses go stale, and
              // clear result/error/note so stale "Resume anyway" or
              // disambiguation actions can never act on old tokens.
              resolveSeqRef.current += 1
              setResolving(false); setResult(null); setErrorText(null); setNote(null)
            }}
            onPaste={(e) => {
              // Paste-then-Enter fast path: auto-resolve on paste. Read BOTH the
              // element value (real browsers update it after the event) and the
              // clipboard payload (jsdom never updates the value on paste).
              const pasted = e.clipboardData?.getData('text') ?? ''
              const target = e.currentTarget
              setTimeout(() => {
                const value = target.value.trim() ? target.value : pasted
                if (!value.trim()) return
                if (!target.value.trim()) setInputValue(pasted)
                void runResolve(value)
              }, 0)
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void runResolve(inputValue) }
            }}
          />
        </div>

        {resolving && (
          <div role="status" className="flex items-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" /> Searching all agents…
          </div>
        )}

        {note && <div role="status" className="text-sm text-emerald-500">{note}</div>}

        {retryState && (
          <div role="status" className="flex items-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
            {retryState === 'warming'
              ? 'Session index is still warming — this is not a "not found".'
              : matchesToShow.length > 0
                ? 'Some agents could not be searched right now — the matches below may be incomplete. Confirm one manually or retry.'
                : 'Some agents could not be searched right now — this is not a "not found".'}
            <button
              type="button"
              className="underline hover:text-foreground"
              onClick={() => void runResolve(inputValue)}
            >
              Retry
            </button>
          </div>
        )}

        {showMatchList && (
          <ul aria-label="Matching sessions" className="flex flex-col gap-1 max-h-64 overflow-y-auto">
            {matchesToShow.map((m) => (
              <li key={`${m.provider}:${m.sessionId}`}>
                <button
                  type="button"
                  className="w-full text-left rounded border border-border px-2 py-1.5 hover:bg-muted/50"
                  aria-label={`Resume ${PROVIDER_LABELS[m.provider] ?? m.provider} session ${m.title ?? m.sessionId}`}
                  onClick={() => finishResume(m)}
                >
                  <div className="text-sm truncate">{m.title || m.firstUserMessage || m.sessionId}</div>
                  <div className="text-2xs text-muted-foreground truncate">
                    {PROVIDER_LABELS[m.provider] ?? m.provider} · {m.cwd ?? m.projectPath} · {formatRelativeTime(m.lastActivityAt)}
                  </div>
                </button>
              </li>
            ))}
          </ul>
        )}

        {errorText && <div role="alert" className="text-sm text-red-500">{errorText}</div>}

        {showCwdInput && (
          <div className="flex flex-col gap-2 border-t border-border pt-2">
            <div className="flex flex-col gap-1">
              <label htmlFor="resume-anyway-cwd" className="text-xs text-muted-foreground">
                {showResumeAnyway
                  ? 'Working directory'
                  : 'Working directory (this session has no recorded one — required to open it)'}
              </label>
              <input
                id="resume-anyway-cwd"
                className="bg-background border border-border rounded px-2 py-1.5 text-sm font-mono"
                value={anywayCwd}
                onChange={(e) => setAnywayCwd(e.target.value)}
              />
            </div>
            {showResumeAnyway && (
              <button
                type="button"
                className="self-start rounded border border-border px-3 py-1.5 text-sm hover:bg-muted/50 disabled:opacity-50 disabled:cursor-not-allowed"
                onClick={handleResumeAnyway}
                disabled={!anywayCwd.trim()}
              >
                Resume anyway with {PROVIDER_LABELS[picker]}
              </button>
            )}
          </div>
        )}

        <div className="flex justify-end gap-2 pt-1">
          <button
            type="button"
            className="rounded px-3 py-1.5 text-sm text-muted-foreground hover:text-foreground hover:bg-muted/50"
            onClick={onClose}
          >
            Cancel
          </button>
          <button
            type="button"
            className="rounded border border-border px-3 py-1.5 text-sm hover:bg-muted/50"
            onClick={() => void runResolve(inputValue)}
            disabled={resolving || inputValue.trim() === ''}
          >
            Find session
          </button>
        </div>
      </div>
    </div>,
    document.body,
  )
}
