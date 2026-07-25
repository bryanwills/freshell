#!/usr/bin/env node
// Fake `amplifier` CLI for the TERM-15/TERM-16 activity e2e
// (`terminal-activity-rust.spec.ts`), under the LAUNCHER-ASSIGNED identity
// mechanism (mirrors `fake-amplifier-cli.mjs`): the broker mints a UUID at
// terminal create, pre-creates the session stub dir (metadata.json + empty
// transcript.jsonl + empty events.jsonl), and ALWAYS spawns
// `amplifier resume <uuid>` -- for FRESH panes too. The old locator/
// association sweep is deleted; the create-time events-lane resolver already
// watches the stub's events.jsonl. So this fixture takes the session id from
// argv, ADOPTS the broker's pre-created stub, and appends its
// ACTIVITY-relevant records (schema-carrying `amplifier.log` lifecycle) to
// THAT stub's events.jsonl:
//
//   - FIRST Enter: stamps the stub's "used" signature (turn_count into
//     metadata.json + one transcript line -- what the broker's stub GC
//     respects), then appends `prompt:submit` (the record that CONFIRMS the
//     tracker's provisional busy), then after a delay appends
//     `prompt:complete` (the single turn boundary -> terminal.turn.complete).
//   - LATER Enters: append `prompt:submit`, then `prompt:complete` after the
//     delay -- subsequent turns on the same session.
//
// All records carry live `ts` (the tracker folds ts into liveness -- a stale
// fixture ts would look like >deadman silence), the launcher-assigned
// `session_id`, and the real schema gate (`amplifier.log` major 1); without
// it the Rust lane degrades by design.

import fs from 'node:fs'
import path from 'node:path'

function amplifierHome() {
  // Mirror the Rust broker's resolve_amplifier_home() (validated F1):
  // FRESHELL_AMPLIFIER_HOME override else $HOME/.amplifier (the e2e harness
  // sandboxes HOME). The real CLI's AMPLIFIER_HOME is caches-only and must
  // NOT be consulted -- server and fake CLI must resolve the SAME home.
  if (process.env.FRESHELL_AMPLIFIER_HOME) return process.env.FRESHELL_AMPLIFIER_HOME
  const home = process.env.HOME || process.env.USERPROFILE || '.'
  return path.join(home, '.amplifier')
}

const argv = process.argv.slice(2)
const sessionId = argv[0] === 'resume' ? (argv[1] ?? '') : ''

/** Locate the broker's pre-created stub dir for the launcher-assigned id. */
function findStubDir() {
  const projectsDir = path.join(amplifierHome(), 'projects')
  let slugs = []
  try { slugs = fs.readdirSync(projectsDir) } catch { return null }
  for (const slug of slugs) {
    const dir = path.join(projectsDir, slug, 'sessions', sessionId)
    if (fs.existsSync(path.join(dir, 'metadata.json'))) return dir
  }
  return null
}

function record(event, extra = {}) {
  return `${JSON.stringify({
    ts: new Date().toISOString(),
    lvl: 'INFO',
    schema: { name: 'amplifier.log', ver: '1.0.0' },
    event,
    session_id: sessionId,
    ...extra,
  })}\n`
}

const TURN_MS = Number(process.env.FAKE_AMPLIFIER_TURN_MS || 1200)

process.stdout.write(`amplifier: resumed session ${sessionId}\r\n`)

let eventsPath = null

process.stdin.setEncoding('utf8')
process.stdin.on('data', () => {
  if (!eventsPath) {
    const stubDir = findStubDir()
    if (!stubDir) {
      // The stub is created by the broker BEFORE spawn; its absence means the
      // contract is broken -- fail loudly rather than invent a session dir.
      process.stdout.write(`amplifier: ERROR no pre-created stub for ${sessionId}\r\n`)
      return
    }
    eventsPath = path.join(stubDir, 'events.jsonl')
    // Mirror the real CLI's first-turn save: the "used" signature the
    // broker's stub GC respects (used sessions survive terminal exit).
    const metaPath = path.join(stubDir, 'metadata.json')
    const meta = JSON.parse(fs.readFileSync(metaPath, 'utf8'))
    meta.turn_count = (meta.turn_count ?? 0) + 1
    fs.writeFileSync(metaPath, JSON.stringify(meta))
    fs.appendFileSync(path.join(stubDir, 'transcript.jsonl'), `${JSON.stringify({ role: 'user', content: 'fake turn' })}\n`)
  }
  fs.appendFileSync(eventsPath, record('prompt:submit'))
  process.stdout.write('amplifier: thinking...\r\n')
  setTimeout(() => {
    fs.appendFileSync(eventsPath, record('prompt:complete'))
    process.stdout.write('amplifier: turn complete\r\n')
  }, TURN_MS)
})
process.stdin.resume()
