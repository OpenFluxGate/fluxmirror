// Session — single-session detail page.
//
// Pulls `/api/session/:id`, renders the heuristic name, lifecycle,
// summary stats, dominant cwd, top files, tool mix, and the per-event
// timeline. The timeline is intentionally minimal — sessions are a
// "what did this run look like" tool, not full provenance (that's
// the file page).

import { useMemo, useState } from 'react'
import { Link, useParams } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'

import { getSession, type Session, type SessionEvent } from '../lib/api'
import { StatTile } from '../components/StatTile'
import { LifecycleBadge, formatDuration } from './Sessions'

const FIRST_PAINT_LIMIT = 100

export function SessionDetail() {
  const params = useParams()
  const id = params.id ?? ''

  const session = useQuery({
    queryKey: ['session', id],
    queryFn: () => getSession(id),
    enabled: id.length > 0,
    retry: 0,
  })

  const [shown, setShown] = useState(FIRST_PAINT_LIMIT)

  if (!id) {
    return (
      <p className="text-sm text-[var(--color-redact)]">
        no session id supplied — open one from the{' '}
        <Link to="/sessions" className="text-[var(--color-accent)]">
          sessions
        </Link>{' '}
        list.
      </p>
    )
  }
  if (session.isPending) {
    return <p className="text-sm text-[var(--color-muted)]">loading…</p>
  }
  if (session.isError || !session.data) {
    return (
      <p className="text-sm text-[var(--color-redact)]">
        failed to load /api/session: {String(session.error)}
      </p>
    )
  }

  const data: Session = session.data
  const visible = data.events.slice(0, shown)
  const dayGroups = groupEventsByLocalDate(visible)

  return (
    <div className="space-y-8">
      <header className="space-y-2 border-b border-[var(--color-border)] pb-4">
        <p className="text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
          session · {data.id}
        </p>
        <h1 className="font-mono text-base text-[var(--color-text)] break-all">
          {data.name}
        </h1>
        {data.intent && (
          <p className="text-sm italic text-[var(--color-muted)]">
            {data.intent}
          </p>
        )}
        <div className="flex flex-wrap items-center gap-3 text-xs text-[var(--color-muted)]">
          <LifecycleBadge lifecycle={data.lifecycle} />
          <span>
            {formatLocalShort(data.start)} → {formatLocalShort(data.end)}
          </span>
          <span>·</span>
          <span>{formatDuration(data.start, data.end)}</span>
          <span>·</span>
          <span>{data.event_count.toLocaleString()} events</span>
          <span>·</span>
          <span>{data.agents.join(', ')}</span>
        </div>
      </header>

      <section className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <StatTile label="events" value={data.event_count.toLocaleString()} />
        <StatTile
          label="duration"
          value={formatDuration(data.start, data.end)}
        />
        <StatTile label="agents" value={data.agents.length} />
        <StatTile
          label="top files"
          value={data.top_files.length || '—'}
        />
      </section>

      {data.dominant_cwd && (
        <section>
          <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-2">
            dominant cwd
          </h2>
          <p className="font-mono text-sm break-all">{data.dominant_cwd}</p>
        </section>
      )}

      {data.top_files.length > 0 && (
        <section>
          <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
            top files
          </h2>
          <ul className="space-y-1 text-sm font-mono">
            {data.top_files.map((p) => (
              <li key={p}>
                <Link
                  to={`/file/${p}`}
                  className="text-[var(--color-accent)] hover:underline break-all"
                >
                  {p}
                </Link>
              </li>
            ))}
          </ul>
        </section>
      )}

      {data.tool_mix.length > 0 && (
        <section>
          <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
            tool mix
          </h2>
          <div className="overflow-hidden rounded border border-[var(--color-border)]">
            <table className="w-full border-collapse text-sm">
              <thead className="bg-[var(--color-panel)] text-[var(--color-muted)]">
                <tr>
                  <th className="px-3 py-2 text-left font-normal">tool</th>
                  <th className="px-3 py-2 text-right font-normal">count</th>
                </tr>
              </thead>
              <tbody>
                {data.tool_mix.map((t) => (
                  <tr
                    key={t.tool}
                    className="border-t border-[var(--color-border)]"
                  >
                    <td className="px-3 py-1.5 font-mono">{t.tool}</td>
                    <td className="px-3 py-1.5 text-right tabular-nums">
                      {t.count.toLocaleString()}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      )}

      <section>
        <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
          timeline
        </h2>
        {data.events.length === 0 ? (
          <p className="text-xs text-[var(--color-muted)]">
            no events on this session.
          </p>
        ) : (
          <div className="space-y-6">
            {dayGroups.map((group) => (
              <DayGroup key={group.dateLabel} group={group} />
            ))}
            {shown < data.events.length && (
              <div>
                <button
                  type="button"
                  onClick={() =>
                    setShown((n) =>
                      Math.min(n + FIRST_PAINT_LIMIT, data.events.length),
                    )
                  }
                  className="text-xs text-[var(--color-accent)] hover:underline"
                >
                  load more ({data.events.length - shown} remaining)
                </button>
              </div>
            )}
          </div>
        )}
      </section>
    </div>
  )
}

interface EventGroup {
  dateLabel: string
  events: SessionEvent[]
}

function groupEventsByLocalDate(events: SessionEvent[]): EventGroup[] {
  const buckets = new Map<string, SessionEvent[]>()
  for (const ev of events) {
    const key = localDateKey(ev.ts)
    const arr = buckets.get(key)
    if (arr) {
      arr.push(ev)
    } else {
      buckets.set(key, [ev])
    }
  }
  return Array.from(buckets.entries()).map(([dateLabel, evs]) => ({
    dateLabel,
    events: evs,
  }))
}

function DayGroup({ group }: { group: EventGroup }) {
  return (
    <div>
      <h3 className="font-mono text-xs uppercase tracking-wider text-[var(--color-muted)] mb-2">
        {group.dateLabel}
      </h3>
      <ol className="space-y-2">
        {group.events.map((ev, i) => (
          <li key={`${ev.ts}-${i}`}>
            <EventRow ev={ev} />
          </li>
        ))}
      </ol>
    </div>
  )
}

function EventRow({ ev }: { ev: SessionEvent }) {
  const detail = useMemo(() => truncate(ev.detail ?? '', 120), [ev.detail])
  return (
    <article className="rounded border border-[var(--color-border)] bg-[var(--color-panel)] px-3 py-2 text-sm">
      <div className="flex flex-wrap items-baseline gap-3">
        <span className="font-mono tabular-nums text-[var(--color-muted)]">
          {formatLocalTime(ev.ts)}
        </span>
        <span className="rounded bg-[var(--color-bg)] px-2 py-0.5 font-mono text-xs text-[var(--color-text)]">
          {ev.tool || '—'}
        </span>
        <span className="text-xs text-[var(--color-muted)]">{ev.agent}</span>
      </div>
      <p className="mt-1 font-mono text-xs break-all">{detail || '—'}</p>
    </article>
  )
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

function formatLocalShort(ts: string): string {
  if (!ts) return ''
  try {
    const d = new Date(ts)
    if (Number.isNaN(d.getTime())) return ts
    return d.toLocaleString()
  } catch {
    return ts
  }
}

function formatLocalTime(ts: string): string {
  if (!ts) return ''
  try {
    const d = new Date(ts)
    if (Number.isNaN(d.getTime())) return ts
    return d.toLocaleTimeString()
  } catch {
    return ts
  }
}

function truncate(s: string, max: number): string {
  if (s.length <= max) return s
  return `${s.slice(0, max - 1)}…`
}
