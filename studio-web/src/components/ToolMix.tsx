// ToolMix — horizontal bar list for the tool-mix breakdown.
//
// One row per tool, sorted desc by count. Width scales to the local
// max so the heaviest tool always reaches the right edge of the bar
// area; smaller tools render proportionally.

import type { ToolMixEntry } from '../lib/api'

interface ToolMixProps {
  entries: ToolMixEntry[]
  limit?: number
}

export function ToolMix({ entries, limit }: ToolMixProps) {
  const visible = limit ? entries.slice(0, limit) : entries
  if (visible.length === 0) {
    return (
      <p className="text-xs text-[var(--color-muted)]">no tool calls in window</p>
    )
  }
  const max = visible.reduce((m, t) => Math.max(m, t.count), 0)
  return (
    <ol className="space-y-1.5 font-mono text-xs">
      {visible.map((t) => {
        const ratio = max === 0 ? 0 : t.count / max
        const width = t.count === 0 ? 0 : Math.max(2, ratio * 240)
        return (
          <li key={t.tool} className="flex items-center gap-3">
            <span className="w-32 shrink-0 truncate text-[var(--color-text)]">
              {t.tool}
            </span>
            <span
              className="h-2.5 rounded-sm bg-[var(--color-mono)]"
              style={{ width: `${width}px` }}
              aria-hidden
            />
            <span className="text-right tabular-nums text-[var(--color-muted)]">
              {t.count.toLocaleString()}
            </span>
          </li>
        )
      })}
    </ol>
  )
}
