// Sessions — heuristic work-session list.
//
// Pulls `/api/sessions?from=…&to=…` and renders one row per session,
// grouped by the local start date. Each row links into the detail
// page at `/session/:id`. Default trailing window is 7 days; the
// dropdown lets the reader widen it without typing dates.

import { useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'

import { getSessions, type Session, type SessionLifecycle } from '../lib/api'

const RANGE_OPTIONS: ReadonlyArray<{ label: string; days: number }> = [
  { label: '24h', days: 1 },
  { label: '7d', days: 7 },
  { label: '14d', days: 14 },
  { label: '30d', days: 30 },
]

export function Sessions() {
  const [days, setDays] = useState<number>(7)

  const { from, to } = useMemo(() => buildRange(days), [days])

  const sessions = useQuery({
    queryKey: ['sessions', from, to],
    queryFn: () => getSessions(from, to),
  })

  return (
    <div className="space-y-6">
      <header className="flex items-baseline justify-between">
        <div>
          <p className="text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
            work sessions
          </p>
          <h1 className="mt-1 text-2xl text-[var(--color-text)] font-medium">
            Sessions
          </h1>
          <p className="mt-1 text-xs text-[var(--color-muted)]">
            Auto-named from event clusters · 30-min gap split · purely
            heuristic.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <label className="text-xs text-[var(--color-muted)]">range</label>
          <select
            value={days}
            onChange={(e) => setDays(Number(e.target.value))}
            className="rounded border border-[var(--color-border)] bg-[var(--color-panel)] px-2 py-1 text-xs"
          >
            {RANGE_OPTIONS.map((o) => (
              <option key={o.days} value={o.days}>
                {o.label}
              </option>
            ))}
          </select>
        </div>
      </header>

      {sessions.isPending && (
        <p className="text-xs text-[var(--color-muted)]">loading…</p>
      )}
      {sessions.isError && (
        <p className="text-sm text-[var(--color-redact)]">
          failed to load /api/sessions: {String(sessions.error)}
        </p>
      )}
      {sessions.data && sessions.data.length === 0 && (
        <p className="text-xs text-[var(--color-muted)]">
          no sessions in the selected window. Run any agent for a few
          minutes and come back.
        </p>
      )}
      {sessions.data && sessions.data.length > 0 && (
        <SessionGroups sessions={sessions.data} />
      )}
    </div>
  )
}

function SessionGroups({ sessions }: { sessions: Session[] }) {
  const groups = useMemo(() => groupByLocalDate(sessions), [sessions])
  return (
    <div className="space-y-8">
      {groups.map((group) => (
        <section key={group.dateLabel}>
          <h2 className="font-mono text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
            {group.dateLabel}
          </h2>
          <div className="overflow-hidden rounded border border-[var(--color-border)]">
            <table className="w-full border-collapse text-sm">
              <thead className="bg-[var(--color-panel)] text-[var(--color-muted)]">
                <tr>
                  <th className="px-3 py-2 text-left font-normal">name</th>
                  <th className="px-3 py-2 text-left font-normal">when</th>
                  <th className="px-3 py-2 text-right font-normal">
                    duration
                  </th>
                  <th className="px-3 py-2 text-right font-normal">events</th>
                  <th className="px-3 py-2 text-left font-normal">agents</th>
                  <th className="px-3 py-2 text-left font-normal">
                    lifecycle
                  </th>
                </tr>
              </thead>
              <tbody>
                {group.sessions.map((s) => (
                  <SessionRow key={s.id} session={s} />
                ))}
              </tbody>
            </table>
          </div>
        </section>
      ))}
    </div>
  )
}

function SessionRow({ session }: { session: Session }) {
  return (
    <tr className="border-t border-[var(--color-border)] hover:bg-[var(--color-panel)]">
      <td className="px-3 py-2">
        <Link
          to={`/session/${session.id}`}
          className="font-mono text-[var(--color-accent)] hover:underline"
        >
          {session.name}
        </Link>
        {session.intent && (
          <p className="mt-0.5 text-xs italic text-[var(--color-muted)]">
            {session.intent}
          </p>
        )}
      </td>
      <td className="px-3 py-2 font-mono tabular-nums text-[var(--color-muted)]">
        {formatLocalTimeRange(session.start, session.end)}
      </td>
      <td className="px-3 py-2 text-right font-mono tabular-nums">
        {formatDuration(session.start, session.end)}
      </td>
      <td className="px-3 py-2 text-right tabular-nums">
        {session.event_count.toLocaleString()}
      </td>
      <td className="px-3 py-2 font-mono text-xs">
        {session.agents.join(', ')}
      </td>
      <td className="px-3 py-2">
        <LifecycleBadge lifecycle={session.lifecycle} />
      </td>
    </tr>
  )
}

export function LifecycleBadge({ lifecycle }: { lifecycle: SessionLifecycle }) {
  const styles = lifecycleStyle(lifecycle)
  return (
    <span
      className={`inline-flex rounded px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider ${styles}`}
    >
      {lifecycle}
    </span>
  )
}

function lifecycleStyle(lifecycle: SessionLifecycle): string {
  // Tailwind palette tokens stay neutral so the badge inherits the
  // dashboard's monochrome aesthetic. The "shipping" tag is the only
  // one that earns a brighter accent.
  switch (lifecycle) {
    case 'shipping':
      return 'bg-emerald-500/15 text-emerald-300 border border-emerald-500/30'
    case 'testing':
      return 'bg-sky-500/15 text-sky-300 border border-sky-500/30'
    case 'building':
      return 'bg-indigo-500/15 text-indigo-300 border border-indigo-500/30'
    case 'polishing':
      return 'bg-amber-500/15 text-amber-300 border border-amber-500/30'
    case 'investigating':
      return 'bg-violet-500/15 text-violet-300 border border-violet-500/30'
    case 'idle':
    default:
      return 'bg-[var(--color-bg)] text-[var(--color-muted)] border border-[var(--color-border)]'
  }
}

interface DateGroup {
  dateLabel: string
  sessions: Session[]
}

function groupByLocalDate(sessions: Session[]): DateGroup[] {
  const buckets = new Map<string, Session[]>()
  for (const s of sessions) {
    const key = localDateKey(s.start)
    const arr = buckets.get(key)
    if (arr) {
      arr.push(s)
    } else {
      buckets.set(key, [s])
    }
  }
  // Reverse-chronological — the most recent date sits at the top.
  const sorted = Array.from(buckets.entries()).sort(([a], [b]) =>
    a < b ? 1 : a > b ? -1 : 0,
  )
  return sorted.map(([dateLabel, sessions]) => ({ dateLabel, sessions }))
}

function localDateKey(ts: string): string {
  try {
    const d = new Date(ts)
    if (Number.isNaN(d.getTime())) return ts.slice(0, 10)
    const yyyy = d.getFullYear()
    const mm = String(d.getMonth() + 1).padStart(2, '0')
    const dd = String(d.getDate()).padStart(2, '0')
    return `${yyyy}-${mm}-${dd}`
  } catch {
    return ts.slice(0, 10)
  }
}

function formatLocalTimeRange(startIso: string, endIso: string): string {
  const start = new Date(startIso)
  const end = new Date(endIso)
  if (Number.isNaN(start.getTime()) || Number.isNaN(end.getTime())) {
    return `${startIso} → ${endIso}`
  }
  const fmt = (d: Date) =>
    `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
  return `${fmt(start)} – ${fmt(end)}`
}

export function formatDuration(startIso: string, endIso: string): string {
  const start = new Date(startIso)
  const end = new Date(endIso)
  if (Number.isNaN(start.getTime()) || Number.isNaN(end.getTime())) {
    return '—'
  }
  const ms = end.getTime() - start.getTime()
  if (ms <= 0) return '0m'
  const minutes = Math.round(ms / 60000)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  const rem = minutes % 60
  if (rem === 0) return `${hours}h`
  return `${hours}h ${rem}m`
}

function buildRange(days: number): { from: string; to: string } {
  const today = new Date()
  const tomorrow = new Date(today)
  tomorrow.setDate(today.getDate() + 1)
  const from = new Date(tomorrow)
  from.setDate(tomorrow.getDate() - days)
  return {
    from: ymd(from),
    to: ymd(tomorrow),
  }
}

function ymd(d: Date): string {
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}
