# Resume Session Button — freshell left rail

## Summary

Add a **Resume** button to the left rail (Sidebar) that is **always on screen**: it is
rendered in a pinned footer region of the rail, BELOW the infinite-scroll session list's
scroll viewport — NOT as an item inside the scrollable list. It must be visible at every
scroll position (top/middle/bottom) and in every rail presentation where the list is
visible, including the `fullWidth` mobile mode.

Implementation note (verified): the Sidebar root is `h-full flex flex-col` and the list
wrapper is the `flex flex-1 min-h-0` region — the footer is a sibling div AFTER that
wrapper. Give it a `data-testid` for tests.

Clicking it opens a Resume dialog: an agent picker (advisory — see resolution engine) and
a paste field for a resume string. freshell resolves the string to a concrete
(agent, full session id, cwd, sessionType) tuple and resumes that session in a tab.

## Supported agents

The CLI providers freshell already supports: `claude`, `codex`, `opencode`, `amplifier`
(`shared/coding-cli-defaults.ts` DEFAULT_ENABLED_CLI_PROVIDERS). The picker lists these;
detection may resolve to any of them regardless of picker state.

## Target backend

The resolve capability must live in the server that actually serves the app in the
default `dev`/`start` path — per current `package.json` that is the **Node server**
(`server/index.ts`), which has its own session indexer
(`server/coding-cli/session-indexer.ts`). The Rust server's `IndexExistenceProbe`
(`crates/freshell-server/src/existence.rs`) is exact-match-only, WS/reconcile-internal,
and is NOT this feature's API. The planner must confirm which server serves the sidebar
in supported deployments and implement there; parity in the other server is out of scope
for this feature.

**New work, stated explicitly:** a resolve endpoint (e.g. `POST /api/sessions/resolve`
or equivalent) that scans the session index across ALL providers at once for exact and
prefix matches and returns per-candidate metadata. Nothing existing does prefix search;
the existing exact-id fallbacks (claude transcript locator, opencode by-id DB query) may
be reused for exact-id misses. Prefix matching may be limited to indexed sessions —
if extending prefix search to fallback stores is non-trivial, document
"prefix only matches indexed sessions" as an accepted limitation rather than building
new store scanners.

## Extremely permissive input parsing

Accept arbitrary pasted text and extract candidate session ids. Token shapes:

- Full UUIDs (any version, any case, `8-4-4-4-12`).
- Opencode ids: `ses_` + 26 base62 chars (e.g. `ses_root0000000000000000000000`) —
  first-class token shape; also accept other known `xxx_`-prefixed id families.
- Short hex tokens as id **prefixes** (amplifier surfaces short ids like `417e8345`):
  require **≥8 hex chars containing at least one digit** (avoids matching English words
  like "decade"/"facade"), up to 32 chars.
- Strip surrounding noise: full command lines (`codex resume <id>`,
  `claude --resume <id>`, `claude -r <id>`, `amplifier --resume <id>`,
  `opencode --session <id>`), quotes, backticks, whitespace/newlines, shell prompts
  (`$`, `>`), flags, and ids embedded in longer strings or paths.
- If multiple candidate tokens exist: prefer prefixed ids (`ses_…`) and full UUIDs,
  then the longest hex token; try candidates in that order until one resolves.

The parser is a pure, shared function (usable by client for live feedback and by tests).

## Getting the agent RIGHT (hard requirement)

A resume attempted with the wrong agent fails — the resolved agent must be correct.
Strategy: **evidence decides; hints only assist the UI.**

1. **Evidence (decisive):** one resolve call scans the session index snapshot across all
   four providers simultaneously (exact match AND prefix match). No per-agent probe
   ordering — a single scan answers all agents at once. On an exact-id miss against the
   index, reuse the existing exact-id fallbacks (claude transcript locator, opencode
   by-id) before concluding absence.
2. **Hints (advisory only, labeled unverified):** explicit agent words in the pasted
   text, CLI command shapes, and id-format heuristics (codex ids are *typically* UUIDv7,
   claude's v4, short-hex *suggests* amplifier — but none of this is guaranteed; codex
   ids can be non-UUID, and amplifier ids are full UUIDs whose short form is a prefix).
   Hints are used ONLY to pre-fill the agent picker and as the default agent for the
   "resume anyway" escape hatch. The user's picker choice is itself a hint — store
   evidence overrides it.
3. **Resolve responses carry full resume metadata** for each match: provider, full
   session id, `cwd`, `sessionType`, title/first-message snippet, last-modified — the
   existing tab-resume path requires sessionId, provider, sessionType, and cwd, not just
   (agent, id).
4. **Outcomes:**
   - Exactly one match → resume with that provider (even if the picker disagreed); show
     a small non-blocking note ("found in codex").
   - Multiple matches (across providers, or a prefix matching several sessions) → show a
     disambiguation list (capped, e.g. 20, most-recent first) with the metadata above;
     one click resumes.
   - Zero matches with the index **ready** → clear inline error, input preserved.
     Offer a "resume anyway" escape hatch: attempts a verbatim resume with the
     picker-selected (or hint-default) agent, with cwd defaulting to the user's home
     directory and shown/editable before launch.
   - Index **warming / provider unavailable / unknown** → this is NOT "not found":
     show a loading/retry state and re-resolve when the index is ready.
5. A tab is only created once a concrete (provider, full id, cwd, sessionType) tuple is
   in hand (except the explicit "resume anyway" path).

## Resume in a tab

Reuse the existing tab-resume mechanics (the same path the sidebar uses when opening a
session: `openSessionTab`/`buildResumeContent` and `session-type-utils`). Follow the
sidebar's existing dedup convention: if the session is already open in a pane
(`findPaneForSession`), focus that pane instead of spawning a duplicate; otherwise open
a **new tab**, focused, running the correct agent with the FULL session id.

## Acceptance examples (must be tests)

| Pasted input | Expected |
|---|---|
| `417e8345` | prefix-match in amplifier store → resume amplifier session |
| `codex resume 019fac27-69d7-78a0-b972-b339d551042e` | codex (hint + store evidence agree) |
| `ed2afda6-a340-443e-ba60-024a1b3554b4` | no hint; resolve finds it under claude → resume claude |
| `opencode --session ses_root0000000000000000000000` | opencode (prefixed id shape) |
| bare `ses_…` id with picker set to claude | evidence wins → opencode, with a note |
| `  "claude --resume ed2afda6-…"  ` with picker set to codex | evidence wins → claude, with a note |
| prefix matching multiple sessions | disambiguation list, capped, most-recent first |
| valid id, index still warming | loading/retry state, NOT "not found" |
| garbage with no id-like token | inline error, no tab created |
| session already open in a pane | focuses existing pane, no duplicate tab |

## Non-functional requirements

- Parser: pure function, table-driven unit tests including the cases above plus
  adversarial noise (multi-line paste, ANSI codes, trailing punctuation, `decade`-style
  hex-looking words must NOT match).
- Resolve endpoint: tested against fixture session stores/indexes for all four agents
  (exact, prefix, ambiguous, missing, warming).
- UI: pinned placement verified at top/middle/bottom scroll and in `fullWidth` mobile
  mode; keyboard accessible; paste-then-Enter fast path (auto-resolve on paste).
- Follow repo conventions (AGENTS.md / CLAUDE.md), match existing Sidebar patterns,
  TDD where the repo requires it.
