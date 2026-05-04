// Home — landing dashboard. Surfaces the latest event ("Now"), a
// today summary tile, this week's heatmap, and the most recent
// auto-named work sessions.

import { Link } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'

import { fetchNow, fetchToday, fetchWeek, getSessions } from '../lib/api'
import { HeatmapBar } from '../components/HeatmapBar'
import { StatTile } from '../components/StatTile'
import { LifecycleBadge, formatDuration } from './Sessions'

interface Health {
  status: string
  version: string
  db: string
  agent_events: number
  proxy_events: number
}

async function fetchHealth(): Promise<Health> {
  const res = await fetch('/health')
  if (!res.ok) throw new Error(`status ${res.status}`)
  return res.json()
}

// Trailing window the home dashboard polls for sessions. Matches the
// week heatmap above so the cards line up.
const HOME_SESSIONS_DAYS = 7
const HOME_SESSIONS_LIMIT = 5

export function Home() {
  const health = useQuery({ queryKey: ['health'], queryFn: fetchHealth })
  const now = useQuery({ queryKey: ['now'], queryFn: fetchNow })
  const today = useQuery({ queryKey: ['today'], queryFn: fetchToday })
  const week = useQuery({ queryKey: ['week'], queryFn: fetchWeek })
  const homeRange = buildHomeRange(HOME_SESSIONS_DAYS)
  const sessions = useQuery({
    queryKey: ['sessions', homeRange.from, homeRange.to],
    queryFn: () => getSessions(homeRange.from, homeRange.to),
  })

  return (
    <div className="space-y-8">
      <section>
        <h1 className="text-2xl text-[var(--color-text)] font-medium">
          Your AI coding, in one tab.
        </h1>
        <p className="mt-2 text-sm text-[var(--color-muted)]">
          Read-only dashboard over your local fluxmirror SQLite store.
        </p>
      </section>

      <section>
        <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
          now
        </h2>
        <div className="rounded border border-[var(--color-border)] bg-[var(--color-panel)] p-4 font-mono text-sm">
          {now.isPending && <span>loading…</span>}
          {now.isError && (
            <span className="text-[var(--color-redact)]">
              {String(now.error)}
            </span>
          )}
          {!now.isPending && !now.isError && now.data === null && (
            <span className="text-[var(--color-muted)]">
              no events captured yet — run any agent to populate.
            </span>
          )}
          {now.data && (
            <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1">
              <dt className="text-[var(--color-muted)]">latest</dt>
              <dd>{new Date(now.data.latest_ts_utc).toLocaleString()}</dd>
              <dt className="text-[var(--color-muted)]">agent</dt>
              <dd>{now.data.latest_agent}</dd>
              <dt className="text-[var(--color-muted)]">tool</dt>
              <dd>{now.data.latest_tool || '—'}</dd>
              <dt className="text-[var(--color-muted)]">detail</dt>
              <dd className="break-all">{now.data.latest_detail || '—'}</dd>
              <dt className="text-[var(--color-muted)]">cwd</dt>
              <dd className="break-all">{now.data.latest_cwd || '—'}</dd>
              <dt className="text-[var(--color-muted)]">last hour</dt>
              <dd>
                {now.data.last_hour_total.toLocaleString()} call(s) ·{' '}
                {now.data.last_hour_agents.length} agent(s)
              </dd>
            </dl>
          )}
        </div>
      </section>

      <section>
        <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
          today
        </h2>
        <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
          <StatTile
            label="total calls"
            value={today.data?.total_events.toLocaleString() ?? '—'}
            hint={today.data?.date}
          />
          <StatTile
            label="agents"
            value={today.data?.agents.length ?? '—'}
          />
          <StatTile
            label="files touched"
            value={today.data?.distinct_files.length.toLocaleString() ?? '—'}
          />
          <StatTile
            label="shell cmds"
            value={today.data?.shells.length ?? '—'}
          />
        </div>
        <p className="mt-3 text-xs text-[var(--color-muted)]">
          Open the{' '}
          <Link to="/today" className="text-[var(--color-accent)]">
            today
          </Link>{' '}
          page for the full report.
        </p>
      </section>

      <section>
        <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
          this week
        </h2>
        {week.data ? (
          <HeatmapBar
            rows={week.data.daily.map((d) => ({
              label: d.date.slice(5),
              count: d.calls,
            }))}
            ariaLabel="Calls per day this week"
          />
        ) : (
          <p className="text-xs text-[var(--color-muted)]">loading…</p>
        )}
        <p className="mt-3 text-xs text-[var(--color-muted)]">
          Open the{' '}
          <Link to="/week" className="text-[var(--color-accent)]">
            week
          </Link>{' '}
          page for the full rollup.
        </p>
      </section>

      <section>
        <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
          recent sessions
        </h2>
        {sessions.isPending && (
          <p className="text-xs text-[var(--color-muted)]">loading…</p>
        )}
        {sessions.isError && (
          <p className="text-xs text-[var(--color-redact)]">
            failed to load /api/sessions: {String(sessions.error)}
          </p>
        )}
        {sessions.data && sessions.data.length === 0 && (
          <p className="text-xs text-[var(--color-muted)]">
            no sessions in the trailing 7 days yet.
          </p>
        )}
        {sessions.data && sessions.data.length > 0 && (
          <ul className="space-y-2">
            {sessions.data.slice(-HOME_SESSIONS_LIMIT).reverse().map((s) => (
              <li
                key={s.id}
                className="rounded border border-[var(--color-border)] bg-[var(--color-panel)] px-3 py-2 text-sm"
              >
                <div className="flex flex-wrap items-baseline justify-between gap-2">
                  <Link
                    to={`/session/${s.id}`}
                    className="font-mono text-[var(--color-accent)] hover:underline break-all"
                  >
                    {s.name}
                  </Link>
                  <LifecycleBadge lifecycle={s.lifecycle} />
                </div>
                {s.intent && (
                  <p className="mt-0.5 text-xs italic text-[var(--color-muted)]">
                    {s.intent}
                  </p>
                )}
                <p className="mt-1 text-xs text-[var(--color-muted)] font-mono tabular-nums">
                  {new Date(s.start).toLocaleString()} ·{' '}
                  {formatDuration(s.start, s.end)} ·{' '}
                  {s.event_count.toLocaleString()} events ·{' '}
                  {s.agents.join(', ')}
                </p>
              </li>
            ))}
          </ul>
        )}
        <p className="mt-3 text-xs text-[var(--color-muted)]">
          Open the{' '}
          <Link to="/sessions" className="text-[var(--color-accent)]">
            sessions
          </Link>{' '}
          page for the full list.
        </p>
      </section>

      <section className="text-xs text-[var(--color-muted)]">
        {health.data && (
          <p>
            db: <code className="font-mono">{health.data.db}</code> ·{' '}
            {health.data.agent_events.toLocaleString()} agent events ·{' '}
            {health.data.proxy_events.toLocaleString()} proxy events
          </p>
        )}
      </section>
    </div>
  )
}

function buildHomeRange(days: number): { from: string; to: string } {
  const today = new Date()
  const tomorrow = new Date(today)
  tomorrow.setDate(today.getDate() + 1)
  const from = new Date(tomorrow)
  from.setDate(tomorrow.getDate() - days)
  return { from: ymd(from), to: ymd(tomorrow) }
}

function ymd(d: Date): string {
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}
