// File — per-file provenance timeline.
//
// Renders every `agent_events` row whose `detail` matches the path
// captured from the URL, grouped by local date with each touch
// expandable to show the ±5 minute context window. Optional commit
// history column piggybacks on `/api/file/git`.

import { useMemo, useState } from 'react'
import { useParams } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'

import {
  getFile,
  getFileGit,
  type ContextEvent,
  type ProvenanceData,
  type ProvenanceEvent,
} from '../lib/api'
import { StatTile } from '../components/StatTile'

const FIRST_PAINT_LIMIT = 50

export function File() {
  const params = useParams()
  // react-router's `*` param captures the rest of the URL after
  // `/file/`. The link helpers `encodeURIComponent` the path before
  // navigating, but `useParams` returns it already decoded — perfect
  // for round-tripping into the API call.
  const rawPath = params['*'] ?? ''

  const provenance = useQuery({
    queryKey: ['file', rawPath],
    queryFn: () => getFile(rawPath),
    enabled: rawPath.length > 0,
  })
  const git = useQuery({
    queryKey: ['file', rawPath, 'git'],
    queryFn: () => getFileGit(rawPath),
    enabled: rawPath.length > 0,
    retry: 0,
  })

  const [expanded, setExpanded] = useState(FIRST_PAINT_LIMIT)

  if (!rawPath) {
    return (
      <p className="text-sm text-[var(--color-redact)]">
        no file path supplied — open a file from the today, week, or home pages.
      </p>
    )
  }
  if (provenance.isPending) {
    return <p className="text-sm text-[var(--color-muted)]">loading…</p>
  }
  if (provenance.isError || !provenance.data) {
    return (
      <p className="text-sm text-[var(--color-redact)]">
        failed to load /api/file: {String(provenance.error)}
      </p>
    )
  }

  const data: ProvenanceData = provenance.data

  if (data.total_touches === 0) {
    return (
      <section className="space-y-3">
        <header className="space-y-1">
          <h1 className="font-mono text-base text-[var(--color-text)] break-all">
            {data.path}
          </h1>
          <p className="text-xs text-[var(--color-muted)]">
            no touches recorded for this path.
          </p>
        </header>
      </section>
    )
  }

  const lastTouchedTs = data.events[data.events.length - 1]?.ts ?? ''
  const firstTouchedTs = data.events[0]?.ts ?? ''

  const visible = data.events.slice(0, expanded)
  const groups = groupByLocalDate(visible)

  return (
    <div className="space-y-8">
      <header className="sticky top-0 z-10 -mx-6 border-b border-[var(--color-border)] bg-[var(--color-bg)] px-6 py-4">
        <p className="text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
          file provenance
        </p>
        <h1 className="mt-1 font-mono text-base text-[var(--color-text)] break-all">
          {data.path}
        </h1>
        <p className="mt-2 text-xs text-[var(--color-muted)]">
          {data.total_touches.toLocaleString()} touch
          {data.total_touches === 1 ? '' : 'es'} ·{' '}
          first {formatLocalShort(firstTouchedTs)} · last{' '}
          {formatLocalShort(lastTouchedTs)}
        </p>
      </header>

      <section className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <StatTile
          label="total touches"
          value={data.total_touches.toLocaleString()}
        />
        <StatTile label="agents" value={data.agents.length} />
        <StatTile
          label="last touched"
          value={formatLocalShort(lastTouchedTs) || '—'}
        />
        <StatTile
          label="git history"
          value={
            git.data && git.data.length > 0
              ? `${git.data.length}+`
              : '—'
          }
        />
      </section>

      {data.agents.length > 0 && (
        <section>
          <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
            agents
          </h2>
          <div className="overflow-x-auto rounded border border-[var(--color-border)]">
            <table className="w-full border-collapse text-sm">
              <thead className="bg-[var(--color-panel)] text-[var(--color-muted)]">
                <tr>
                  <th className="px-3 py-2 text-left font-normal">agent</th>
                  <th className="px-3 py-2 text-right font-normal">touches</th>
                </tr>
              </thead>
              <tbody>
                {data.agents.map((a) => (
                  <tr
                    key={a.agent}
                    className="border-t border-[var(--color-border)]"
                  >
                    <td className="px-3 py-1.5 font-mono">{a.agent}</td>
                    <td className="px-3 py-1.5 text-right tabular-nums">
                      {a.count.toLocaleString()}
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
        <div className="space-y-6">
          {groups.map((group) => (
            <DayGroup key={group.dateLabel} group={group} />
          ))}
        </div>
        {expanded < data.events.length && (
          <div className="mt-4">
            <button
              type="button"
              onClick={() =>
                setExpanded((n) => Math.min(n + FIRST_PAINT_LIMIT, data.events.length))
              }
              className="text-xs text-[var(--color-accent)] hover:underline"
            >
              load more ({data.events.length - expanded} remaining)
            </button>
          </div>
        )}
      </section>

      {git.data && git.data.length > 0 && (
        <section>
          <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
            git history
          </h2>
          <div className="overflow-x-auto rounded border border-[var(--color-border)]">
            <table className="w-full border-collapse text-sm">
              <thead className="bg-[var(--color-panel)] text-[var(--color-muted)]">
                <tr>
                  <th className="px-3 py-2 text-left font-normal">commit</th>
                  <th className="px-3 py-2 text-left font-normal">when</th>
                  <th className="px-3 py-2 text-left font-normal">subject</th>
                </tr>
              </thead>
              <tbody>
                {git.data.map((c) => (
                  <tr
                    key={c.hash}
                    className="border-t border-[var(--color-border)]"
                  >
                    <td className="px-3 py-1.5 font-mono text-[var(--color-muted)]">
                      {c.hash.slice(0, 9)}
                    </td>
                    <td className="px-3 py-1.5 font-mono tabular-nums text-[var(--color-muted)]">
                      {formatLocalShort(c.ts)}
                    </td>
                    <td className="px-3 py-1.5 break-all">{c.subject}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      )}
    </div>
  )
}

interface Group {
  dateLabel: string
  events: ProvenanceEvent[]
}

function groupByLocalDate(events: ProvenanceEvent[]): Group[] {
  const buckets = new Map<string, ProvenanceEvent[]>()
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

function DayGroup({ group }: { group: Group }) {
  return (
    <div>
      <h3 className="font-mono text-xs uppercase tracking-wider text-[var(--color-muted)] mb-2">
        {group.dateLabel}
      </h3>
      <ol className="space-y-3">
        {group.events.map((ev, i) => (
          <li key={`${ev.ts}-${i}`}>
            <EventCard ev={ev} />
          </li>
        ))}
      </ol>
    </div>
  )
}

function EventCard({ ev }: { ev: ProvenanceEvent }) {
  const [open, setOpen] = useState(false)
  const detailSnippet = useMemo(() => truncate(ev.detail ?? '', 80), [ev.detail])
  const hasContext = ev.before_context.length > 0 || ev.after_context.length > 0
  return (
    <article className="rounded border border-[var(--color-border)] bg-[var(--color-panel)] p-3 text-sm">
      <header className="flex flex-wrap items-baseline justify-between gap-2">
        <div className="flex items-baseline gap-3">
          <span className="font-mono tabular-nums text-[var(--color-muted)]">
            {formatLocalTime(ev.ts)}
          </span>
          <span className="rounded bg-[var(--color-bg)] px-2 py-0.5 font-mono text-xs text-[var(--color-text)]">
            {ev.tool || '—'}
          </span>
          {ev.tool_class && (
            <span className="text-xs text-[var(--color-muted)]">
              {ev.tool_class}
            </span>
          )}
          <span className="text-xs text-[var(--color-muted)]">{ev.agent}</span>
        </div>
        {hasContext && (
          <button
            type="button"
            onClick={() => setOpen((v) => !v)}
            className="text-xs text-[var(--color-accent)] hover:underline"
          >
            {open ? 'hide context' : 'show ±5 min context'}
          </button>
        )}
      </header>
      <p className="mt-2 font-mono text-xs break-all">{detailSnippet || '—'}</p>
      {open && hasContext && (
        <div className="mt-3 grid gap-3 md:grid-cols-2">
          <ContextList title="before" items={ev.before_context} />
          <ContextList title="after" items={ev.after_context} />
        </div>
      )}
    </article>
  )
}

function ContextList({ title, items }: { title: string; items: ContextEvent[] }) {
  return (
    <div>
      <h4 className="text-[10px] uppercase tracking-wider text-[var(--color-muted)] mb-1">
        {title}
      </h4>
      {items.length === 0 ? (
        <p className="text-xs text-[var(--color-muted)]">no events</p>
      ) : (
        <ul className="space-y-1">
          {items.map((c, i) => (
            <li
              key={`${c.ts}-${i}`}
              className="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-xs font-mono break-all"
            >
              <span className="text-[var(--color-muted)] mr-2">
                {formatLocalTime(c.ts)}
              </span>
              <span className="text-[var(--color-text)] mr-2">
                {c.tool || '—'}
              </span>
              <span className="text-[var(--color-muted)] mr-2">{c.agent}</span>
              <span className="break-all">{truncate(c.detail ?? '', 80)}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

function truncate(s: string, max: number): string {
  if (s.length <= max) return s
  return `${s.slice(0, max - 1)}…`
}
