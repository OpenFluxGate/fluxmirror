import { Link } from 'react-router-dom'

export function NotFound() {
  return (
    <div className="space-y-3">
      <h1 className="text-xl font-medium">404</h1>
      <p className="text-sm text-[var(--color-muted)]">
        That page does not exist yet — Phase 3 wires the routes one
        milestone at a time.
      </p>
      <Link to="/" className="text-sm">
        ← back home
      </Link>
    </div>
  )
}
