import { describe, it, expect } from 'vitest'
import { parseResumeInput, MAX_RESUME_CANDIDATES } from '@shared/resume-input-parser'

describe('parseResumeInput — token extraction', () => {
  const cases: Array<{ name: string; input: string; expected: string[] }> = [
    {
      name: 'full UUID',
      input: 'ed2afda6-a340-443e-ba60-024a1b3554b4',
      expected: ['ed2afda6-a340-443e-ba60-024a1b3554b4'],
    },
    {
      name: 'opencode ses_ id (26 base62)',
      input: 'ses_root0000000000000000000000',
      expected: ['ses_root0000000000000000000000'],
    },
    {
      name: 'short hex prefix with a digit',
      input: '417e8345',
      expected: ['417e8345'],
    },
    {
      name: 'codex resume command line',
      input: 'codex resume 019fac27-69d7-78a0-b972-b339d551042e',
      expected: ['019fac27-69d7-78a0-b972-b339d551042e'],
    },
    {
      name: 'claude -r short flag',
      input: 'claude -r ed2afda6-a340-443e-ba60-024a1b3554b4',
      expected: ['ed2afda6-a340-443e-ba60-024a1b3554b4'],
    },
    {
      name: 'opencode --session command',
      input: 'opencode --session ses_root0000000000000000000000',
      expected: ['ses_root0000000000000000000000'],
    },
    {
      name: 'quoted + shell prompt + whitespace',
      input: '  $ "claude --resume ed2afda6-a340-443e-ba60-024a1b3554b4"  ',
      expected: ['ed2afda6-a340-443e-ba60-024a1b3554b4'],
    },
    {
      name: 'backticks and trailing punctuation',
      input: 'try `amplifier --resume 417e8345`, it works.',
      expected: ['417e8345'],
    },
    {
      name: 'truncated UUID pasted with trailing dash and ellipsis',
      input: '"claude --resume ed2afda6-…"',
      expected: ['ed2afda6'],
    },
    {
      name: 'id embedded in a path',
      input: '/home/u/.claude/projects/x/ed2afda6-a340-443e-ba60-024a1b3554b4.jsonl',
      expected: ['ed2afda6-a340-443e-ba60-024a1b3554b4'],
    },
    {
      name: 'multi-line paste with ANSI codes',
      input: '\u001b[32m> session started\u001b[0m\nresume with:\n  codex resume 019fac27-69d7-78a0-b972-b339d551042e\n',
      expected: ['019fac27-69d7-78a0-b972-b339d551042e'],
    },
    {
      name: 'multiple candidates: prefixed id and UUID before hex, hex longest-first',
      input: 'ses_root0000000000000000000000 ed2afda6-a340-443e-ba60-024a1b3554b4 417e8345 417e8345abcd',
      expected: [
        'ses_root0000000000000000000000',
        'ed2afda6-a340-443e-ba60-024a1b3554b4',
        '417e8345abcd',
        '417e8345',
      ],
    },
  ]

  for (const c of cases) {
    it(c.name, () => {
      expect(parseResumeInput(c.input).candidates).toEqual(c.expected)
    })
  }

  const rejects: Array<{ name: string; input: string }> = [
    { name: 'hex-looking English word without a digit', input: 'decade facade' },
    { name: 'all-letter hex ≥8 chars without a digit', input: 'deadbeefcafebabe'.replace(/[0-9]/g, 'a') },
    { name: 'short hex (<8 chars)', input: '417e83' },
    { name: 'snake_case identifier is not an id family', input: 'snake_casedword my_function_name' },
    { name: 'plain prose', input: 'please resume my last session thanks' },
    { name: 'empty string', input: '' },
    { name: 'flags only', input: 'claude --resume --verbose' },
  ]

  for (const c of rejects) {
    it(`rejects: ${c.name}`, () => {
      expect(parseResumeInput(c.input).candidates).toEqual([])
    })
  }

  it('dedupes hex/UUID tokens case-insensitively, keeping first casing', () => {
    expect(parseResumeInput('417E8345 417e8345').candidates).toEqual(['417E8345'])
  })

  it('keeps case-distinct ses_ ids separate (base62 is case-SENSITIVE)', () => {
    const a = 'ses_root0000000000000000000000'
    const b = 'ses_ROOT0000000000000000000000'
    expect(parseResumeInput(`${a} ${b}`).candidates).toEqual([a, b])
  })

  it('caps candidates at MAX_RESUME_CANDIDATES (server work budget)', () => {
    const tokens = Array.from({ length: MAX_RESUME_CANDIDATES + 4 }, (_, i) => `417e83450a${String(i).padStart(2, '0')}`)
    const { candidates } = parseResumeInput(tokens.join(' '))
    expect(candidates).toHaveLength(MAX_RESUME_CANDIDATES)
  })
})

describe('parseResumeInput — agent hints (advisory only)', () => {
  it('command shape beats id-format: codex resume <v7 uuid>', () => {
    const r = parseResumeInput('codex resume 019fac27-69d7-78a0-b972-b339d551042e')
    expect(r.agentHint).toEqual({ provider: 'codex', source: 'command' })
  })
  it('claude --resume command', () => {
    const r = parseResumeInput('claude --resume ed2afda6-a340-443e-ba60-024a1b3554b4')
    expect(r.agentHint).toEqual({ provider: 'claude', source: 'command' })
  })
  it('opencode --session command', () => {
    const r = parseResumeInput('opencode --session ses_root0000000000000000000000')
    expect(r.agentHint).toEqual({ provider: 'opencode', source: 'command' })
  })
  it('amplifier --resume command', () => {
    const r = parseResumeInput('amplifier --resume 417e8345')
    expect(r.agentHint).toEqual({ provider: 'amplifier', source: 'command' })
  })
  it('bare agent word', () => {
    const r = parseResumeInput('from my codex run: ed2afda6-a340-443e-ba60-024a1b3554b4')
    expect(r.agentHint).toEqual({ provider: 'codex', source: 'word' })
  })
  it('id-format: ses_ suggests opencode', () => {
    expect(parseResumeInput('ses_root0000000000000000000000').agentHint)
      .toEqual({ provider: 'opencode', source: 'id-format' })
  })
  it('id-format: UUIDv7 suggests codex', () => {
    expect(parseResumeInput('019fac27-69d7-78a0-b972-b339d551042e').agentHint)
      .toEqual({ provider: 'codex', source: 'id-format' })
  })
  it('id-format: UUIDv4 suggests claude', () => {
    expect(parseResumeInput('ed2afda6-a340-443e-ba60-024a1b3554b4').agentHint)
      .toEqual({ provider: 'claude', source: 'id-format' })
  })
  it('id-format: bare short hex suggests amplifier', () => {
    expect(parseResumeInput('417e8345').agentHint)
      .toEqual({ provider: 'amplifier', source: 'id-format' })
  })
  it('no token, no hint', () => {
    expect(parseResumeInput('nothing here').agentHint).toBeUndefined()
  })
})
