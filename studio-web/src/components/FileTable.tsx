// FileTable — top-N file paths with optional tool column. Used by both
// Today (files-edited / files-read) and Week (top edited / top read).
//
// When `linkify` is set (the default), each path renders as a router
// link to `/file/<encoded>` so the per-file provenance timeline is
// always one click away. Callers can opt out (e.g. cwd / mcp method
// tables) by passing `linkify={false}`.

import { Link } from 'react-router-dom'

interface FileTableProps {
  rows: { path: string; tool?: string; count: number }[]
  /// Optional row cap. Defaults to all rows.
  limit?: number
  /// Display column for the tool. Hidden when no row carries a tool.
  showTool?: boolean
  /// Heading shown above the table. Optional — caller may render its
  /// own heading and pass `undefined`.
  caption?: string
  /// Render the path column as a link to `/file/<encoded>`. Default
  /// `true`; set false for cwd / method tables where the path isn't
  /// a tracked file.
  linkify?: boolean
}

export function FileTable({
  rows,
  limit,
  showTool,
  caption,
  linkify = true,
}: FileTableProps) {
  const visible = limit ? rows.slice(0, limit) : rows
  if (visible.length === 0) {
    return null
  }
  const showToolCol = !!showTool && visible.some((r) => !!r.tool)
  return (
    <div>
      {caption && (
        <h3 className="text-xs uppercase tracking-wider text-[var(--color-muted)] mb-2">
          {caption}
        </h3>
      )}
      <div className="overflow-x-auto rounded border border-[var(--color-border)]">
        <table className="w-full border-collapse text-sm">
          <thead className="bg-[var(--color-panel)] text-[var(--color-muted)]">
            <tr>
              <th className="px-3 py-2 text-left font-normal">path</th>
              {showToolCol && (
                <th className="px-3 py-2 text-left font-normal">tool</th>
              )}
              <th className="px-3 py-2 text-right font-normal">count</th>
            </tr>
          </thead>
          <tbody>
            {visible.map((r, i) => (
              <tr
                key={`${r.path}-${r.tool ?? ''}-${i}`}
                className="border-t border-[var(--color-border)]"
              >
                <td className="px-3 py-1.5 font-mono break-all text-[var(--color-text)]">
                  {linkify ? (
                    <Link
                      to={`/file/${encodeURIComponent(r.path)}`}
                      className="text-[var(--color-text)] hover:text-[var(--color-accent)] hover:underline"
                    >
                      {r.path}
                    </Link>
                  ) : (
                    r.path
                  )}
                </td>
                {showToolCol && (
                  <td className="px-3 py-1.5 font-mono text-[var(--color-muted)]">
                    {r.tool ?? ''}
                  </td>
                )}
                <td className="px-3 py-1.5 text-right tabular-nums">
                  {r.count.toLocaleString()}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}
