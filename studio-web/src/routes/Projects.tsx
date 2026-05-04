// Projects — cross-day project arcs.
//
// Pulls `/api/projects?days_back=…` and renders one card per cluster.
// Clicking a card unfolds the underlying sessions inline (re-using the
// session row component from the Sessions page) so the reader can drop
// into a single session's detail view without a round trip.

import { useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'

import {
  getProjects,
  getSessions,
  type Project,
  type ProjectStatus,
  type Session,
} from '../lib/api'
import { LifecycleBadge, formatDuration } from './Sessions'

const RANGE_OPTIONS: ReadonlyArray<{ label: string; days: number }> = [
  { label: '7d', days: 7 },
  { label: '14d', days: 14 },
  { label: '30d', days: 30 },
  { label: '90d', days: 90 },
]

export function Projects() {
  const [days, setDays] = useState<number>(30)

  const projects = useQuery({
    queryKey: ['projects', days],
    queryFn: () => getProjects(days),
  })

  return (
    <div className="space-y-6">
      <header className="flex items-baseline justify-between">
        <div>
          <p className="text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
            project arcs
          </p>
          <h1 className="mt-1 text-2xl text-[var(--color-text)] font-medium">
            Projects
          </h1>
          <p className="mt-1 text-xs text-[var(--color-muted)]">
            Sessions clustered across days · LLM-named when reachable,
            heuristic otherwise.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <label className="text-xs text-[var(--color-muted)]">window</label>
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

      {projects.isPending && (
        <p className="text-xs text-[var(--color-muted)]">loading…</p>
      )}
      {projects.isError && (
        <p className="text-sm text-[var(--color-redact)]">
          failed to load /api/projects: {String(projects.error)}
        </p>
      )}
      {projects.data && projects.data.length === 0 && (
        <p className="text-xs text-[var(--color-muted)]">
          no projects clustered yet — run a few sessions in the same
          working directory and come back.
        </p>
      )}
      {projects.data && projects.data.length > 0 && (
        <div className="space-y-4">
          {projects.data
            .slice()
            .sort((a, b) => (a.end < b.end ? 1 : a.end > b.end ? -1 : 0))
            .map((p) => (
              <ProjectCard key={p.id} project={p} windowDays={days} />
            ))}
        </div>
      )}
    </div>
  )
}

interface ProjectCardProps {
  project: Project
  windowDays: number
}

function ProjectCard({ project, windowDays }: ProjectCardProps) {
  const [open, setOpen] = useState(false)
  const range = useMemo(() => buildRange(windowDays), [windowDays])
  const sessions = useQuery({
    queryKey: ['sessions', range.from, range.to],
    queryFn: () => getSessions(range.from, range.to),
    enabled: open,
  })

  const projectSessions = useMemo(() => {
    if (!sessions.data) return [] as Session[]
    const ids = new Set(project.session_ids)
    return sessions.data.filter((s) => ids.has(s.id))
  }, [sessions.data, project.session_ids])

  return (
    <article className="rounded border border-[var(--color-border)] bg-[var(--color-panel)]">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="w-full text-left px-4 py-3 hover:bg-[var(--color-bg)]"
      >
        <div className="flex flex-wrap items-baseline justify-between gap-3">
          <div className="min-w-0">
            <h2 className="font-mono text-base text-[var(--color-text)] break-all">
              {project.name}
            </h2>
            <p className="mt-1 text-xs text-[var(--color-muted)] font-mono tabular-nums">
              {project.session_ids.length} session(s) · {project.total_events.toLocaleString()} events
              {project.total_usd > 0 && (
                <> · ${project.total_usd.toFixed(4)}</>
              )}
              {project.dominant_cwd && (
                <> · <span className="break-all">{project.dominant_cwd}</span></>
              )}
            </p>
          </div>
          <div className="flex items-center gap-2 shrink-0">
            <StatusPill status={project.status} />
            <SourcePill source={project.source} />
          </div>
        </div>
        <p className="mt-3 text-sm text-[var(--color-text)] leading-relaxed">
          {project.arc}
        </p>
        <p className="mt-2 text-[10px] uppercase tracking-wider text-[var(--color-muted)] font-mono">
          {project.start.slice(0, 10)} → {project.end.slice(0, 10)}
          {' · '}
          {open ? 'hide sessions' : 'show sessions'}
        </p>
      </button>
      {open && (
        <div className="border-t border-[var(--color-border)] px-4 py-3">
          {sessions.isPending && (
            <p className="text-xs text-[var(--color-muted)]">loading…</p>
          )}
          {sessions.isError && (
            <p className="text-xs text-[var(--color-redact)]">
              failed to load sessions: {String(sessions.error)}
            </p>
          )}
          {projectSessions.length === 0 && sessions.data && (
            <p className="text-xs text-[var(--color-muted)]">
              session detail isn&apos;t inside the current window — widen
              it above.
            </p>
          )}
          {projectSessions.length > 0 && (
            <table className="w-full border-collapse text-sm">
              <thead className="text-[var(--color-muted)] text-left">
                <tr>
                  <th className="px-2 py-1 font-normal">name</th>
                  <th className="px-2 py-1 font-normal">when</th>
                  <th className="px-2 py-1 font-normal text-right">duration</th>
                  <th className="px-2 py-1 font-normal text-right">events</th>
                  <th className="px-2 py-1 font-normal">lifecycle</th>
                </tr>
              </thead>
              <tbody>
                {projectSessions.map((s) => (
                  <tr
                    key={s.id}
                    className="border-t border-[var(--color-border)] hover:bg-[var(--color-bg)]"
                  >
                    <td className="px-2 py-1">
                      <Link
                        to={`/session/${s.id}`}
                        className="font-mono text-[var(--color-accent)] hover:underline"
                      >
                        {s.name}
                      </Link>
                    </td>
                    <td className="px-2 py-1 font-mono tabular-nums text-[var(--color-muted)]">
                      {new Date(s.start).toLocaleString()}
                    </td>
                    <td className="px-2 py-1 font-mono tabular-nums text-right">
                      {formatDuration(s.start, s.end)}
                    </td>
                    <td className="px-2 py-1 tabular-nums text-right">
                      {s.event_count.toLocaleString()}
                    </td>
                    <td className="px-2 py-1">
                      <LifecycleBadge lifecycle={s.lifecycle} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      )}
    </article>
  )
}

export function StatusPill({ status }: { status: ProjectStatus }) {
  const styles = statusStyle(status)
  return (
    <span
      className={`inline-flex rounded px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider ${styles}`}
    >
      {status}
    </span>
  )
}

function SourcePill({ source }: { source: Project['source'] }) {
  const label = source === 'llm' ? 'llm' : 'heuristic'
  const styles =
    source === 'llm'
      ? 'bg-sky-500/15 text-sky-300 border border-sky-500/30'
      : 'bg-[var(--color-bg)] text-[var(--color-muted)] border border-[var(--color-border)]'
  return (
    <span
      className={`inline-flex rounded px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider ${styles}`}
    >
      {label}
    </span>
  )
}

function statusStyle(status: ProjectStatus): string {
  switch (status) {
    case 'active':
      return 'bg-emerald-500/15 text-emerald-300 border border-emerald-500/30'
    case 'paused':
      return 'bg-amber-500/15 text-amber-300 border border-amber-500/30'
    case 'shipped':
      return 'bg-indigo-500/15 text-indigo-300 border border-indigo-500/30'
    case 'abandoned':
    default:
      return 'bg-[var(--color-bg)] text-[var(--color-muted)] border border-[var(--color-border)]'
  }
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
