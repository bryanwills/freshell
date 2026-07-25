#!/usr/bin/env node
// Fake `amplifier` CLI for the restore-across-restart e2e. Mirrors ONLY the
// restore-relevant behavior of the real CLI under the LAUNCHER-ASSIGNED
// identity mechanism (the broker mints a UUID at terminal create, pre-creates
// the session stub dir, and always spawns `amplifier resume <uuid>`):
//
//   - RESUME launch (`resume <id>` -- the ONLY mode the broker uses for
//     amplifier panes now): prints a deterministic, greppable marker naming
//     which id it resumed, then ADOPTS the broker's pre-created stub dir --
//     on the first line of stdin (the pane's first Enter/submit) it stamps
//     `turn_count` into the stub's metadata.json and appends one transcript
//     line, the exact "used" signature the broker's stub GC respects (used
//     sessions survive terminal exit; never-used stubs are GC'd and
//     re-stubbed on restore). Mirrors argv to `FAKE_AMPLIFIER_ARGV_LOG` if
//     set (parity with `installFakeCodexAppServer`'s
//     `FAKE_CODEX_APP_SERVER_ARG_LOG` pattern in `restore-matrix.spec.ts`)
//     so the scenario has two independent, non-DOM ways to prove the resume
//     argv.
//   - FRESH launch (no args): stays interactive and lazily creates its own
//     session dir on first stdin. This branch no longer runs for
//     broker-created amplifier panes (the broker always spawns `resume`);
//     it is kept only so the fixture stays self-consistent as a CLI.
//
// Both modes stay alive (`stdin.resume()`) so the pane's terminal status
// remains 'running', matching a real interactive TUI rather than a one-shot
// process the exit-surfacing path would treat as exited.

import fs from 'node:fs'
import path from 'node:path'

const argv = process.argv.slice(2)

function appendArgvLog() {
  const logPath = process.env.FAKE_AMPLIFIER_ARGV_LOG
  if (!logPath) return
  fs.mkdirSync(path.dirname(logPath), { recursive: true })
  fs.appendFileSync(logPath, `${JSON.stringify({ pid: process.pid, t: Date.now(), argv })}\n`)
}
appendArgvLog()

function slugify(cwd) {
  const base = path.basename(cwd) || 'root'
  const cleaned = base.replace(/[^a-zA-Z0-9-]+/g, '-').toLowerCase()
  return cleaned || 'project'
}

function amplifierHome() {
  // Mirror the Rust broker's resolve_amplifier_home() (validated F1):
  // FRESHELL_AMPLIFIER_HOME override else $HOME/.amplifier. The real CLI's
  // AMPLIFIER_HOME is caches-only and must NOT be consulted here either --
  // server and fake CLI must resolve the SAME home.
  if (process.env.FRESHELL_AMPLIFIER_HOME) return process.env.FRESHELL_AMPLIFIER_HOME
  const home = process.env.HOME || process.env.USERPROFILE || '.'
  return path.join(home, '.amplifier')
}

if (argv[0] === 'resume') {
  const sessionId = argv[1] ?? ''
  process.stdout.write(`amplifier: resumed session ${sessionId}\r\n`)
  // Mirror the real CLI's first-turn save: find the (broker pre-created)
  // session dir under any project slug, stamp turn_count into metadata.json
  // and append one transcript line -- the exact "used" signature the
  // broker's stub GC respects (used sessions survive terminal exit).
  let turnRecorded = false
  process.stdin.setEncoding('utf8')
  process.stdin.on('data', () => {
    if (turnRecorded) return
    turnRecorded = true
    const projectsDir = path.join(amplifierHome(), 'projects')
    let slugs = []
    try { slugs = fs.readdirSync(projectsDir) } catch { /* no home yet */ }
    for (const slug of slugs) {
      const dir = path.join(projectsDir, slug, 'sessions', sessionId)
      const metaPath = path.join(dir, 'metadata.json')
      if (!fs.existsSync(metaPath)) continue
      const meta = JSON.parse(fs.readFileSync(metaPath, 'utf8'))
      meta.turn_count = (meta.turn_count ?? 0) + 1
      fs.writeFileSync(metaPath, JSON.stringify(meta))
      fs.appendFileSync(path.join(dir, 'transcript.jsonl'), `${JSON.stringify({ role: 'user', content: 'fake turn' })}\n`)
      process.stdout.write(`amplifier: turn recorded ${sessionId}\r\n`)
      break
    }
  })
  process.stdin.resume()
} else {
  process.stdout.write('amplifier> \r\n')

  let sessionCreated = false
  process.stdin.setEncoding('utf8')
  process.stdin.on('data', () => {
    // Any input at all counts as "the first submit" for this fixture's
    // purposes -- the pty's own cooked-mode line discipline already
    // withholds bytes from this process until the user presses Enter, so
    // the first `data` event this process ever sees IS that submit.
    if (sessionCreated) return
    sessionCreated = true

    const cwd = process.cwd()
    const slug = slugify(cwd)
    const sessionId = `fake-amp-${Date.now()}-${process.pid}`
    const sessionDir = path.join(amplifierHome(), 'projects', slug, 'sessions', sessionId)
    fs.mkdirSync(sessionDir, { recursive: true })
    const lines = [
      JSON.stringify({ event: 'session:start' }),
      JSON.stringify({ event: 'session:config', working_dir: cwd }),
    ]
    fs.writeFileSync(path.join(sessionDir, 'events.jsonl'), `${lines.join('\n')}\n`)

    process.stdout.write(`amplifier: session ${sessionId} started\r\n`)
  })
  process.stdin.resume()
}
