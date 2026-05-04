// Typed fetch helpers for the fluxmirror-studio JSON API.
//
// The interfaces below mirror the serde-derived DTOs in
// `fluxmirror-core::report::dto`. Keep them in lockstep — adding a
// field on the Rust side without updating these breaks autocomplete
// silently in the IDE but never breaks the runtime (extra JSON keys
// are tolerated).

export interface AgentCount {
  agent: string
  calls: number
  sessions: string[]
  active_days: string[] // YYYY-MM-DD
  top_tool: string
}

export interface FileTouch {
  path: string
  tool: string
  count: number
}

export interface PathCount {
  path: string
  count: number
}

export interface ToolMixEntry {
  tool: string
  count: number
}

export interface MethodCount {
  method: string
  count: number
}

export interface ShellEvent {
  time_local: string // HH:MM
  detail: string
  ts_utc: string // ISO 8601
}

export interface HourBucket {
  hour: number // 0..=23
  count: number
}

export interface DayRow {
  date: string // YYYY-MM-DD
  calls: number
}

export interface AgentCost {
  agent: string
  usd: number
  tokens_in: number
  tokens_out: number
  estimate: boolean
}

export interface ModelCost {
  model: string
  usd: number
  tokens_in: number
  tokens_out: number
  estimate: boolean
}

export interface CostSummary {
  from: string
  to: string
  total_usd: number
  by_agent: AgentCost[]
  by_model: ModelCost[]
  /** 0.0..1.0 — share of `total_usd` attributed to heuristic estimates. */
  estimate_share: number
}

export interface TodayData {
  date: string // YYYY-MM-DD
  tz: string
  total_events: number
  agents: AgentCount[]
  files_edited: FileTouch[]
  files_read: PathCount[]
  shells: ShellEvent[]
  cwds: PathCount[]
  mcp_methods: MethodCount[]
  tool_mix: ToolMixEntry[]
  hours: HourBucket[]
  writes_total: number
  reads_total: number
  distinct_files: string[]
  cost?: CostSummary | null
}

export interface WeekData {
  range_start: string // YYYY-MM-DD
  range_end: string // YYYY-MM-DD
  tz: string
  total_events: number
  agents: AgentCount[]
  files_edited: FileTouch[]
  files_read: PathCount[]
  cwds: PathCount[]
  tool_mix: ToolMixEntry[]
  daily: DayRow[]
  heatmap: number[][] // 7 × 24
  shell_counts: PathCount[]
  mcp_count: number
  writes_total: number
  reads_total: number
  cost?: CostSummary | null
}

export interface NowSnapshot {
  latest_ts_utc: string
  latest_agent: string
  latest_tool: string
  latest_detail: string
  latest_cwd: string
  last_hour_total: number
  last_hour_agents: AgentCount[]
}

export interface AgentTouchCount {
  agent: string
  count: number
}

export interface ContextEvent {
  ts: string
  agent: string
  tool: string
  detail: string | null
}

export interface ProvenanceEvent {
  ts: string
  agent: string
  tool: string
  tool_class: string
  detail: string | null
  before_context: ContextEvent[]
  after_context: ContextEvent[]
}

export interface ProvenanceData {
  path: string
  total_touches: number
  agents: AgentTouchCount[]
  events: ProvenanceEvent[]
}

export interface GitCommit {
  hash: string
  ts: string
  subject: string
}

// Heuristic session lifecycle. Lowercase variant strings match the
// `serde(rename_all = "lowercase")` annotation on the Rust enum.
export type SessionLifecycle =
  | 'shipping'
  | 'building'
  | 'polishing'
  | 'testing'
  | 'investigating'
  | 'idle'

export interface SessionEvent {
  ts: string
  agent: string
  tool: string
  detail: string | null
}

export interface Session {
  id: string
  start: string
  end: string
  agents: string[]
  event_count: number
  dominant_cwd: string | null
  top_files: string[]
  tool_mix: ToolMixEntry[]
  lifecycle: SessionLifecycle
  name: string
  // Empty for the list endpoint; populated for the detail endpoint.
  events: SessionEvent[]
}

export interface ReplayEvent {
  ts: string // ISO 8601 UTC
  agent: string
  tool: string
  tool_class: string
  detail: string | null
}

export interface MinuteBucket {
  minute: number // 0..1439
  count: number
}

export interface ReplayDay {
  date: string // YYYY-MM-DD
  events: ReplayEvent[]
  minute_buckets: MinuteBucket[] // 1440 entries
}

export interface ReplaySnapshot {
  at: string // ISO 8601 UTC
  active_file: string | null
  last_n_events: ReplayEvent[]
  agent_minute_mix: AgentCount[]
  tool_minute_mix: ToolMixEntry[]
}

// Lifecycle phase of a project. Lowercase variant strings match the
// `serde(rename_all = "lowercase")` annotation on the Rust enum.
export type ProjectStatus = 'active' | 'paused' | 'shipped' | 'abandoned'

// Provenance of a project's name + arc fields. `llm` means the LLM
// upgrade ran successfully; `heuristic` means the deterministic
// fallback was used (provider off, parse error, budget hit, etc.).
export type ProjectSource = 'llm' | 'heuristic'

export interface Project {
  id: string
  name: string
  arc: string
  status: ProjectStatus
  session_ids: string[]
  start: string // ISO 8601 UTC
  end: string // ISO 8601 UTC
  total_events: number
  total_usd: number
  dominant_cwd: string | null
  source: ProjectSource
}

async function getJson<T>(path: string): Promise<T> {
  const res = await fetch(path)
  if (!res.ok) {
    let body = ''
    try {
      body = await res.text()
    } catch {
      // ignored — error body is best-effort context
    }
    throw new Error(`${path} → ${res.status}${body ? `: ${body}` : ''}`)
  }
  return (await res.json()) as T
}

export const fetchToday = (): Promise<TodayData> => getJson('/api/today')
export const fetchWeek = (): Promise<WeekData> => getJson('/api/week')
export const fetchNow = (): Promise<NowSnapshot | null> =>
  getJson<NowSnapshot | null>('/api/now')

export const getFile = (path: string): Promise<ProvenanceData> =>
  getJson<ProvenanceData>(`/api/file?path=${encodeURIComponent(path)}`)

export const getFileGit = (path: string): Promise<GitCommit[]> =>
  getJson<GitCommit[]>(`/api/file/git?path=${encodeURIComponent(path)}`)

export const getSessions = (
  from?: string,
  to?: string,
): Promise<Session[]> => {
  const params = new URLSearchParams()
  if (from) params.set('from', from)
  if (to) params.set('to', to)
  const qs = params.toString()
  return getJson<Session[]>(qs ? `/api/sessions?${qs}` : '/api/sessions')
}

export const getSession = (id: string): Promise<Session> =>
  getJson<Session>(`/api/session/${encodeURIComponent(id)}`)

export const getReplay = (date: string): Promise<ReplayDay> =>
  getJson<ReplayDay>(`/api/replay/${encodeURIComponent(date)}`)

export const getReplaySnapshot = (
  date: string,
  ts: string,
): Promise<ReplaySnapshot> =>
  getJson<ReplaySnapshot>(
    `/api/replay/${encodeURIComponent(date)}/at?ts=${encodeURIComponent(ts)}`,
  )

export const getProjects = (daysBack?: number): Promise<Project[]> => {
  if (daysBack && daysBack > 0) {
    return getJson<Project[]>(`/api/projects?days_back=${daysBack}`)
  }
  return getJson<Project[]>('/api/projects')
}
