#!/usr/bin/env node
// Fake `claude` terminal CLI for e2e. Mirrors fake-codex-cli.mjs: appends
// {pid,t,argv} JSONL to FAKE_CLAUDE_ARGV_LOG on every invocation, prints a
// greppable marker, then stays "running" via stdin.resume().
//
// Real claude launch shapes (extensions/claude-code/freshell.json):
//   fresh:  claude ... --session-id <uuid>   (pre-allocated at t=0 by the
//           WS/picker create path ONLY, crates/freshell-ws/src/terminal.rs:969-982;
//           REST POST /api/tabs never mints one, terminal_tabs.rs:756-768)
//   resume: claude ... --resume <id>
// Flags are searched anywhere in argv (resume args are appended LAST by the
// launch builder), matching fake-codex-cli.mjs's rationale.
import fs from 'node:fs'
import path from 'node:path'

const argv = process.argv.slice(2)

function appendArgvLog() {
  const logPath = process.env.FAKE_CLAUDE_ARGV_LOG
  if (!logPath) return
  fs.mkdirSync(path.dirname(logPath), { recursive: true })
  fs.appendFileSync(logPath, `${JSON.stringify({ pid: process.pid, t: Date.now(), argv })}\n`)
}
appendArgvLog()

const resumeIdx = argv.indexOf('--resume')
const startIdx = argv.indexOf('--session-id')

if (resumeIdx !== -1) {
  process.stdout.write(`claude: resumed session ${argv[resumeIdx + 1] ?? ''}\r\n`)
} else if (startIdx !== -1) {
  process.stdout.write(`claude: session ${argv[startIdx + 1] ?? ''} started\r\n`)
} else {
  process.stdout.write('claude> \r\n')
}
process.stdin.resume()
