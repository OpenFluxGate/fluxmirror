// HeatmapBar — fixed-width bar chart used by both Today (24-hour
// strip) and Week (per-day daily totals). The caller passes the raw
// counts; the component computes the local max and renders one bar
// per row. Empty rows still render so the layout stays a fixed grid.

interface HeatmapBarProps {
  rows: { label: string; count: number }[]
  /// Width of the longest bar in pixels at scale 1. Defaults to 240px
  /// — wide enough to read the count alongside without the bar wrapping.
  maxWidth?: number
  /// Used in screen-reader output. Defaults to `Activity bar chart`.
  ariaLabel?: string
}

export function HeatmapBar({
  rows,
  maxWidth = 240,
  ariaLabel = 'Activity bar chart',
}: HeatmapBarProps) {
  const max = rows.reduce((m, r) => Math.max(m, r.count), 0)
  if (rows.length === 0) {
    return (
      <p className="text-xs text-[var(--color-muted)]">no data in window</p>
    )
  }

  return (
    <ol
      aria-label={ariaLabel}
      className="space-y-1 font-mono text-xs text-[var(--color-text)]"
    >
      {rows.map((row, i) => {
        const ratio = max === 0 ? 0 : row.count / max
        // Always paint at least 2px when count > 0 so a single outlier
        // doesn't render as zero width and disappear visually.
        const width = row.count === 0 ? 0 : Math.max(2, ratio * maxWidth)
        return (
          <li key={`${row.label}-${i}`} className="flex items-center gap-3">
            <span className="w-16 shrink-0 text-[var(--color-muted)]">
              {row.label}
            </span>
            <span
              className="h-3 rounded-sm bg-[var(--color-accent)]"
              style={{ width: `${width}px` }}
              aria-hidden
            />
            <span className="text-right tabular-nums">{row.count}</span>
          </li>
        )
      })}
    </ol>
  )
}
