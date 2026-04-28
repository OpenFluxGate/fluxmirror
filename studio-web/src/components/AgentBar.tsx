// AgentBar — per-agent activity row used by Today and Week. Renders
// one stacked row per agent, sorted by call count desc.

import type { AgentCount } from '../lib/api'

interface AgentBarProps {
  agents: AgentCount[]
}

export function AgentBar({ agents }: AgentBarProps) {
  if (agents.length === 0) {
    return (
      <p className="text-xs text-[var(--color-muted)]">no agent activity</p>
    )
  }
  const max = agents.reduce((m, a) => Math.max(m, a.calls), 0)
  return (
    <ol className="space-y-2">
      {agents.map((a) => {
        const ratio = max === 0 ? 0 : a.calls / max
        const width = a.calls === 0 ? 0 : Math.max(2, ratio * 320)
        return (
          <li
            key={a.agent}
            className="rounded border border-[var(--color-border)] bg-[var(--color-panel)] px-3 py-2"
          >
            <div className="flex items-center justify-between text-sm">
              <span className="font-mono text-[var(--color-text)]">{a.agent}</span>
              <span className="text-xs text-[var(--color-muted)]">
                {a.sessions.length}{' '}
                {a.sessions.length === 1 ? 'session' : 'sessions'}
                {a.top_tool && (
                  <>
                    {' · '}
                    <span className="font-mono">{a.top_tool}</span>
                  </>
                )}
              </span>
            </div>
            <div className="mt-2 flex items-center gap-3">
              <span
                className="h-2 rounded-sm bg-[var(--color-accent)]"
                style={{ width: `${width}px` }}
                aria-hidden
              />
              <span className="font-mono text-xs tabular-nums text-[var(--color-text)]">
                {a.calls.toLocaleString()}
              </span>
            </div>
          </li>
        )
      })}
    </ol>
  )
}
