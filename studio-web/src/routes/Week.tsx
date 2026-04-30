// Week — 7-day rolling rollup. Mirrors the Phase 2 weekly HTML card:
// summary tile, daily breakdown bar chart, top files / shells, agent
// breakdown, tool mix, plus a 7×24 day-of-week heatmap.

import { useQuery } from '@tanstack/react-query'

import { fetchWeek, type WeekData } from '../lib/api'
import { AgentBar } from '../components/AgentBar'
import {
  CostBlock,
  formatCostUsd,
  formatEstimateHint,
} from '../components/CostBlock'
import { FileTable } from '../components/FileTable'
import { HeatmapBar } from '../components/HeatmapBar'
import { StatTile } from '../components/StatTile'
import { ToolMix } from '../components/ToolMix'

const DOW_LABELS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']

export function Week() {
  const week = useQuery({ queryKey: ['week'], queryFn: fetchWeek })

  if (week.isPending) {
    return <p className="text-sm text-[var(--color-muted)]">loading…</p>
  }
  if (week.isError || !week.data) {
    return (
      <p className="text-sm text-[var(--color-redact)]">
        failed to load /api/week: {String(week.error)}
      </p>
    )
  }

  const data: WeekData = week.data
  if (data.total_events === 0) {
    return (
      <section className="space-y-3">
        <h1 className="text-2xl font-medium text-[var(--color-text)]">
          Last 7 Days
        </h1>
        <p className="text-sm text-[var(--color-muted)]">
          {data.range_start} ~ {data.range_end} ({data.tz}) · no activity in
          window yet.
        </p>
      </section>
    )
  }

  const activeDays = data.daily.filter((d) => d.calls > 0).length
  const heatmapMax = data.heatmap.reduce(
    (m, row) => row.reduce((rm, n) => Math.max(rm, n), m),
    0,
  )

  return (
    <div className="space-y-8">
      <header className="space-y-1">
        <h1 className="text-2xl font-medium text-[var(--color-text)]">
          Last 7 Days
        </h1>
        <p className="text-xs text-[var(--color-muted)]">
          {data.range_start} ~ {data.range_end} · {data.tz}
        </p>
      </header>

      <section className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <StatTile
          label="total calls"
          value={data.total_events.toLocaleString()}
        />
        <StatTile label="agents" value={data.agents.length} />
        <StatTile label="active days" value={`${activeDays}/7`} />
        {data.cost ? (
          <StatTile
            label="cost (estimate)"
            value={formatCostUsd(data.cost.total_usd)}
            hint={formatEstimateHint(data.cost.estimate_share)}
          />
        ) : (
          <StatTile
            label="mcp traffic"
            value={data.mcp_count.toLocaleString()}
          />
        )}
      </section>

      {data.cost && (
        <section>
          <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
            cost breakdown
          </h2>
          <CostBlock cost={data.cost} />
        </section>
      )}

      <section>
        <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
          daily breakdown
        </h2>
        <HeatmapBar
          rows={data.daily.map((d) => ({ label: d.date.slice(5), count: d.calls }))}
          ariaLabel="Calls per day"
        />
      </section>

      <section>
        <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
          day-of-week × hour heatmap
        </h2>
        <Heatmap heatmap={data.heatmap} max={heatmapMax} />
      </section>

      <section>
        <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
          agents
        </h2>
        <AgentBar agents={data.agents} />
      </section>

      <section className="grid gap-6 md:grid-cols-2">
        <div>
          <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
            top edited files
          </h2>
          <FileTable
            rows={data.files_edited.map((f) => ({
              path: f.path,
              tool: f.tool,
              count: f.count,
            }))}
            limit={30}
            showTool
          />
        </div>
        <div>
          <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
            top read files
          </h2>
          <FileTable
            rows={data.files_read.map((f) => ({ path: f.path, count: f.count }))}
            limit={15}
          />
        </div>
      </section>

      <section className="grid gap-6 md:grid-cols-2">
        <div>
          <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
            tool mix
          </h2>
          <ToolMix entries={data.tool_mix} limit={20} />
        </div>
        {data.shell_counts.length > 0 && (
          <div>
            <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
              top shell commands
            </h2>
            <FileTable
              rows={data.shell_counts.map((s) => ({
                path: s.path,
                count: s.count,
              }))}
              limit={10}
              linkify={false}
            />
          </div>
        )}
      </section>

      {data.cwds.length > 0 && (
        <section>
          <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
            working directories
          </h2>
          <FileTable
            rows={data.cwds.map((c) => ({ path: c.path, count: c.count }))}
            limit={10}
            linkify={false}
          />
        </section>
      )}
    </div>
  )
}

interface HeatmapProps {
  heatmap: number[][]
  max: number
}

function Heatmap({ heatmap, max }: HeatmapProps) {
  if (max === 0) {
    return (
      <p className="text-xs text-[var(--color-muted)]">no activity in window</p>
    )
  }
  const cellSize = 14
  return (
    <div className="overflow-x-auto">
      <div className="inline-block">
        <div
          className="grid gap-px"
          style={{ gridTemplateColumns: `auto repeat(24, ${cellSize}px)` }}
          role="img"
          aria-label="Calls by day-of-week and hour"
        >
          <div />
          {Array.from({ length: 24 }, (_, h) => (
            <div
              key={`h-${h}`}
              className="text-[10px] text-[var(--color-muted)] text-center font-mono"
            >
              {h % 6 === 0 ? String(h).padStart(2, '0') : ''}
            </div>
          ))}
          {heatmap.map((row, dow) => (
            <RowFragment key={`row-${dow}`} dow={dow} row={row} max={max} cellSize={cellSize} />
          ))}
        </div>
      </div>
    </div>
  )
}

function RowFragment({
  dow,
  row,
  max,
  cellSize,
}: {
  dow: number
  row: number[]
  max: number
  cellSize: number
}) {
  return (
    <>
      <div className="pr-2 text-[10px] text-[var(--color-muted)] text-right font-mono leading-none flex items-center justify-end">
        {DOW_LABELS[dow] ?? ''}
      </div>
      {row.map((n, h) => {
        const ratio = max === 0 ? 0 : n / max
        const opacity = n === 0 ? 0 : 0.15 + ratio * 0.85
        return (
          <div
            key={`c-${dow}-${h}`}
            title={`${DOW_LABELS[dow]} ${String(h).padStart(2, '0')}:00 · ${n}`}
            style={{
              width: `${cellSize}px`,
              height: `${cellSize}px`,
              backgroundColor: `rgb(2 132 199 / ${opacity})`,
              border: '1px solid var(--color-border)',
            }}
          />
        )
      })}
    </>
  )
}
