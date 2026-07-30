import os from 'os'
import { Router } from 'express'
import { z } from 'zod'
import { cleanString } from './utils.js'
import { parseResumeInput } from '../shared/resume-input-parser.js'
import { resolveSessionCandidates } from './coding-cli/session-resolver.js'
import { buildResolveFallbacks, withRequestBudget, type ResolveFallbacks } from './coding-cli/resolve-fallbacks.js'
import { makeSessionKey, type CodingCliProviderName } from './coding-cli/types.js'
import type { CodingCliProvider } from './coding-cli/provider.js'
import { CodingCliProviderSchema } from '../shared/ws-protocol.js'
import { logger } from './logger.js'
import { setResponsePerfContext } from './request-logger.js'
import { cascadeSessionRenameToTerminal } from './rename-cascade.js'
import { AI_CONFIG } from './ai-prompts.js'
import { generateAiSessionTitle } from './ai-title.js'
import { extractTitleFromMessage } from '../shared/title-utils.js'
import type { TerminalMeta } from './terminal-metadata-service.js'
import type { SessionMetadataStore } from './session-metadata-store.js'
import { DEFAULT_CLI_PROVIDER_NAMES } from './platform.js'
import { SessionDirectoryQuerySchema } from '../shared/read-models.js'
import {
  KnownSessionMetadataTypeSchema,
  SessionTypeMetadataSourceSchema,
} from '../shared/session-flavor.js'
import { querySessionDirectory } from './session-directory/service.js'
import { createRequestAbortSignal } from './read-models/request-abort.js'
import {
  defaultReadModelScheduler,
  isReadModelAbortError,
  type ReadModelWorkScheduler,
} from './read-models/work-scheduler.js'

const log = logger.child({ component: 'sessions-router' })

export const ResolveBodySchema = z.object({ input: z.string().min(1).max(20_000) })

export const SessionPatchSchema = z.object({
  titleOverride: z.string().optional().nullable(),
  summaryOverride: z.string().optional().nullable(),
  deleted: z.coerce.boolean().optional(),
  archived: z.coerce.boolean().optional(),
  createdAtOverride: z.coerce.number().optional(),
})

export interface SessionsRouterDeps {
  configStore: {
    getSettings: () => Promise<any>
    patchSessionOverride: (key: string, data: any) => Promise<any>
    deleteSession: (key: string) => Promise<void>
  }
  codingCliIndexer: {
    getProjects: () => any[]
    refresh: () => Promise<void>
    isReady?: () => boolean
    getScanFailures?: () => CodingCliProviderName[]
    requestRefresh?: () => void
  }
  codingCliProviders: CodingCliProvider[]
  /** Test override for the exact-id fallbacks (defaults to buildResolveFallbacks(...)). */
  resolveFallbacks?: ResolveFallbacks
  /** Test override for the "resume anyway" default cwd (defaults to os.homedir()). */
  homeDir?: string
  perfConfig: { slowSessionRefreshMs: number }
  terminalMetadata?: { list: () => TerminalMeta[] }
  registry?: { updateTitle: (id: string, title: string) => void }
  wsHandler?: { broadcastTerminalsChanged?: () => void }
  sessionMetadataStore?: SessionMetadataStore
  serverInstanceId?: string
  validCliProviders?: string[]
  readModelScheduler?: ReadModelWorkScheduler
}

export function createSessionsRouter(deps: SessionsRouterDeps): Router {
  const { configStore, codingCliIndexer, codingCliProviders, perfConfig } = deps
  const router = Router()
  const readModelScheduler = deps.readModelScheduler ?? defaultReadModelScheduler
  const validCliProviders = new Set(deps.validCliProviders ?? DEFAULT_CLI_PROVIDER_NAMES)
  const sessionMetadataProviderSchema = z.string().min(1).superRefine((value, ctx) => {
    if (validCliProviders.has(value)) return
    ctx.addIssue({
      code: z.ZodIssueCode.custom,
      message: `Unknown CLI provider: '${value}'`,
    })
  })

  router.get('/session-directory', async (req, res) => {
    const parsed = SessionDirectoryQuerySchema.safeParse({
      query: typeof req.query.query === 'string' ? req.query.query : undefined,
      tier: typeof req.query.tier === 'string' ? req.query.tier : undefined,
      cursor: typeof req.query.cursor === 'string' ? req.query.cursor : undefined,
      priority: req.query.priority,
      revision: typeof req.query.revision === 'string' ? Number(req.query.revision) : undefined,
      limit: typeof req.query.limit === 'string' ? Number(req.query.limit) : undefined,
      includeSubagents: req.query.includeSubagents,
      includeNonInteractive: req.query.includeNonInteractive,
      includeEmpty: req.query.includeEmpty,
    })

    if (!parsed.success) {
      return res.status(400).json({ error: 'Invalid request', details: parsed.error.issues })
    }

    const signal = createRequestAbortSignal(req, res)

    try {
      const page = await readModelScheduler.schedule({
        lane: parsed.data.priority,
        signal,
        run: (scheduledSignal) => querySessionDirectory({
          projects: codingCliIndexer.getProjects(),
          query: parsed.data,
          terminalMeta: deps.terminalMetadata?.list() ?? [],
          providers: codingCliProviders,
          signal: scheduledSignal,
        }),
      })
      setResponsePerfContext(res, {
        readModelLane: parsed.data.priority,
        responsePayloadBytes: Buffer.byteLength(JSON.stringify(page), 'utf8'),
      })
      res.json(page)
    } catch (error) {
      if (signal.aborted || isReadModelAbortError(error)) {
        return
      }
      const message = error instanceof Error ? error.message : 'Session directory query failed'
      const status = /cursor/i.test(message) ? 400 : 500
      if (status === 500) {
        log.error({ err: error }, 'Session directory query failed')
      }
      res.status(status).json({ error: message })
    }
  })

  router.patch('/sessions/:sessionId', async (req, res) => {
    const rawId = req.params.sessionId
    const provider = (req.query.provider as CodingCliProviderName) || 'claude'
    const compositeKey = rawId.includes(':') ? rawId : makeSessionKey(provider, rawId)
    const parsed = SessionPatchSchema.safeParse(req.body || {})
    if (!parsed.success) {
      return res.status(400).json({ error: 'Invalid request', details: parsed.error.issues })
    }
    const { titleOverride, summaryOverride, deleted, archived, createdAtOverride } = parsed.data
    const next = await configStore.patchSessionOverride(compositeKey, {
      titleOverride: cleanString(titleOverride),
      ...(cleanString(titleOverride) ? { titleSource: 'user' as const } : {}),
      summaryOverride: cleanString(summaryOverride),
      deleted,
      archived,
      createdAtOverride,
    })

    // Cascade: if this session is running in a terminal, also rename the terminal
    const cleanTitle = cleanString(titleOverride)
    let cascadedTerminalId: string | undefined
    if (cleanTitle && deps.terminalMetadata) {
      try {
        const parts = compositeKey.split(':')
        const sessionProvider = (parts.length >= 2 ? parts[0] : provider) as CodingCliProviderName
        const sessionId = parts.length >= 2 ? parts.slice(1).join(':') : rawId
        cascadedTerminalId = await cascadeSessionRenameToTerminal(
          deps.terminalMetadata.list(),
          sessionProvider,
          sessionId,
          cleanTitle,
        )
        if (cascadedTerminalId) {
          deps.registry?.updateTitle(cascadedTerminalId, cleanTitle)
          deps.wsHandler?.broadcastTerminalsChanged?.()
        }
      } catch (err) {
        log.warn({ err, compositeKey }, 'Cascade rename to terminal failed (non-fatal)')
      }
    }

    await codingCliIndexer.refresh()
    res.json({ ...next, cascadedTerminalId })
  })

  router.post('/sessions/:sessionId/generate-title', async (req, res) => {
    const rawId = req.params.sessionId
    const provider = (req.query.provider as CodingCliProviderName) || 'claude'
    const compositeKey = rawId.includes(':') ? rawId : makeSessionKey(provider, rawId)

    const firstMessage = typeof req.body?.firstMessage === 'string' ? req.body.firstMessage : ''
    if (!firstMessage.trim()) {
      return res.status(400).json({ error: 'firstMessage is required' })
    }

    // An authoritative provider-generated title (e.g. Amplifier's own AI-generated
    // name) is already the canonical name. Short-circuit before any 'ai' override
    // write so freshell never shadows it.
    const parsed = deps.codingCliIndexer
      .getProjects()
      .flatMap((p) => p.sessions)
      .find((s) => makeSessionKey(s.provider, s.sessionId) === compositeKey)
    if (parsed?.titleSource === 'provider-generated') {
      return res.json({ title: parsed.title ?? null, source: 'provider-generated' })
    }

    // No Gemini key: finalize from the first user message instead of failing.
    // Uses the same (default) length as the client first-message title so the
    // persisted name matches and there is no visible flip.
    if (!AI_CONFIG.enabled()) {
      const fallback = extractTitleFromMessage(firstMessage)
      if (!fallback) {
        return res.json({ title: null, source: 'none' })
      }
      const result = await configStore.patchSessionOverride(compositeKey, {
        titleOverride: fallback,
        titleSource: 'first-message',
      })
      await codingCliIndexer.refresh()
      return res.json({ title: result.titleOverride, source: result.titleSource })
    }

    try {
      const settings = await configStore.getSettings()
      const title = await generateAiSessionTitle(firstMessage, settings.ai?.titlePrompt)
      if (!title) {
        return res.json({ title: null, source: 'none' })
      }

      const stored = await configStore.patchSessionOverride(compositeKey, {
        titleOverride: title,
        titleSource: 'ai',
      })
      await codingCliIndexer.refresh()
      res.json({ title: stored.titleOverride, source: stored.titleSource })
    } catch (err: any) {
      log.warn({ err }, 'AI title generation failed')
      res.json({ title: null, source: 'none', error: err.message })
    }
  })

  router.delete('/sessions/:sessionId', async (req, res) => {
    const rawId = req.params.sessionId
    const provider = (req.query.provider as CodingCliProviderName) || 'claude'
    const compositeKey = rawId.includes(':') ? rawId : makeSessionKey(provider, rawId)
    await configStore.deleteSession(compositeKey)
    await codingCliIndexer.refresh()
    res.json({ ok: true })
  })

  const SessionMetadataPostSchema = z.object({
    provider: sessionMetadataProviderSchema,
    sessionId: z.string().min(1),
    sessionType: KnownSessionMetadataTypeSchema,
    sessionTypeSource: SessionTypeMetadataSourceSchema.optional(),
  })

  router.post('/session-metadata', async (req, res) => {
    if (!deps.sessionMetadataStore) {
      return res.status(500).json({ error: 'Session metadata store not configured' })
    }
    const parsed = SessionMetadataPostSchema.safeParse(req.body ?? {})
    if (!parsed.success) {
      return res.status(400).json({ error: 'Missing required fields: provider, sessionId, sessionType', details: parsed.error.issues })
    }
    const { provider, sessionId, sessionType, sessionTypeSource } = parsed.data
    const changed = await deps.sessionMetadataStore.set(provider, sessionId, {
      sessionType,
      ...(sessionTypeSource ? { sessionTypeSource } : {}),
    })
    if (changed) {
      await codingCliIndexer.refresh()
    }
    res.json({ ok: true, changed })
  })

  // Resume-button resolve: one scan answers all providers at once (spec:
  // docs/plans/2026-07-29-resume-button-spec.md). Evidence decides the agent;
  // the parsed hint is advisory UI state only. ACCEPTED LIMITATION (spec-
  // sanctioned): prefix matching covers indexed sessions only; exact-id
  // misses fall back to the claude transcript locator + opencode by-id query.
  const resolveFallbacks = deps.resolveFallbacks
    ?? buildResolveFallbacks(deps.codingCliProviders, { sessionMetadataStore: deps.sessionMetadataStore })
  const KNOWN_RESUME_PROVIDERS = ['claude', 'codex', 'opencode', 'amplifier'] as const
  router.post('/sessions/resolve', async (req, res) => {
    const parsed = ResolveBodySchema.safeParse(req.body)
    if (!parsed.success) {
      res.status(400).json({ error: 'body must be { input: string } (1-20000 chars)' })
      return
    }
    try {
      // The indexer scans ONLY settings-enabled providers (verified:
      // session-indexer.ts filters by enabledProviders, ~line 1385). A
      // disabled provider's sessions cannot be found — report it as
      // UNSEARCHED so the client never implies the id does not exist.
      const settings = await deps.configStore.getSettings().catch(() => ({}))
      const enabled = new Set<string>(
        settings?.codingCli?.enabledProviders ?? KNOWN_RESUME_PROVIDERS,
      )
      const unsearchedProviders = KNOWN_RESUME_PROVIDERS.filter((n) => !enabled.has(n))
      const { candidates, agentHint } = parseResumeInput(parsed.data.input)
      const { matches, providerErrors: fallbackErrors } = await resolveSessionCandidates(candidates, {
        getProjects: () => deps.codingCliIndexer.getProjects(),
        // Fresh budget per request: bounds fallback work; shape gates run
        // before the budget inside withRequestBudget.
        fallbacks: withRequestBudget(resolveFallbacks),
      })
      // A provider whose last index scan FAILED was not searched either —
      // the indexer swallows listing failures and still reports ready. A
      // DISABLED provider is UNSEARCHED (reported above), never a provider
      // error: without this filter a failed-then-disabled provider would
      // keep responses degraded forever (no successful scan can ever clear
      // it) and trap the user in a Retry loop.
      const scanFailures = (deps.codingCliIndexer.getScanFailures?.() ?? [])
        .filter((name) => enabled.has(name))
      const providerErrors = [...new Set([...fallbackErrors, ...scanFailures])]
      // degraded = something FAILED — even when matches exist: a failed
      // provider means a HIGHER-priority exact match may have been missed,
      // so the client must not auto-resume a surviving lower-priority match
      // (and with zero matches it shows retry, never "no matching session").
      const indexState: 'ready' | 'warming' | 'degraded' =
        deps.codingCliIndexer.isReady?.() === false ? 'warming'
        : providerErrors.length > 0 ? 'degraded'
        : 'ready'
      // Fire-and-forget: give the user's Retry a chance to converge once a
      // failed provider recovers (scan failures only clear on a new scan).
      if (indexState === 'degraded') deps.codingCliIndexer.requestRefresh?.()
      res.json({
        indexState,
        tokens: candidates,
        agentHint: agentHint ?? null,
        homeDir: deps.homeDir ?? os.homedir(),
        providerErrors,
        unsearchedProviders,
        matches,
      })
    } catch (err) {
      log.warn({ err }, 'sessions/resolve failed')
      res.status(500).json({ error: 'resolve failed' })
    }
  })

  return router
}
