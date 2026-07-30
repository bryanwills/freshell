import { z } from 'zod'

export const ResumeResolveRequestSchema = z
  .object({
    input: z.string().min(1).max(20000),
  })
  .strict()

export const ResumeResolveMatchSchema = z.object({
  provider: z.string().min(1),
  sessionId: z.string().min(1),
  cwd: z.string().optional(),
  sessionType: z.string().optional(),
  title: z.string().optional(),
  firstUserMessage: z.string().optional(),
  lastActivityAt: z.number().int().nonnegative().optional(),
  matchKind: z.enum(['exact', 'prefix']),
})

export const ResumeResolveHintSchema = z.object({
  provider: z.string().min(1),
  source: z.enum(['command', 'word', 'id-shape']),
})

/**
 * Per-provider error summary. A provider that could not be searched (locked
 * DB, unreadable store, failed index scan) is 'degraded' — NEVER "not found".
 */
export const ResumeResolveProviderErrorSchema = z.object({
  provider: z.string().min(1),
  /** errno code from the underlying failure when known (e.g. 'EACCES'). */
  code: z.string().optional(),
  message: z.string().optional(),
})

/**
 * Provider-health extension is ADDITIVE and backward-tolerant: legacy
 * responses without the new fields still parse (defaults apply), and legacy
 * clients ignore the extra fields.
 *
 * status semantics:
 *  - 'ready': every enabled provider was searched successfully.
 *  - 'warming': the index has not completed its first scan — retry, not "not found".
 *  - 'degraded': at least one provider FAILED. Even with matches present the
 *    client must never auto-resume: a failed higher-priority exact search may
 *    have hidden the right session.
 */
export const ResumeResolveResponseSchema = z.object({
  status: z.enum(['ready', 'warming', 'degraded']),
  matches: z.array(ResumeResolveMatchSchema),
  hint: ResumeResolveHintSchema.nullable(),
  providerErrors: z.array(ResumeResolveProviderErrorSchema).default([]),
  /** Settings-disabled providers — reported so "not found" never overclaims. */
  unsearchedProviders: z.array(z.string()).default([]),
  /** Server home directory, so the client can prefill a concrete cwd. */
  homeDir: z.string().optional(),
})

export type ResumeResolveRequest = z.infer<typeof ResumeResolveRequestSchema>
export type ResumeResolveMatch = z.infer<typeof ResumeResolveMatchSchema>
export type ResumeResolveHint = z.infer<typeof ResumeResolveHintSchema>
export type ResumeResolveProviderError = z.infer<typeof ResumeResolveProviderErrorSchema>
export type ResumeResolveResponse = z.infer<typeof ResumeResolveResponseSchema>
