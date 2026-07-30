import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { configureStore } from '@reduxjs/toolkit'
import Sidebar from '@/components/Sidebar'
import tabsReducer from '@/store/tabsSlice'
import panesReducer from '@/store/panesSlice'
import settingsReducer from '@/store/settingsSlice'
import connectionReducer from '@/store/connectionSlice'
import sessionsReducer from '@/store/sessionsSlice'
import sessionActivityReducer from '@/store/sessionActivitySlice'

vi.mock('@/lib/ws-client', () => ({
  getWsClient: () => ({
    send: vi.fn(),
    onMessage: vi.fn(() => () => {}),
    connect: vi.fn().mockResolvedValue(undefined),
  }),
}))

function makeStore() {
  return configureStore({
    reducer: {
      tabs: tabsReducer,
      panes: panesReducer,
      settings: settingsReducer,
      connection: connectionReducer,
      sessions: sessionsReducer,
      sessionActivity: sessionActivityReducer,
    },
  })
}

function renderSidebar(props: Partial<React.ComponentProps<typeof Sidebar>> = {}) {
  return render(
    <Provider store={makeStore()}>
      <Sidebar view="terminal" onNavigate={vi.fn()} {...props} />
    </Provider>,
  )
}

beforeEach(() => vi.clearAllMocks())
afterEach(() => cleanup())

describe('Sidebar pinned resume footer', () => {
  it('pins the footer: IMMEDIATE next sibling of the flex-1 min-h-0 scroll wrapper, outside it, non-shrinking', () => {
    // jsdom cannot do layout, so this asserts the EXACT pinning mechanism the
    // spec mandates instead of faking scroll events: the scroll wrapper is the
    // `flex flex-1 min-h-0` div that CONTAINS the session list; the footer is
    // that wrapper's IMMEDIATE next sibling inside the same flex-column parent
    // and carries flex-shrink-0 so it can never be scrolled away or squeezed
    // out. Any placement that violates the spec (inside the list, deeper in the
    // tree, or after other siblings) fails one of these assertions.
    renderSidebar()
    const footer = screen.getByTestId('sidebar-footer')
    const list = screen.getByTestId('sidebar-session-list')
    const wrapper = footer.previousElementSibling as HTMLElement | null
    expect(wrapper).not.toBeNull()
    expect(wrapper!.className).toContain('flex-1')
    expect(wrapper!.className).toContain('min-h-0')
    expect(wrapper!.contains(list)).toBe(true)
    expect(wrapper!.contains(footer)).toBe(false)
    expect(footer.parentElement).toBe(wrapper!.parentElement)
    expect(footer.className).toContain('flex-shrink-0')
  })

  it('renders in fullWidth mobile mode', () => {
    renderSidebar({ fullWidth: true })
    expect(screen.getByTestId('sidebar-resume-button')).toBeInTheDocument()
  })

  it('button is keyboard accessible with an accessible name and opens the dialog', () => {
    renderSidebar()
    const button = screen.getByRole('button', { name: /resume a session/i })
    expect(button).toHaveAttribute('data-testid', 'sidebar-resume-button')
    fireEvent.click(button)
    expect(screen.getByRole('dialog', { name: /resume a session/i })).toBeInTheDocument()
  })
})
