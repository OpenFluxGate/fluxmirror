import { useQuery } from '@tanstack/react-query'

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

export function Home() {
  const health = useQuery({ queryKey: ['health'], queryFn: fetchHealth })

  return (
    <div className="space-y-8">
      <section>
        <h1 className="text-2xl text-[var(--color-text)] font-medium">
          Your AI coding, in one tab.
        </h1>
        <p className="mt-2 text-sm text-[var(--color-muted)]">
          fluxmirror-studio is the read-only dashboard over your local
          fluxmirror SQLite store. Pages land in M2 — for now this is a
          health check.
        </p>
      </section>

      <section>
        <h2 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-3">
          status
        </h2>
        <div className="rounded border border-[var(--color-border)] bg-[var(--color-panel)] p-4 font-mono text-sm">
          {health.isPending && <span>loading…</span>}
          {health.isError && (
            <span className="text-[var(--color-redact)]">
              {String(health.error)}
            </span>
          )}
          {health.data && (
            <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1">
              <dt className="text-[var(--color-muted)]">status</dt>
              <dd>{health.data.status}</dd>
              <dt className="text-[var(--color-muted)]">version</dt>
              <dd>{health.data.version}</dd>
              <dt className="text-[var(--color-muted)]">db</dt>
              <dd className="break-all">{health.data.db}</dd>
              <dt className="text-[var(--color-muted)]">agent_events</dt>
              <dd>{health.data.agent_events.toLocaleString()}</dd>
              <dt className="text-[var(--color-muted)]">proxy_events</dt>
              <dd>{health.data.proxy_events.toLocaleString()}</dd>
            </dl>
          )}
        </div>
      </section>

      <section className="text-xs text-[var(--color-muted)]">
        <p>
          The capture binary keeps writing to{' '}
          <code className="font-mono">events.db</code>. This studio reads
          the same file, separately.
        </p>
      </section>
    </div>
  )
}
