# Tab Icon Caps (3 repo + 3 pane) Implementation Plan

> **For agentic workers:** This plan is executed task-by-task by the
> workflow's execute stage: a fresh implementer per task, with a spec +
> quality review after each task. Steps use checkbox (`- [ ]`) syntax
> for tracking.

**Goal:** Cap each icon group on a tab independently at 3 — at most 3 repo
icons and at most 3 pane-type icons per tab, keeping the existing `+N`
overflow badge semantics for pane icons and silently truncating repo icons.

**Architecture:** All cap logic lives in exactly one place:
`renderIcons()` in `src/components/TabItem.tsx` (verified by exploration —
see "Parity surfaces" below). The change replaces the single
`MAX_TAB_ICONS = 6` constant with `MAX_PANE_ICONS = 3` (pane icons + `+N`
badge, semantics otherwise unchanged) and `MAX_REPO_ICONS = 3` (explicit,
silent truncation of distinct repo icons). Repo icons continue to derive
from the *visible* pane slice (today's semantics: panes hidden beyond the
pane cap never contribute a repo icon), so with a pane cap of 3 the repo
cap is structurally guaranteed; the explicit `MAX_REPO_ICONS` guard is
added as the refactor step so the locked "each group capped independently
at 3" decision survives any future change to `MAX_PANE_ICONS`.

**Tech Stack:** React 18 + TypeScript, vitest (jsdom) via the repo's
coordinated test runner, Testing Library, eslint + jsx-a11y.

## Locked design decisions (from the user)

- Per tab: at most **3 repo icons** and at most **3 pane-type icons**,
  capped independently.
- Pane icons keep the existing `+N` overflow badge (5 panes → 3 icons +
  `+2`). Badge markup/coloring (`text-blue-500` when a hidden pane is busy,
  else `text-muted-foreground`) is unchanged.
- Repo icons beyond 3: **silent truncation** (judgment call made here: a
  second `+N` badge would be ambiguous next to the pane badge and crowd the
  tab; the spec explicitly allows silent truncation). Deterministic order:
  first-appearance order over the visible pane slice — exactly how repo
  icons are ordered today.
- Result: a tab shows at most 3 repo icons + 3 pane icons + one `+N` badge.

## Global Constraints

- Work only in the worktree `/home/dan/code/freshell/.worktrees/tab-icon-caps`
  (branch `feat/tab-icon-caps`, branched from `origin/main`).
- Do NOT open a PR, do NOT merge, do NOT restart or deploy any server.
- No server, protocol, or settings changes. No Rust changes.
- TDD (Red-Green-Refactor) per repo rules (`AGENTS.md`).
- jsx-a11y lint is a CI gate: `npm run lint` must be clean of new warnings.
- Tests run ONLY via coordinated commands: `npm run test:vitest -- run <files>
  --config config/vitest/vitest.config.ts` for targeted runs,
  `npm run test:unit` for the broad run. Never raw `npx vitest`; never pass
  `--config` to public commands (`npm run test:unit` etc. reject it).
- Run all commands from the worktree root (the vitest config excludes
  `**/.worktrees/**`, so running from the main repo root would skip these
  tests).
- README.md is the only end-user markdown doc — create no new docs beyond
  this plan.
- Commits are focused and atomic, conventional-commit style.

## Parity surfaces (verified — no changes needed)

Exploration confirmed `MAX_TAB_ICONS` exists ONLY in
`src/components/TabItem.tsx` (lines 27, 85, 86, 88) and no other component
shares or duplicates the cap logic:

- `src/components/MobileTabStrip.tsx` — no icon logic at all (text "Busy"
  pill only). **Do not touch.**
- `src/components/TabSwitcher.tsx` — no icon logic at all. **Do not touch.**
- `src/components/TabsView.tsx:693` — a *local variable* named `PaneIcon`
  from its own private `paneKindIcon` mapper; unrelated icon system, no
  cap. **Do not touch.**
- `src/components/panes/PaneHeader.tsx` — renders exactly one RepoIcon +
  one PaneIcon per pane header; a cap is not applicable. **Do not touch.**

Test-suite audit (re-confirmed by an independent validation sweep of all
pane-icon/repo-icon testid consumers, all `+N` text assertions, and all
loop-built >=4-pane fixtures) confirmed exactly two tests pin the old cap
of 6 and must change in lockstep with the constant:
`test/unit/client/components/TabItem.test.tsx:240-260` and
`test/unit/client/components/TabBar.test.tsx:1392-1447`. Nothing else in
`test/` (including `test/e2e*`) renders >3 panes in one tab while asserting
icon counts or `+N` text. There are no snapshot tests anywhere under `test/`.

**Anchor trust note:** all line anchors in this plan were re-verified
against HEAD `0a6657f1`. If any anchor does not match what you see, locate
the target by symbol/test name (quoted in each step), never by line number
alone, and proceed.

**Verified baseline (recorded at `0a6657f1`, do not re-litigate):**
`npm run typecheck:client` exits 0; `npm run test:unit` passes 4144/4144
(353 files); `npm run lint` exits 0 with 0 errors and exactly **9
pre-existing warnings** (all `react-hooks/exhaustive-deps`, in 8 files,
**none** in `TabItem.tsx`). Two known noise sources, neither attributable
to this change: (a) vitest runs with `sequence.shuffle: true`, and (b)
`test/unit/client/store/storage-migration.fresh-agent.test.ts` has a
<500ms wall-clock perf budget that can fail spuriously under machine load
— if either produces an unexpected failure unrelated to tab icons, re-run
once before investigating.

**E2E coverage note:** this change is pure presentation logic inside
`TabItem`, already covered end-to-end by the existing
`test/e2e/repo-icon-tab-flow.test.tsx` flow (single-pane, count-agnostic —
verified unaffected by the new cap). Unit tests at the `TabItem` and
`TabBar` (Redux-integrated) levels are the highest meaningful abstraction
for this cap; no new e2e file is warranted for a constant change.

---

### Task 1: Lower the pane-icon cap from 6 to 3 (keep `+N` badge semantics)

**Files:**
- Modify: `src/components/TabItem.tsx:27` (constant) and `:85-88` (usages)
- Test: `test/unit/client/components/TabItem.test.tsx` (modify the test at
  `:240-260`, add three tests after it)
- Test: `test/unit/client/components/TabBar.test.tsx` (modify the test
  `'caps at 6 icons and shows overflow indicator'` at `:1392-1447`)

**Interfaces:**
- Consumes: existing `TabItem` props (`paneEntries`, `busyPaneIds`,
  `iconsOnTabs`) and the test fixtures already in each file:
  `createPaneEntries(contents: PaneContent[])` (TabItem.test.tsx:46-51,
  1-based `pane-N` ids) and the mocked `data-testid="pane-icon"` /
  `getByText('+N')` selectors.
- Produces: module-private `const MAX_PANE_ICONS = 3` in
  `src/components/TabItem.tsx` (replaces `MAX_TAB_ICONS`; stays
  unexported — all existing cap tests hardcode numbers, keep that
  convention). Task 2 relies on `renderIcons()` still grouping the visible
  slice into `groups: { key, info?, entries }[]` exactly as today.

- [ ] **Step 1: Update the existing hidden-busy overflow test to the new cap**

In `test/unit/client/components/TabItem.test.tsx`, the test at `:240-260`
(`'shows blue overflow indicator when the exact busy terminal is hidden
beyond the visible icon cap'`) builds 7 panes with `pane-7` busy and asserts
`getByText('+1')`. With a cap of 3, 7 panes overflow by 4. Change only the
assertion line:

```tsx
    expect(screen.getByText('+4').getAttribute('class')).toContain('text-blue-500')
```

- [ ] **Step 2: Add three new failing cap tests to TabItem.test.tsx**

Insert immediately after that test (still inside the top-level
`describe('TabItem', ...)`, before the click-handling tests):

```tsx
  it('caps pane icons at 3 and shows a muted +N badge for the rest', () => {
    const paneContents: PaneContent[] = Array.from({ length: 5 }, (_, index) => ({
      kind: 'terminal',
      mode: 'shell',
      shell: 'system',
      status: 'running',
      createRequestId: `req-${index + 1}`,
      terminalId: `term-${index + 1}`,
    }))

    render(<TabItem {...defaultProps} paneEntries={createPaneEntries(paneContents)} />)

    expect(screen.getAllByTestId('pane-icon')).toHaveLength(3)
    const badge = screen.getByText('+2')
    expect(badge.getAttribute('class')).toContain('text-muted-foreground')
    expect(badge.getAttribute('class')).not.toContain('text-blue-500')
  })

  it('shows 3 icons plus +1 at 4 panes (cap boundary)', () => {
    const paneContents: PaneContent[] = Array.from({ length: 4 }, (_, index) => ({
      kind: 'terminal',
      mode: 'shell',
      shell: 'system',
      status: 'running',
      createRequestId: `req-${index + 1}`,
      terminalId: `term-${index + 1}`,
    }))

    render(<TabItem {...defaultProps} paneEntries={createPaneEntries(paneContents)} />)

    expect(screen.getAllByTestId('pane-icon')).toHaveLength(3)
    expect(screen.getByText('+1')).toBeInTheDocument()
  })

  it('shows no overflow badge at exactly 3 panes', () => {
    const paneContents: PaneContent[] = Array.from({ length: 3 }, (_, index) => ({
      kind: 'terminal',
      mode: 'shell',
      shell: 'system',
      status: 'running',
      createRequestId: `req-${index + 1}`,
      terminalId: `term-${index + 1}`,
    }))

    render(<TabItem {...defaultProps} paneEntries={createPaneEntries(paneContents)} />)

    expect(screen.getAllByTestId('pane-icon')).toHaveLength(3)
    expect(screen.queryByText(/^\+\d+$/)).toBeNull()
  })
```

(Note: `getByText` matches only an element's *direct* text nodes, so the
regex matches only the badge span, never its flex-container parents — the
same reason the existing `getByText('+1')` test works.)

- [ ] **Step 3: Update the TabBar integration cap test**

In `test/unit/client/components/TabBar.test.tsx`, the test
`'caps at 6 icons and shows overflow indicator'` (`:1392-1447`) builds a
7-pane nested layout and asserts 6 icons + `+1`. Keep the 7-pane fixture;
change the title and the two assertions at the end:

```tsx
    it('caps at 3 icons and shows overflow indicator', () => {
```

```tsx
      // Should show 3 icons + overflow indicator
      const icons = screen.getAllByTestId('pane-icon')
      expect(icons).toHaveLength(3)

      // Overflow indicator shows +4
      expect(screen.getByText('+4')).toBeInTheDocument()
```

- [ ] **Step 4: Run both test files to verify RED**

Run (from the worktree root):

```bash
npm run test:vitest -- run \
  test/unit/client/components/TabItem.test.tsx \
  test/unit/client/components/TabBar.test.tsx \
  --config config/vitest/vitest.config.ts
```

Expected: FAIL with exactly 4 failing tests —
1. TabItem `'shows blue overflow indicator...'` — unable to find `'+4'`
   (currently renders `+1`).
2. TabItem `'caps pane icons at 3...'` — expected length 3, received 5.
3. TabItem `'shows 3 icons plus +1 at 4 panes...'` — expected length 3,
   received 4.
4. TabBar `'caps at 3 icons and shows overflow indicator'` — expected
   length 3, received 6.

`'shows no overflow badge at exactly 3 panes'` PASSES already (3 ≤ 6) —
it is a boundary guard, expected green in the red run.

(Both files are verified green at baseline — 83/83 — so all 4 failures are
attributable to the new expectations. Vitest shuffles test order per run;
if an *unrelated* test fails intermittently, re-run once before
investigating. If the failure set differs from the 4 above in a way that
involves icon counts or `+N` text, STOP and re-audit before proceeding.)

- [ ] **Step 5: Implement — rename the constant and set it to 3**

In `src/components/TabItem.tsx`, replace line 27:

```tsx
const MAX_TAB_ICONS = 6
```

with:

```tsx
/** Max pane-type icons shown per tab; panes beyond this fold into the '+N' badge. */
const MAX_PANE_ICONS = 3
```

and update the three usages inside `renderIcons()` (currently lines 85-88):

```tsx
    const visible = paneEntries.slice(0, MAX_PANE_ICONS)
    const overflow = paneEntries.length - MAX_PANE_ICONS
    const hiddenBusyPane = paneEntries
      .slice(MAX_PANE_ICONS)
      .some((entry) => busyPaneIds.includes(entry.paneId))
```

No other production change in this task. `MAX_TAB_ICONS` must no longer
appear anywhere in `src/`.

- [ ] **Step 6: Run both test files to verify GREEN**

```bash
npm run test:vitest -- run \
  test/unit/client/components/TabItem.test.tsx \
  test/unit/client/components/TabBar.test.tsx \
  --config config/vitest/vitest.config.ts
```

Expected: PASS (all tests in both files, including all pre-existing ones —
the audit found no other test in these files rendering >3 panes in one tab).

- [ ] **Step 7: Commit**

```bash
git add src/components/TabItem.tsx \
  test/unit/client/components/TabItem.test.tsx \
  test/unit/client/components/TabBar.test.tsx
git commit -m "feat(tabs): cap pane-type tab icons at 3 with +N overflow badge"
```

---

### Task 2: Pin and make explicit the repo-icon cap of 3 (silent truncation)

**Files:**
- Modify: `src/components/TabItem.tsx` (add `MAX_REPO_ICONS`; guard the
  RepoIcon render inside `renderIcons()`)
- Test: `test/unit/client/components/TabItem.test.tsx` (add three tests to
  the existing `describe('repo icons')` block at `:391-456`)

**Interfaces:**
- Consumes: from Task 1, `MAX_PANE_ICONS = 3` and the unchanged group
  structure in `renderIcons()` (`groups: Array<{ key: string; info?:
  RepoIconInfo; entries }>` built in first-appearance order over the
  visible slice, RepoIcon rendered via `{group.info && <RepoIcon ... />}`).
  From the test file's `repo icons` describe block: the `codingContent(cwd)`
  and `entries(cwds)` helpers (0-based `pane-N` ids, attach `repoCwd`) and
  the `repoIcons` fixture map with keys `/repo/a` and `/repo/b`.
- Produces: module-private `const MAX_REPO_ICONS = 3` in
  `src/components/TabItem.tsx` and a `repoIconKeys: Set<string>` guard in
  `renderIcons()`. Nothing outside `TabItem.tsx` consumes these.

**TDD honesty note:** with `MAX_PANE_ICONS = 3`, repo icons (derived from
the ≤3 visible panes) can never exceed 3, so the three tests below are
*characterization* tests — they are written first and expected to PASS
against Task 1's code. They pin the locked decision so any future change
(e.g. raising `MAX_PANE_ICONS`) that would leak >3 repo icons fails
loudly. Step 3 then makes the cap explicit as the REFACTOR phase, with the
suite staying green. Do not skip the tests just because they start green.

This structure complies with the repo rule (`AGENTS.md:7`: "We use
Red-Green-Refactor TDD for all changes but the most trivial... We never
skip the tests, and never skip the refactor."): adding a guard for an
invariant that already structurally holds is a *refactor*, refactors are
definitionally performed green, and characterization-before-refactor is
the canonical safe-refactor technique. The genuine RED driver for this
task family is Task 1 Step 4. Prior repo plans use the same pre-argued
green-task pattern. If a reviewer nonetheless rejects the green-first
structure, the sanctioned fallback is: fold this task's steps into Task 1
as an extension of its refactor phase (same tests, same guard, one task)
— do not drop the tests or the guard.

**Open interpretation note (recorded decision):** repo icons derive from
the *visible* pane slice — a repo whose panes are ALL hidden beyond the
pane cap shows no repo icon (a 4-repo tab can show as few as 1 repo
icon). This is today's verified behavior and this plan deliberately
preserves it (the goal statement says "keeping existing... semantics").
The alternative reading — "first 3 distinct repos across ALL panes" —
was considered and rejected as an unrequested behavior change. If the
user later wants the alternative, it is a contained `renderIcons()`
change (derive repo groups from all `paneEntries` instead of `visible`)
plus updating the test
`'does not render repo icons for repos whose panes are all hidden beyond the pane cap'`,
which is a characterization test of current behavior, pending
confirmation — not a spec.

- [ ] **Step 1: Add repo-icon cap tests to the `describe('repo icons')` block**

In `test/unit/client/components/TabItem.test.tsx`, inside
`describe('repo icons', ...)` (the existing `repoIcons` fixture is at
`:395`; add after the last existing test in the block), first add an
extended fixture next to the existing `repoIcons` const:

```tsx
    const manyRepoIcons = {
      ...repoIcons,
      '/repo/c': { repoKey: '/repo/c', repoName: 'c' },
      '/repo/d': { repoKey: '/repo/d', repoName: 'd' },
    }
```

then add these three tests at the end of the block:

```tsx
    it('shows at most 3 repo icons when a tab spans more than 3 distinct repos', () => {
      render(
        <TabItem
          {...defaultProps}
          paneEntries={entries(['/repo/a', '/repo/b', '/repo/c', '/repo/d'])}
          repoIcons={manyRepoIcons}
        />,
      )
      const repoIconsRendered = screen.getAllByTestId('repo-icon')
      expect(repoIconsRendered).toHaveLength(3)
      // Deterministic: the first 3 distinct repos in pane order.
      expect(repoIconsRendered.map((el) => el.getAttribute('data-repo-key'))).toEqual([
        '/repo/a',
        '/repo/b',
        '/repo/c',
      ])
      expect(screen.getAllByTestId('pane-icon')).toHaveLength(3)
      // Repo truncation is silent: the only badge is the pane-overflow +1.
      expect(screen.getAllByText(/^\+\d+$/)).toHaveLength(1)
      expect(screen.getByText('+1')).toBeInTheDocument()
    })

    it('does not render repo icons for repos whose panes are all hidden beyond the pane cap', () => {
      render(
        <TabItem
          {...defaultProps}
          paneEntries={entries(['/repo/a', '/repo/a', '/repo/a', '/repo/b'])}
          repoIcons={manyRepoIcons}
        />,
      )
      const repoIconsRendered = screen.getAllByTestId('repo-icon')
      expect(repoIconsRendered).toHaveLength(1)
      expect(repoIconsRendered[0].getAttribute('data-repo-key')).toBe('/repo/a')
      expect(screen.getAllByTestId('pane-icon')).toHaveLength(3)
      expect(screen.getByText('+1')).toBeInTheDocument()
    })

    it('shows all 3 repo icons with no badge when exactly 3 panes span 3 repos', () => {
      render(
        <TabItem
          {...defaultProps}
          paneEntries={entries(['/repo/a', '/repo/b', '/repo/c'])}
          repoIcons={manyRepoIcons}
        />,
      )
      expect(screen.getAllByTestId('repo-icon')).toHaveLength(3)
      expect(screen.getAllByTestId('pane-icon')).toHaveLength(3)
      expect(screen.queryByText(/^\+\d+$/)).toBeNull()
    })
```

(Selector note for the silent-truncation test: `getAllByText(/^\+\d+$/)`
is expected to match exactly the one badge span — Testing Library matches
direct text nodes only. If it ever yields ≠1 match, scope the query with
`within(...)` on the icon row or match on the badge's class instead — a
test-local fix, NOT a production bug.)

- [ ] **Step 2: Run the test file — expect all three new tests to PASS**

```bash
npm run test:vitest -- run test/unit/client/components/TabItem.test.tsx \
  --config config/vitest/vitest.config.ts
```

Expected: PASS (whole file). These characterization tests confirm the
repo-icon bound already holds after Task 1. If any of them FAILS, stop and
re-examine Task 1 before proceeding.

- [ ] **Step 3: REFACTOR — make the repo cap explicit in TabItem.tsx**

In `src/components/TabItem.tsx`, add directly below `MAX_PANE_ICONS`:

```tsx
/**
 * Max distinct repo icons shown per tab (locked decision: each icon group
 * is capped independently at 3). Repos beyond the cap are silently
 * truncated -- the '+N' badge counts hidden panes only.
 */
const MAX_REPO_ICONS = 3
```

Inside `renderIcons()`, after the `groups` loop finishes (currently right
before the `return (`), add:

```tsx
    // Cap distinct repo icons independently of the pane cap. The visible
    // slice (<= MAX_PANE_ICONS entries) cannot yield more repo groups than
    // that today; this guard keeps the repo-icon bound at 3 even if
    // MAX_PANE_ICONS changes.
    const repoIconKeys = new Set(
      groups
        .filter((group) => group.info)
        .slice(0, MAX_REPO_ICONS)
        .map((group) => group.key),
    )
```

and change the RepoIcon render line inside the JSX from:

```tsx
            {group.info && <RepoIcon info={group.info} className="h-3 w-3 shrink-0" />}
```

to:

```tsx
            {group.info && repoIconKeys.has(group.key) && (
              <RepoIcon info={group.info} className="h-3 w-3 shrink-0" />
            )}
```

- [ ] **Step 4: Run both test files to verify still GREEN**

```bash
npm run test:vitest -- run \
  test/unit/client/components/TabItem.test.tsx \
  test/unit/client/components/TabBar.test.tsx \
  --config config/vitest/vitest.config.ts
```

Expected: PASS (all tests in both files).

- [ ] **Step 5: Commit**

```bash
git add src/components/TabItem.tsx test/unit/client/components/TabItem.test.tsx
git commit -m "feat(tabs): cap repo icons per tab at 3 with silent truncation"
```

---

### Task 3: Full verification (coordinated suite, typecheck, lint)

**Files:**
- No new files; fixes only if a check fails (keep any fix atomic and
  committed with an explanatory message).

**Interfaces:**
- Consumes: the committed work from Tasks 1-2.
- Produces: a verified-green branch `feat/tab-icon-caps` — the deliverable
  of this whole plan. Do NOT open a PR, merge, or deploy.

- [ ] **Step 1: Typecheck the client**

```bash
npm run typecheck:client
```

Expected: exits 0, no errors.

- [ ] **Step 2: Run the coordinated unit suite**

```bash
FRESHELL_TEST_SUMMARY="tab-icon-caps: verify pane/repo icon caps" npm run test:unit
```

Expected: PASS, 0 failures — the recorded baseline is 4144/4144 across 353
files. (This is a broad coordinated run — if another agent holds the
coordinator gate, wait; never kill a foreign holder.) Known baseline noise:
`test/unit/client/store/storage-migration.fresh-agent.test.ts` has a
<500ms perf budget that can fail spuriously under machine load, and vitest
shuffles test order per run — if the ONLY failure is that perf test or an
intermittent failure clearly unrelated to tab icons, re-run once before
attributing it to this change.

- [ ] **Step 3: Lint (jsx-a11y CI gate)**

```bash
npm run lint
```

Expected: exit 0 with 0 errors. NOTE: eslint's exit code ignores warnings,
so "no new warnings" must be checked against the recorded baseline: exactly
**9 pre-existing warnings**, all `react-hooks/exhaustive-deps`, in 8 files,
**none** in `TabItem.tsx`. The gate is: total warnings still 9, and no
warning mentions `TabItem.tsx` or any file this change touched (the change
adds no interactive elements or roles, so nothing new should appear). If a
new warning points at the modified lines, fix it (`npm run lint:fix` for
mechanical fixes), re-run Steps 1-3, and commit the fix:

```bash
git add -A && git commit -m "fix(tabs): resolve lint findings in tab icon cap change"
```

- [ ] **Step 4: Confirm the branch is clean and self-contained**

```bash
git status --short && git log --oneline origin/main..HEAD
```

Expected: empty status (no uncommitted files); commit list shows the plan
doc commit plus the Task 1 and Task 2 commits (and any lint-fix commit).
Confirm no changes exist outside `src/components/TabItem.tsx`,
`test/unit/client/components/TabItem.test.tsx`,
`test/unit/client/components/TabBar.test.tsx`, and
`docs/plans/2026-07-28-tab-icon-caps.md`. Stop here — no PR, no merge,
no deploy.
