// Replay — scrubbable per-day timeline of every captured tool call.
//
// Top: heatmap-as-slider + transport controls. Below: three live panes
// driven by a snapshot fetch keyed off the scrub position. Keyboard
// nav and playback are handled by `useReplayClock`. Snapshot fetches
// are second-rounded so requestAnimationFrame ticks at 60fps don't
// flood the API.

import { useEffect, useMemo } from 'react'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { keepPreviousData, useQuery } from '@tanstack/react-query'

import {
  getReplay,
  getReplaySnapshot,
  type ReplayDay,
  type ReplayEvent,
  type ReplaySnapshot,
} from '../lib/api'
import {
  REPLAY_SPEEDS,
  useReplayClock,
} from '../lib/useReplayClock'
import { HeatmapTrack } from '../components/HeatmapTrack'

const MINUTE_MS = 60_000

export function Replay() {
  const params = useParams()
  const navigate = useNavigate()
  const dateParam = params.date

  // No date in URL → redirect to today's replay.
  useEffect(() => {
    if (!dateParam) {
      navigate(`/replay/${todayLocalIso()}`, { replace: true })
    }
  }, [dateParam, navigate])

  if (!dateParam) {
    return <p className="text-sm text-[var(--color-muted)]">redirecting…</p>
  }

  return <ReplayBody date={dateParam} />
}

function ReplayBody({ date }: { date: string }) {
  const navigate = useNavigate()

  const bounds = useMemo(() => parseDateBounds(date), [date])

  const day = useQuery({
    queryKey: ['replay', date],
    queryFn: () => getReplay(date),
  })

  const clock = useReplayClock({
    dayStartMs: bounds.startMs,
    dayEndMs: bounds.endMs,
    initialMs: bounds.startMs,
  })

  // Round to whole seconds so playback at 60fps doesn't rebuild the
  // query key every frame. The snapshot is identical for any ts inside
  // the same second, so this is a free coalesce.
  const tsSecondMs = Math.floor(clock.ts / 1000) * 1000
  const tsIso = useMemo(() => new Date(tsSecondMs).toISOString(), [tsSecondMs])

  const snap = useQuery({
    queryKey: ['replay-snap', date, tsIso],
    queryFn: () => getReplaySnapshot(date, tsIso),
    placeholderData: keepPreviousData,
  })

  const minuteOfDay = Math.max(
    0,
    Math.min(1439, Math.floor((clock.ts - bounds.startMs) / MINUTE_MS)),
  )

  if (day.isPending) {
    return <p className="text-sm text-[var(--color-muted)]">loading…</p>
  }
  if (day.isError || !day.data) {
    return (
      <p className="text-sm text-[var(--color-redact)]">
        failed to load /api/replay/{date}: {String(day.error)}
      </p>
    )
  }

  const data: ReplayDay = day.data

  return (
    <div className="space-y-6">
      <header className="space-y-1">
        <p className="text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
          time machine
        </p>
        <h1 className="font-mono text-2xl text-[var(--color-text)]">
          {data.date}
        </h1>
        <p className="text-xs text-[var(--color-muted)]">
          {data.events.length.toLocaleString()} events ·{' '}
          scrub:{' '}
          <span className="font-mono text-[var(--color-text)]">
            {formatHms(clock.ts)}
          </span>
        </p>
      </header>

      <Transport
        date={date}
        playing={clock.playing}
        speed={clock.speed}
        onPrev={() => clock.step(-10 * MINUTE_MS)}
        onNext={() => clock.step(10 * MINUTE_MS)}
        onToggle={clock.toggle}
        onSpeed={clock.setSpeed}
        onDateChange={(d) => navigate(`/replay/${d}`)}
      />

      <section>
        <HeatmapTrack
          buckets={data.minute_buckets}
          currentMinute={minuteOfDay}
          onScrub={(m) => clock.setTs(bounds.startMs + m * MINUTE_MS)}
        />
      </section>

      <section className="grid gap-6 md:grid-cols-3">
        <ActiveFilePane snap={snap.data} />
        <LastEventsPane events={snap.data?.last_n_events ?? []} />
        <MixPane snap={snap.data} />
      </section>

      <footer className="text-[11px] text-[var(--color-muted)]">
        ←/→ ±1 min · Shift+←/→ ±10 min · Space play/pause · Home/End jump
      </footer>
    </div>
  )
}

interface TransportProps {
  date: string
  playing: boolean
  speed: number
  onPrev: () => void
  onNext: () => void
  onToggle: () => void
  onSpeed: (s: number) => void
  onDateChange: (d: string) => void
}

function Transport(props: TransportProps) {
  return (
    <div className="flex flex-wrap items-center gap-3 rounded border border-[var(--color-border)] bg-[var(--color-panel)] px-3 py-2 text-xs">
      <button
        type="button"
        onClick={props.onPrev}
        className="rounded bg-[var(--color-bg)] px-2 py-1 font-mono hover:bg-[var(--color-border)]"
        aria-label="back 10 minutes"
      >
        ◀◀
      </button>
      <button
        type="button"
        onClick={props.onToggle}
        className="rounded bg-[var(--color-accent)] px-3 py-1 font-mono text-[var(--color-bg)] hover:opacity-80"
        aria-label={props.playing ? 'pause' : 'play'}
      >
        {props.playing ? '⏸' : '▶'}
      </button>
      <button
        type="button"
        onClick={props.onNext}
        className="rounded bg-[var(--color-bg)] px-2 py-1 font-mono hover:bg-[var(--color-border)]"
        aria-label="forward 10 minutes"
      >
        ▶▶
      </button>
      <div className="flex items-center gap-1">
        <span className="text-[var(--color-muted)]">speed</span>
        {REPLAY_SPEEDS.map((s) => (
          <button
            key={s}
            type="button"
            onClick={() => props.onSpeed(s)}
            className={
              s === props.speed
                ? 'rounded bg-[var(--color-text)] px-2 py-0.5 font-mono text-[var(--color-bg)]'
                : 'rounded px-2 py-0.5 font-mono text-[var(--color-muted)] hover:text-[var(--color-text)]'
            }
          >
            {s}x
          </button>
        ))}
      </div>
      <label className="ml-auto flex items-center gap-2">
        <span className="text-[var(--color-muted)]">date</span>
        <input
          type="date"
          value={props.date}
          onChange={(e) => {
            const v = e.target.value
            if (v) props.onDateChange(v)
          }}
          className="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 font-mono"
        />
      </label>
    </div>
  )
}

function ActiveFilePane({ snap }: { snap: ReplaySnapshot | undefined }) {
  return (
    <article className="rounded border border-[var(--color-border)] bg-[var(--color-panel)] p-3">
      <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-2">
        active file
      </h2>
      {snap?.active_file ? (
        <Link
          to={`/file/${snap.active_file}`}
          className="font-mono text-sm text-[var(--color-text)] break-all hover:text-[var(--color-accent)]"
        >
          {snap.active_file}
        </Link>
      ) : (
        <p className="text-xs text-[var(--color-muted)]">
          no edit/write in the trailing 60s.
        </p>
      )}
    </article>
  )
}

function LastEventsPane({ events }: { events: ReplayEvent[] }) {
  return (
    <article className="rounded border border-[var(--color-border)] bg-[var(--color-panel)] p-3">
      <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-2">
        last 5 events
      </h2>
      {events.length === 0 ? (
        <p className="text-xs text-[var(--color-muted)]">no events yet.</p>
      ) : (
        <ol className="space-y-1.5 font-mono text-xs">
          {events.map((ev, i) => (
            <li
              key={`${ev.ts}-${i}`}
              className="flex gap-2 border-b border-[var(--color-border)] pb-1 last:border-b-0 last:pb-0"
            >
              <span className="w-14 shrink-0 tabular-nums text-[var(--color-muted)]">
                {formatLocalTime(ev.ts)}
              </span>
              <span className="w-16 shrink-0 truncate text-[var(--color-text)]">
                {ev.tool || '—'}
              </span>
              <span className="break-all text-[var(--color-muted)]">
                {ev.detail ?? ''}
              </span>
            </li>
          ))}
        </ol>
      )}
    </article>
  )
}

function MixPane({ snap }: { snap: ReplaySnapshot | undefined }) {
  const agents = snap?.agent_minute_mix ?? []
  const tools = snap?.tool_minute_mix ?? []
  const agentMax = agents.reduce((m, a) => Math.max(m, a.calls), 0)
  const toolMax = tools.reduce((m, t) => Math.max(m, t.count), 0)
  return (
    <article className="rounded border border-[var(--color-border)] bg-[var(--color-panel)] p-3 space-y-3">
      <div>
        <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-2">
          agent mix · 60s
        </h2>
        {agents.length === 0 ? (
          <p className="text-xs text-[var(--color-muted)]">no activity.</p>
        ) : (
          <ul className="space-y-1 font-mono text-xs">
            {agents.map((a) => (
              <li key={a.agent} className="flex items-center gap-2">
                <span className="w-24 shrink-0 truncate text-[var(--color-text)]">
                  {a.agent}
                </span>
                <span
                  className="h-2 rounded-sm bg-[var(--color-accent)]"
                  style={{
                    width: `${agentMax === 0 ? 0 : Math.max(2, (a.calls / agentMax) * 120)}px`,
                  }}
                  aria-hidden
                />
                <span className="tabular-nums text-[var(--color-muted)]">
                  {a.calls}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>
      <div>
        <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-2">
          tool mix · 60s
        </h2>
        {tools.length === 0 ? (
          <p className="text-xs text-[var(--color-muted)]">no activity.</p>
        ) : (
          <ul className="space-y-1 font-mono text-xs">
            {tools.slice(0, 8).map((t) => (
              <li key={t.tool} className="flex items-center gap-2">
                <span className="w-24 shrink-0 truncate text-[var(--color-text)]">
                  {t.tool}
                </span>
                <span
                  className="h-2 rounded-sm bg-[var(--color-text)] opacity-60"
                  style={{
                    width: `${toolMax === 0 ? 0 : Math.max(2, (t.count / toolMax) * 120)}px`,
                  }}
                  aria-hidden
                />
                <span className="tabular-nums text-[var(--color-muted)]">
                  {t.count}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </article>
  )
}

interface DayBounds {
  startMs: number
  endMs: number
}

/// Resolve the URL date to UTC bounds. The studio API resolves replay
/// days in UTC (M9 will add the configurable tz), so the browser-side
/// scrub position must use the same anchor — otherwise `minute = 0`
/// would mean local midnight here but UTC midnight on the wire.
function parseDateBounds(date: string): DayBounds {
  const [y, m, d] = date.split('-').map((s) => Number.parseInt(s, 10))
  if (!y || !m || !d) {
    const now = Date.now()
    return { startMs: now, endMs: now + 86_400_000 }
  }
  const start = Date.UTC(y, m - 1, d, 0, 0, 0, 0)
  const end = Date.UTC(y, m - 1, d + 1, 0, 0, 0, 0)
  return { startMs: start, endMs: end }
}

function todayLocalIso(): string {
  const d = new Date()
  const yyyy = d.getUTCFullYear()
  const mm = String(d.getUTCMonth() + 1).padStart(2, '0')
  const dd = String(d.getUTCDate()).padStart(2, '0')
  return `${yyyy}-${mm}-${dd}`
}

function formatHms(ms: number): string {
  const d = new Date(ms)
  const hh = String(d.getUTCHours()).padStart(2, '0')
  const mm = String(d.getUTCMinutes()).padStart(2, '0')
  const ss = String(d.getUTCSeconds()).padStart(2, '0')
  return `${hh}:${mm}:${ss}`
}

function formatLocalTime(ts: string): string {
  if (!ts) return ''
  const d = new Date(ts)
  if (Number.isNaN(d.getTime())) return ts
  const hh = String(d.getUTCHours()).padStart(2, '0')
  const mm = String(d.getUTCMinutes()).padStart(2, '0')
  return `${hh}:${mm}`
}
