// CostBlock — per-agent cost breakdown table sitting under the top-line
// cost StatTile on the Today and Week pages.
//
// The top-line figure ($1.23 +/- estimate footnote) lives in the parent
// page's StatTile strip. This component surfaces the per-agent split so
// a reader can see WHERE the dollars went — typically MCP traffic
// (claude-desktop) versus heuristic-estimated agent activity.

import type { CostSummary } from '../lib/api'

interface Props {
  cost: CostSummary
}

const fmtUsd = (n: number): string => {
  if (Math.abs(n) < 0.01) return n.toFixed(4)
  if (Math.abs(n) < 100) return n.toFixed(2)
  return Math.round(n).toLocaleString()
}

const fmtInt = (n: number): string => n.toLocaleString()

export function CostBlock({ cost }: Props) {
  if (cost.by_agent.length === 0 && cost.total_usd === 0) {
    return null
  }
  return (
    <div className="overflow-x-auto rounded border border-[var(--color-border)]">
      <table className="w-full border-collapse text-sm">
        <thead className="bg-[var(--color-panel)] text-[var(--color-muted)]">
          <tr>
            <th className="px-3 py-2 text-left font-normal">agent</th>
            <th className="px-3 py-2 text-right font-normal">tokens in</th>
            <th className="px-3 py-2 text-right font-normal">tokens out</th>
            <th className="px-3 py-2 text-right font-normal">USD</th>
          </tr>
        </thead>
        <tbody>
          {cost.by_agent.map((row) => (
            <tr
              key={row.agent}
              className="border-t border-[var(--color-border)]"
            >
              <td className="px-3 py-1.5">{row.agent}</td>
              <td className="px-3 py-1.5 text-right font-mono tabular-nums">
                {fmtInt(row.tokens_in)}
              </td>
              <td className="px-3 py-1.5 text-right font-mono tabular-nums">
                {fmtInt(row.tokens_out)}
              </td>
              <td className="px-3 py-1.5 text-right font-mono tabular-nums">
                ${fmtUsd(row.usd)}
                {row.estimate && (
                  <span
                    className="ml-1 text-[var(--color-muted)]"
                    title="estimated from non-MCP agent activity"
                  >
                    *
                  </span>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

export function formatCostUsd(usd: number): string {
  return `$${fmtUsd(usd)}`
}

export function formatEstimateHint(estimateShare: number): string | undefined {
  if (estimateShare <= 0) return undefined
  const pct = Math.round(estimateShare * 100)
  return `${pct}% estimated`
}
