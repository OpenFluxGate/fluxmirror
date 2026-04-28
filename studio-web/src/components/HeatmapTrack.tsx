// HeatmapTrack — 24-hour scrubbable heatmap-as-slider.
//
// 1440 minute cells laid out left-to-right inside an SVG. Cell opacity
// scales with the bucket count so dense minutes pop while quiet
// minutes stay visible as a faint baseline. Click anywhere on the
// track to jump that minute; drag the pointer to scrub.
//
// Pure SVG — no chart library — so the component remains tiny and
// works without any runtime JS dependency beyond React.

import { useCallback, useEffect, useRef } from 'react'

import type { MinuteBucket } from '../lib/api'

const TRACK_HEIGHT = 56
const HOUR_MARK_HEIGHT = TRACK_HEIGHT - 8

interface HeatmapTrackProps {
  /// Exactly 1440 entries (one per minute of the day). Out-of-bound
  /// indices fall back to zero so partial inputs still render.
  buckets: MinuteBucket[]
  /// Current scrub position in minutes since local-midnight (0..1439).
  currentMinute: number
  /// Called when the user clicks or drags. The minute is always
  /// clamped to `[0, 1439]`.
  onScrub: (minute: number) => void
  /// Optional aria label override.
  ariaLabel?: string
}

export function HeatmapTrack({
  buckets,
  currentMinute,
  onScrub,
  ariaLabel = 'Activity heatmap scrubber',
}: HeatmapTrackProps) {
  const counts = useNormalisedCounts(buckets)
  const max = counts.reduce((m, c) => Math.max(m, c), 0)
  const ref = useRef<SVGSVGElement | null>(null)
  const draggingRef = useRef(false)

  const minuteFromEvent = useCallback(
    (clientX: number): number => {
      const el = ref.current
      if (!el) return 0
      const rect = el.getBoundingClientRect()
      const ratio = (clientX - rect.left) / rect.width
      const m = Math.round(ratio * 1440)
      return Math.max(0, Math.min(1439, m))
    },
    [],
  )

  const onPointerDown = useCallback(
    (e: React.PointerEvent<SVGSVGElement>) => {
      e.preventDefault()
      draggingRef.current = true
      const el = ref.current
      if (el && el.setPointerCapture) {
        try {
          el.setPointerCapture(e.pointerId)
        } catch {
          // Some hosts disallow capture on synthetic events; ignore.
        }
      }
      onScrub(minuteFromEvent(e.clientX))
    },
    [minuteFromEvent, onScrub],
  )
  const onPointerMove = useCallback(
    (e: React.PointerEvent<SVGSVGElement>) => {
      if (!draggingRef.current) return
      onScrub(minuteFromEvent(e.clientX))
    },
    [minuteFromEvent, onScrub],
  )
  const onPointerEnd = useCallback(
    (e: React.PointerEvent<SVGSVGElement>) => {
      draggingRef.current = false
      const el = ref.current
      if (el && el.releasePointerCapture) {
        try {
          el.releasePointerCapture(e.pointerId)
        } catch {
          // ignored
        }
      }
    },
    [],
  )

  // Stop dragging on pointer leave outside the SVG (e.g. window blur).
  useEffect(() => {
    const cancel = () => {
      draggingRef.current = false
    }
    window.addEventListener('blur', cancel)
    return () => window.removeEventListener('blur', cancel)
  }, [])

  // 24-cell per-hour aggregation lets us paint the track with 1440
  // rects without paying for the textual hour-mark stroke per minute.
  const hourMarks = []
  for (let h = 1; h < 24; h++) {
    const x = (h * 60 * 100) / 1440
    hourMarks.push(
      <line
        key={h}
        x1={`${x}%`}
        y1={4}
        x2={`${x}%`}
        y2={HOUR_MARK_HEIGHT}
        stroke="var(--color-border)"
        strokeWidth={0.5}
        opacity={0.4}
      />,
    )
  }

  const cells = counts.map((c, m) => {
    const intensity = max === 0 ? 0 : c / max
    const opacity = c === 0 ? 0.04 : 0.18 + intensity * 0.82
    const x = (m / 1440) * 100
    const w = 100 / 1440
    return (
      <rect
        key={m}
        x={`${x}%`}
        y={4}
        width={`${w + 0.02}%`}
        height={TRACK_HEIGHT - 8}
        fill="var(--color-accent)"
        opacity={opacity}
      />
    )
  })

  const cursorPct = (currentMinute / 1440) * 100

  return (
    <div className="select-none">
      <svg
        ref={ref}
        role="slider"
        aria-label={ariaLabel}
        aria-valuemin={0}
        aria-valuemax={1439}
        aria-valuenow={currentMinute}
        viewBox={`0 0 100 ${TRACK_HEIGHT}`}
        preserveAspectRatio="none"
        width="100%"
        height={TRACK_HEIGHT}
        className="cursor-crosshair touch-none"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerEnd}
        onPointerCancel={onPointerEnd}
      >
        <rect
          x={0}
          y={4}
          width={100}
          height={TRACK_HEIGHT - 8}
          fill="var(--color-panel)"
          stroke="var(--color-border)"
          strokeWidth={0.2}
        />
        {cells}
        {hourMarks}
        <line
          x1={`${cursorPct}%`}
          y1={0}
          x2={`${cursorPct}%`}
          y2={TRACK_HEIGHT}
          stroke="var(--color-text)"
          strokeWidth={0.6}
        />
        <circle
          cx={`${cursorPct}%`}
          cy={TRACK_HEIGHT / 2}
          r={1.6}
          fill="var(--color-text)"
        />
      </svg>
      <ul className="mt-1 grid grid-cols-12 text-[10px] text-[var(--color-muted)] tabular-nums">
        {Array.from({ length: 12 }, (_, i) => i * 2).map((h) => (
          <li key={h} className="text-left">
            {String(h).padStart(2, '0')}
          </li>
        ))}
      </ul>
    </div>
  )
}

function useNormalisedCounts(buckets: MinuteBucket[]): number[] {
  const out = new Array<number>(1440).fill(0)
  for (const b of buckets) {
    if (b.minute >= 0 && b.minute < 1440) {
      out[b.minute] = b.count
    }
  }
  return out
}
