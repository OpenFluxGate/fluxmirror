// Today — one-page narrative of the current local-day's AI activity.
//
// Mirrors the CLI's `fluxmirror today` text report section-for-section
// but laid out for scannability in a browser: top stats strip, agent
// bars, hour heatmap, files, shells, tool mix.

import { useQuery } from '@tanstack/react-query'

import { fetchToday, type TodayData } from '../lib/api'
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

export function Today() {
  const today = useQuery({ queryKey: ['today'], queryFn: fetchToday })

  if (today.isPending) {
    return <p className="text-sm text-[var(--color-muted)]">loading…</p>
  }
  if (today.isError || !today.data) {
    return (
      <p className="text-sm text-[var(--color-redact)]">
        failed to load /api/today: {String(today.error)}
      </p>
    )
  }

  const data: TodayData = today.data

  if (data.total_events === 0) {
    return (
      <section className="space-y-3">
        <h1 className="text-2xl font-medium text-[var(--color-text)]">
          Today's Work
        </h1>
        <p className="text-sm text-[var(--color-muted)]">
          {data.date} ({data.tz}) · no activity in window yet.
        </p>
      </section>
    )
  }

  const sessions = data.agents.reduce((m, a) => Math.max(m, a.sessions.length), 0)
  const editRatio =
    data.reads_total > 0
      ? (data.writes_total / data.reads_total).toFixed(2)
      : '—'
  const hourRows = data.hours
    .filter((h) => h.count > 0)
    .map((h) => ({ label: `${String(h.hour).padStart(2, '0')}:00`, count: h.count }))

  return (
    <div className="space-y-8">
      <header className="space-y-1">
        <h1 className="text-2xl font-medium text-[var(--color-text)]">
          Today's Work
        </h1>
        <p className="text-xs text-[var(--color-muted)]">
          {data.date} · {data.tz}
        </p>
      </header>

      <section className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <StatTile
          label="total calls"
          value={data.total_events.toLocaleString()}
        />
        <StatTile label="sessions" value={sessions} />
        <StatTile
          label="files touched"
          value={data.distinct_files.length.toLocaleString()}
        />
        {data.cost ? (
          <StatTile
            label="cost (estimate)"
            value={formatCostUsd(data.cost.total_usd)}
            hint={formatEstimateHint(data.cost.estimate_share)}
          />
        ) : (
          <StatTile label="edit / read" value={editRatio} />
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
          agents
        </h2>
        <AgentBar agents={data.agents} />
      </section>

      <section>
        <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
          hour distribution
        </h2>
        <HeatmapBar rows={hourRows} ariaLabel="Calls per hour" />
      </section>

      <section className="grid gap-6 md:grid-cols-2">
        <div>
          <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
            files written or edited
          </h2>
          <FileTable
            rows={data.files_edited.map((f) => ({
              path: f.path,
              tool: f.tool,
              count: f.count,
            }))}
            limit={20}
            showTool
          />
        </div>
        <div>
          <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
            files only read
          </h2>
          <FileTable
            rows={data.files_read.map((f) => ({ path: f.path, count: f.count }))}
            limit={10}
          />
        </div>
      </section>

      {data.shells.length > 0 && (
        <section>
          <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
            shell commands
          </h2>
          <div className="overflow-x-auto rounded border border-[var(--color-border)]">
            <table className="w-full border-collapse text-sm">
              <thead className="bg-[var(--color-panel)] text-[var(--color-muted)]">
                <tr>
                  <th className="px-3 py-2 text-left font-normal">time</th>
                  <th className="px-3 py-2 text-left font-normal">command</th>
                </tr>
              </thead>
              <tbody>
                {data.shells.map((s, i) => (
                  <tr
                    key={`${s.ts_utc}-${i}`}
                    className="border-t border-[var(--color-border)]"
                  >
                    <td className="px-3 py-1.5 font-mono tabular-nums text-[var(--color-muted)]">
                      {s.time_local}
                    </td>
                    <td className="px-3 py-1.5 font-mono break-all">
                      {s.detail}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      )}

      <section className="grid gap-6 md:grid-cols-2">
        {data.cwds.length > 0 && (
          <div>
            <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
              working directories
            </h2>
            <FileTable
              rows={data.cwds.map((c) => ({ path: c.path, count: c.count }))}
              linkify={false}
            />
          </div>
        )}
        {data.tool_mix.length > 0 && (
          <div>
            <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
              tool mix
            </h2>
            <ToolMix entries={data.tool_mix} limit={20} />
          </div>
        )}
      </section>

      {data.mcp_methods.length > 0 && (
        <section>
          <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
            mcp traffic
          </h2>
          <FileTable
            rows={data.mcp_methods.map((m) => ({
              path: m.method,
              count: m.count,
            }))}
            linkify={false}
          />
        </section>
      )}
    </div>
  )
}
