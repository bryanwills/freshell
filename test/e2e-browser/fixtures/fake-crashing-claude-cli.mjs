#!/usr/bin/env node
// test/e2e-browser/fixtures/fake-crashing-claude-cli.mjs
// Fake claude CLI for crash-resilience e2e. Behavior selection:
//   FAKE_CRASH_UNTIL=N — crash (exit 1) while invocation <= N, then SURVIVE.
//                        Takes PRECEDENCE over FAKE_CRASH_MODE: when set, the
//                        mode checks are never reached, so the 'clean' default
//                        cannot make the surviving invocation exit 0.
//   FAKE_CRASH_MODE (only when FAKE_CRASH_UNTIL is unset):
//     once   — invocation #1 prints output then exits 1; later invocations stay alive
//     always — every invocation prints then exits 1 immediately
//     clean  — prints then exits 0 (the default when neither env is set)
// Every invocation appends {pid,t,argv} to FAKE_CLAUDE_ARGV_LOG (JSONL) and
// bumps the invocation counter in FAKE_CRASH_STATE_FILE.
import fs from 'node:fs'

const argv = process.argv.slice(2)
const logPath = process.env.FAKE_CLAUDE_ARGV_LOG
if (logPath) fs.appendFileSync(logPath, JSON.stringify({ pid: process.pid, t: Date.now(), argv }) + '\n')

let invocation = 1
const stateFile = process.env.FAKE_CRASH_STATE_FILE
if (stateFile) {
  try { invocation = (parseInt(fs.readFileSync(stateFile, 'utf8'), 10) || 0) + 1 } catch { /* first run */ }
  fs.writeFileSync(stateFile, String(invocation))
}

process.stdout.write(`fake-claude invocation ${invocation} argv=${argv.join(' ')}\r\n`)

const crashUntil = Number(process.env.FAKE_CRASH_UNTIL || 0)
if (crashUntil > 0) {
  if (invocation <= crashUntil) {
    process.stdout.write('fake-claude: simulated crash\r\n')
    process.exit(1)
  }
  // invocation > N: fall through to the survive branch below WITHOUT
  // consulting FAKE_CRASH_MODE (its 'clean' default would exit 0 and
  // vacuously satisfy liveness assertions on a dead pane).
} else {
  const mode = process.env.FAKE_CRASH_MODE || 'clean'
  if (mode === 'always' || (mode === 'once' && invocation === 1)) {
    process.stdout.write('fake-claude: simulated crash\r\n')
    process.exit(1)
  }
  if (mode === 'clean') {
    process.stdout.write('fake-claude: clean exit\r\n')
    process.exit(0)
  }
}
// Survive: behave like a long-running interactive CLI.
process.stdin.resume()
setInterval(() => {}, 60_000)
