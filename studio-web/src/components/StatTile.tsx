// StatTile — labelled number tile for the top-of-page summary strip.
// Designed to read at a glance: small uppercase label, large mono
// numeral. Renders a hint line under the value when present.

interface StatTileProps {
  label: string
  value: string | number
  hint?: string
}

export function StatTile({ label, value, hint }: StatTileProps) {
  return (
    <div className="rounded border border-[var(--color-border)] bg-[var(--color-panel)] px-4 py-3">
      <div className="text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
        {label}
      </div>
      <div className="mt-1 font-mono text-2xl text-[var(--color-text)]">
        {value}
      </div>
      {hint && (
        <div className="mt-1 text-xs text-[var(--color-muted)]">{hint}</div>
      )}
    </div>
  )
}
