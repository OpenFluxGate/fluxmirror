// useReplayClock — scrubber + playback state for the /replay page.
//
// Owns the current ts (epoch ms in UTC), playing flag, and speed
// multiplier. While `playing` is true a `requestAnimationFrame` loop
// advances ts at 1 second of real time per real second, multiplied by
// `speed`.
//
// Keyboard nav: ←/→ = ±1 minute, Shift+arrow = ±10 minutes, Space =
// toggle play, Home/End = jump to day bounds.

import { useCallback, useEffect, useRef, useState } from 'react'

export interface ReplayClockOptions {
  /// Inclusive lower bound of the day (epoch ms, UTC).
  dayStartMs: number
  /// Exclusive upper bound of the day (epoch ms, UTC).
  dayEndMs: number
  /// Initial scrub position. Clamped into the day.
  initialMs?: number
}

export interface ReplayClockState {
  ts: number
  playing: boolean
  speed: number
  setTs: (ms: number) => void
  setSpeed: (s: number) => void
  toggle: () => void
  step: (deltaMs: number) => void
  jumpStart: () => void
  jumpEnd: () => void
}

export const REPLAY_SPEEDS: ReadonlyArray<number> = [0.5, 1, 2, 5]
const MINUTE_MS = 60_000

export function useReplayClock(opts: ReplayClockOptions): ReplayClockState {
  const { dayStartMs, dayEndMs } = opts
  const [ts, setTsRaw] = useState<number>(() =>
    clamp(opts.initialMs ?? dayStartMs, dayStartMs, dayEndMs - 1),
  )
  const [playing, setPlaying] = useState(false)
  const [speed, setSpeed] = useState(1)

  const setTs = useCallback(
    (ms: number) => setTsRaw(clamp(ms, dayStartMs, dayEndMs - 1)),
    [dayStartMs, dayEndMs],
  )
  const step = useCallback(
    (deltaMs: number) => setTsRaw((cur) => clamp(cur + deltaMs, dayStartMs, dayEndMs - 1)),
    [dayStartMs, dayEndMs],
  )
  const toggle = useCallback(() => setPlaying((p) => !p), [])
  const jumpStart = useCallback(() => setTsRaw(dayStartMs), [dayStartMs])
  const jumpEnd = useCallback(() => setTsRaw(dayEndMs - 1), [dayEndMs])

  // requestAnimationFrame loop. Advances `ts` while `playing`. Pauses
  // automatically when ts hits the day's upper bound.
  const lastFrameRef = useRef<number>(0)
  useEffect(() => {
    if (!playing) return
    let raf = 0
    lastFrameRef.current = performance.now()
    const tick = (now: number) => {
      const elapsed = now - lastFrameRef.current
      lastFrameRef.current = now
      setTsRaw((cur) => {
        const next = cur + elapsed * speed
        if (next >= dayEndMs - 1) {
          setPlaying(false)
          return dayEndMs - 1
        }
        return next
      })
      raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [playing, speed, dayEndMs])

  // Keyboard nav. Ignored when focus is on an input/textarea/select.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement | null)?.tagName ?? ''
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return
      const big = e.shiftKey ? 10 : 1
      switch (e.key) {
        case 'ArrowLeft':
          e.preventDefault()
          step(-big * MINUTE_MS)
          return
        case 'ArrowRight':
          e.preventDefault()
          step(big * MINUTE_MS)
          return
        case ' ':
        case 'Space':
        case 'Spacebar':
          e.preventDefault()
          toggle()
          return
        case 'Home':
          e.preventDefault()
          jumpStart()
          return
        case 'End':
          e.preventDefault()
          jumpEnd()
          return
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [step, toggle, jumpStart, jumpEnd])

  return { ts, playing, speed, setTs, setSpeed, toggle, step, jumpStart, jumpEnd }
}

function clamp(v: number, lo: number, hi: number): number {
  return v < lo ? lo : v > hi ? hi : v
}
