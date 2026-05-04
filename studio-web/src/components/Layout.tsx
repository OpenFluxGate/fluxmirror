import { NavLink, Outlet } from 'react-router-dom'

const navItems: ReadonlyArray<{ to: string; label: string }> = [
  { to: '/', label: 'home' },
  { to: '/today', label: 'today' },
  { to: '/week', label: 'week' },
  { to: '/sessions', label: 'sessions' },
  { to: '/projects', label: 'projects' },
  { to: '/files', label: 'files' },
  { to: '/replay', label: 'replay' },
  { to: '/settings', label: 'settings' },
]

export function Layout() {
  return (
    <div className="min-h-screen flex flex-col">
      <header className="border-b border-[var(--color-border)] bg-[var(--color-panel)]">
        <div className="mx-auto max-w-6xl px-6 py-4 flex items-center justify-between">
          <div className="flex items-baseline gap-3">
            <span className="font-mono text-sm tracking-wider text-[var(--color-text)]">
              FLUXMIRROR
            </span>
            <span className="text-xs text-[var(--color-muted)]">studio</span>
          </div>
          <nav className="flex gap-5 text-xs text-[var(--color-muted)]">
            {navItems.map((item) => (
              <NavLink
                key={item.to}
                to={item.to}
                end={item.to === '/'}
                className={({ isActive }) =>
                  isActive
                    ? 'text-[var(--color-text)]'
                    : 'hover:text-[var(--color-text)]'
                }
              >
                {item.label}
              </NavLink>
            ))}
          </nav>
        </div>
      </header>
      <main className="flex-1">
        <div className="mx-auto max-w-6xl px-6 py-8">
          <Outlet />
        </div>
      </main>
      <footer className="border-t border-[var(--color-border)] mt-auto">
        <div className="mx-auto max-w-6xl px-6 py-3 text-xs text-[var(--color-muted)] flex justify-between">
          <span>localhost · read-only</span>
          <span className="font-mono">fluxmirror-studio</span>
        </div>
      </footer>
    </div>
  )
}
