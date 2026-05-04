// Phase 4 M-A6 — list of LLM- or heuristic-decorated anomaly stories.
//
// Each item shows a kind icon, the story sentence, and a small evidence
// strip below. An "estimate" pill marks heuristic-source items so the
// reader knows which sentences came from a deterministic template
// rather than the LLM.

import type { AnomalyKind, AnomalyStory } from '../lib/api'

interface Props {
  items: AnomalyStory[]
}

const KIND_ICON: Record<AnomalyKind, string> = {
  file_edit_spike: '⚡',
  tool_mix_departure: '🔀',
  new_agent: '👤',
  new_mcp_method: '🔌',
  cost_per_call_rise: '💸',
}

const KIND_LABEL: Record<AnomalyKind, string> = {
  file_edit_spike: 'edit spike',
  tool_mix_departure: 'tool-mix shift',
  new_agent: 'new agent',
  new_mcp_method: 'new mcp method',
  cost_per_call_rise: 'cost rise',
}

export function AnomalyList({ items }: Props) {
  if (items.length === 0) return null
  return (
    <ul className="space-y-3">
      {items.map((item, i) => (
        <li
          key={`${item.kind}-${i}`}
          className="rounded border border-[var(--color-border)] bg-[var(--color-panel)] p-3"
        >
          <div className="flex flex-wrap items-baseline justify-between gap-2">
            <div className="flex items-baseline gap-2">
              <span aria-hidden="true">{KIND_ICON[item.kind]}</span>
              <span className="text-[10px] uppercase tracking-wider text-[var(--color-muted)] font-mono">
                {KIND_LABEL[item.kind]}
              </span>
            </div>
            {item.source === 'heuristic' && (
              <span
                className="rounded bg-[var(--color-bg)] px-1.5 py-0.5 text-[10px] uppercase tracking-wider text-[var(--color-muted)] font-mono"
                title="Story generated from a deterministic template (LLM provider off, error, or budget hit)."
              >
                estimate
              </span>
            )}
          </div>
          <p className="mt-1.5 text-sm text-[var(--color-text)] leading-relaxed">
            {item.story}
          </p>
          {item.evidence.length > 0 && (
            <ul className="mt-2 flex flex-wrap gap-1.5">
              {item.evidence.slice(0, 5).map((e, j) => (
                <li
                  key={`${i}-${j}`}
                  className="rounded bg-[var(--color-bg)] px-2 py-0.5 font-mono text-[11px] text-[var(--color-muted)]"
                >
                  {e}
                </li>
              ))}
            </ul>
          )}
        </li>
      ))}
    </ul>
  )
}
