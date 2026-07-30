// @vitest-environment node
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import fsp from 'fs/promises'
import os from 'os'
import path from 'path'
import { runOpencodeSessionByIdQuery } from '../../../../server/coding-cli/providers/opencode-by-id-query'

const SES_ROOT = 'ses_root0000000000000000000000'
const SES_CHILD = 'ses_child000000000000000000000'

let dir: string
let dbPath: string

beforeEach(async () => {
  // Throwaway tmp DB — never the user's real opencode data dir (session safety rule).
  dir = await fsp.mkdtemp(path.join(os.tmpdir(), 'opencode-byid-'))
  dbPath = path.join(dir, 'opencode.db')
  const { DatabaseSync } = await import('node:sqlite')
  const db = new DatabaseSync(dbPath)
  db.exec(`
    CREATE TABLE project (id TEXT PRIMARY KEY, worktree TEXT);
    CREATE TABLE session (
      id TEXT PRIMARY KEY,
      project_id TEXT,
      parent_id TEXT,
      directory TEXT,
      title TEXT,
      time_created INTEGER,
      time_updated INTEGER,
      time_archived INTEGER
    );
    INSERT INTO project (id, worktree) VALUES ('p1', '/home/u/oc-proj');
    INSERT INTO session (id, project_id, parent_id, directory, title, time_created, time_updated, time_archived)
      VALUES ('${SES_ROOT}', 'p1', NULL, '/home/u/oc-proj', 'root session', 100, 200, NULL);
    INSERT INTO session (id, project_id, parent_id, directory, title, time_created, time_updated, time_archived)
      VALUES ('${SES_CHILD}', 'p1', '${SES_ROOT}', '/home/u/oc-proj', 'child session', 110, 210, 999);
  `)
  db.close()
})

afterEach(async () => {
  await fsp.rm(dir, { recursive: true, force: true })
})

describe('runOpencodeSessionByIdQuery', () => {
  it('finds a root session by exact id with metadata', async () => {
    const row = await runOpencodeSessionByIdQuery(dbPath, SES_ROOT)
    expect(row).toMatchObject({
      sessionId: SES_ROOT,
      cwd: '/home/u/oc-proj',
      title: 'root session',
      lastActivityAt: 200,
      projectPath: '/home/u/oc-proj',
    })
  })

  it('finds CHILD and ARCHIVED sessions too (unlike the listing query)', async () => {
    const row = await runOpencodeSessionByIdQuery(dbPath, SES_CHILD)
    expect(row?.sessionId).toBe(SES_CHILD)
  })

  it('returns null for an unknown id', async () => {
    expect(await runOpencodeSessionByIdQuery(dbPath, 'ses_missing0000000000000000000')).toBeNull()
  })

  it('works when the project table is absent (degraded schema)', async () => {
    const bare = path.join(dir, 'bare.db')
    const { DatabaseSync } = await import('node:sqlite')
    const db = new DatabaseSync(bare)
    db.exec(`
      CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT, title TEXT, time_created INTEGER, time_updated INTEGER);
      INSERT INTO session VALUES ('${SES_ROOT}', '/d', 't', 1, 2);
    `)
    db.close()
    const row = await runOpencodeSessionByIdQuery(bare, SES_ROOT)
    expect(row?.sessionId).toBe(SES_ROOT)
    expect(row?.projectPath ?? null).toBeNull()
  })
})
