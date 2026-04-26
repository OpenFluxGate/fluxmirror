// Per-language string packs for the report subcommands.
//
// Each `LangPack` carries the small set of user-facing strings a single
// report can render. A report module reads its pack once at the top of
// `run()` and does straight string interpolation — no template engine,
// no runtime parsing.
//
// Adding a new language: add a `static <NAME>: LangPack = ...` block,
// then route an entry through `pack()`. Unknown lang strings fall back
// to English, matching `Language::from_locale` behaviour upstream.

/// All localisable strings used by the `agents` report.
///
/// New reports add fields here rather than spawning a parallel struct
/// so a single edit point covers all languages.
pub struct LangPack {
    /// "Agent Roster" — the H1 of the agents report.
    pub agents_title: &'static str,
    /// Six column headers, in order:
    /// Agent | Calls | Sessions | Active Days | Dominant Tool | Write/Read/Shell
    pub agents_columns: [&'static str; 6],
    /// Single-line message printed when the 7-day window is empty.
    pub agents_no_activity: &'static str,
    /// Heading printed above the rules-based insights bullet list.
    pub insights_heading: &'static str,
    /// Suffix used in the title to label the active timezone.
    /// Rendered like: `<title> (last 7 days, <tz_label> <tz>)`.
    pub tz_label: &'static str,
    /// Range label preceding the start..end dates in the subtitle.
    pub range_label: &'static str,
    /// Insight phrasing: "{agent} is the busiest with N calls".
    /// `{agent}` and `{n}` are simple `{}` placeholders the caller fills.
    pub insight_busiest: &'static str,
    /// Insight phrasing for one-shot agents: "{agent} ran a single
    /// session ({n} calls)".
    pub insight_one_shot: &'static str,
    /// Insight phrasing for write-heavy agents: "{agent} is write-heavy
    /// ({pct}% writes)".
    pub insight_write_heavy: &'static str,

    // ----- today report -----------------------------------------------
    /// "Today's Work" — H1 of the today report. Rendered as
    /// `# {today_title} (YYYY-MM-DD <tz>)`.
    pub today_title: &'static str,
    /// "Activity" section heading.
    pub today_activity_heading: &'static str,
    /// "Files written or edited" section heading.
    pub today_files_edited_heading: &'static str,
    /// "Files only read" section heading.
    pub today_files_read_heading: &'static str,
    /// "Shell commands" section heading.
    pub today_shell_heading: &'static str,
    /// "Working directories" section heading.
    pub today_cwds_heading: &'static str,
    /// "MCP traffic methods" section heading.
    pub today_mcp_heading: &'static str,
    /// "Tool mix" section heading.
    pub today_tool_mix_heading: &'static str,
    /// "Hour distribution" section heading.
    pub today_hours_heading: &'static str,
    /// "Insights" section heading (today report uses its own copy so
    /// future divergence doesn't ripple through `agents`).
    pub today_insights_heading: &'static str,
    /// Single-line message printed when the day has fewer than 5 events.
    pub today_no_data: &'static str,
    /// Insight: "Most productive hour: HH:00 (N calls)". Placeholders:
    /// `{hour}` (HH:00) and `{n}` (call count).
    pub today_insight_busiest_hour: &'static str,
    /// Insight: "Edit-to-read ratio: X.YZ". Placeholder: `{ratio}`.
    pub today_insight_edit_read_ratio: &'static str,
    /// Insight: "Multi-project day: N distinct working dirs with >= 5
    /// calls". Placeholder: `{n}`.
    pub today_insight_multi_project: &'static str,
    /// Three column headers: Agent | Calls | Sessions.
    pub today_columns_calls_sessions: [&'static str; 3],
    /// Three column headers: File | Tool | Count.
    pub today_columns_file_tool_count: [&'static str; 3],
    /// Two column headers: Path | Count.
    pub today_columns_path_count: [&'static str; 2],
    /// Two column headers: Time | Command.
    pub today_columns_time_command: [&'static str; 2],
    /// Two column headers: Method | Count.
    pub today_columns_method_count: [&'static str; 2],
    /// Two column headers: Tool | Count.
    pub today_columns_tool_count: [&'static str; 2],

    // ----- yesterday report -------------------------------------------
    /// "Yesterday's Work" — H1 of the yesterday report. Rendered as
    /// `# {yesterday_title} (YYYY-MM-DD <tz>)`. The body sections reuse
    /// the today-flavoured headings (activity / files / shells / …).
    pub yesterday_title: &'static str,
    /// Single-line message when yesterday's window has fewer than 5
    /// events. Worded for "no activity yesterday" rather than today.
    pub yesterday_no_data: &'static str,

    // ----- week report ------------------------------------------------
    /// "Last 7 Days" — H1 of the week report. Rendered as
    /// `# {week_title} (YYYY-MM-DD ~ YYYY-MM-DD <tz>)`.
    pub week_title: &'static str,
    /// Single-line message when the 7-day window has fewer than 5 events.
    pub week_no_data: &'static str,
    /// "Per-day totals" section heading.
    pub week_daily_totals_heading: &'static str,
    /// "Day distribution" section heading (analogous to `today`'s hour
    /// distribution chart).
    pub week_day_distribution_heading: &'static str,
    /// Two column headers: Date | Calls.
    pub week_columns_date_calls: [&'static str; 2],
    /// Insight phrasing for the most-productive day rule. Placeholders:
    /// `{date}` (YYYY-MM-DD) and `{n}` (calls).
    pub week_insight_top_day: &'static str,
    /// Insight phrasing for the active-days rule. Placeholders: `{n}`
    /// (active days, integer 1..=7).
    pub week_insight_active_days: &'static str,
    /// Insight phrasing for the cross-project rule. Placeholders: `{n}`
    /// (number of distinct cwds with ≥5 calls).
    pub week_insight_cross_project: &'static str,

    // ----- about report -----------------------------------------------
    /// "About FluxMirror" — H1 of the about output.
    pub about_title: &'static str,
    /// One-paragraph project blurb (multi-line OK; rendered verbatim).
    pub about_blurb: &'static str,
    /// "Available commands" section heading.
    pub about_commands_heading: &'static str,
    /// "Where data lives" section heading.
    pub about_paths_heading: &'static str,
    /// Label preceding the database path: "Database:".
    pub about_db_label: &'static str,
    /// Label preceding the log file: "Log file:".
    pub about_log_label: &'static str,

    // ----- compare report ---------------------------------------------
    /// "Today vs Yesterday" — H1 of the compare report.
    pub compare_title: &'static str,
    /// Single-line message when both days are below the limited-activity
    /// threshold (rendered alone with no body).
    pub compare_no_data: &'static str,
    /// Three column headers: Metric | Today | Yesterday | Δ.
    /// We carry four entries instead of three because the table has a
    /// trailing delta column.
    pub compare_columns: [&'static str; 4],
    /// Six row labels for the comparison metrics, in render order:
    /// total calls, edit count, read count, shell count, distinct files,
    /// distinct cwds.
    pub compare_metric_labels: [&'static str; 6],
    /// Insight phrasing for the calls-trend rule. Placeholders:
    /// `{direction}` (an up/down indicator string from the lang pack)
    /// and `{pct}` (absolute percentage).
    pub compare_insight_calls_trend: &'static str,
    /// Insight phrasing when both days have zero events.
    pub compare_insight_both_quiet: &'static str,
    /// Insight phrasing when only today has data: `{n}` is today's count.
    pub compare_insight_only_today: &'static str,
    /// Insight phrasing when only yesterday has data: `{n}` yesterday's count.
    pub compare_insight_only_yesterday: &'static str,
    /// Word for "up" / "increase".
    pub compare_word_up: &'static str,
    /// Word for "down" / "decrease".
    pub compare_word_down: &'static str,

    // ----- init demo row ---------------------------------------------
    /// One-line confirmation printed after `fluxmirror init` inserts the
    /// `agent='setup'` demo row. Includes the literal removal command,
    /// quoted exactly so the user can copy/paste it.
    pub init_demo_row_inserted: &'static str,

    // ----- html weekly digest card (M5 option A) ---------------------
    /// "fluxmirror weekly digest" — H1 of the html card.
    pub html_title: &'static str,
    /// "Activity heatmap" section heading above the 24x7 grid.
    pub html_heatmap_heading: &'static str,
    /// "Top files (edited)" section heading.
    pub html_top_files_heading: &'static str,
    /// "Top shell commands" section heading.
    pub html_top_shells_heading: &'static str,
    /// "Per-agent summary" section heading above the agent table.
    pub html_per_agent_heading: &'static str,
    /// "Summary" section heading above the deterministic blurb.
    pub html_summary_heading: &'static str,
    /// Day-of-week labels for the heatmap rows. Index 0=Monday .. 6=Sunday.
    pub html_dow_labels: [&'static str; 7],
    /// Five column headers for the per-agent summary table:
    /// Agent | Calls | Sessions | Active days | Top tool.
    pub html_agent_columns: [&'static str; 5],
    /// Two column headers for the top-files / top-shells tables: Item | Count.
    pub html_item_count_columns: [&'static str; 2],
    /// Hour-of-day axis label printed above the heatmap grid.
    pub html_hour_axis_label: &'static str,
    /// Summary template. Placeholders: {calls}, {agents}, {dow},
    /// {dow_calls}, {tool}, {tool_calls}.
    pub html_summary_template: &'static str,
    /// Fallback summary line when the week is empty (no events).
    pub html_summary_empty: &'static str,
    /// Footer prefix: "Generated by fluxmirror " (the version + timestamp
    /// is appended in code to keep the footer line one source of truth).
    pub html_footer_generated: &'static str,
}

const ENGLISH: LangPack = LangPack {
    agents_title: "Agent Roster",
    agents_columns: [
        "Agent",
        "Calls",
        "Sessions",
        "Active Days",
        "Dominant Tool",
        "Write/Read/Shell",
    ],
    agents_no_activity: "No agent activity in the last 7 days.",
    insights_heading: "Insights",
    tz_label: "last 7 days",
    range_label: "Range",
    insight_busiest: "{agent} is the busiest with {n} calls.",
    insight_one_shot: "{agent} ran a single session ({n} calls).",
    insight_write_heavy: "{agent} is write-heavy ({pct}% writes).",
    today_title: "Today's Work",
    today_activity_heading: "Activity",
    today_files_edited_heading: "Files written or edited",
    today_files_read_heading: "Files only read",
    today_shell_heading: "Shell commands",
    today_cwds_heading: "Working directories",
    today_mcp_heading: "MCP traffic methods",
    today_tool_mix_heading: "Tool mix",
    today_hours_heading: "Hour distribution",
    today_insights_heading: "Insights",
    today_no_data: "Limited activity today.",
    today_insight_busiest_hour: "Most productive hour: {hour} ({n} calls)",
    today_insight_edit_read_ratio: "Edit-to-read ratio: {ratio}",
    today_insight_multi_project: "Multi-project day: {n} distinct working dirs with >= 5 calls",
    today_columns_calls_sessions: ["Agent", "Calls", "Sessions"],
    today_columns_file_tool_count: ["Path", "Tool", "Count"],
    today_columns_path_count: ["Path", "Count"],
    today_columns_time_command: ["Time", "Command"],
    today_columns_method_count: ["Method", "Count"],
    today_columns_tool_count: ["Tool", "Count"],
    yesterday_title: "Yesterday's Work",
    yesterday_no_data: "No activity yesterday.",
    week_title: "Last 7 Days",
    week_no_data: "Limited activity this week.",
    week_daily_totals_heading: "Per-day totals",
    week_day_distribution_heading: "Day distribution",
    week_columns_date_calls: ["Date", "Calls"],
    week_insight_top_day: "Most productive day: {date} ({n} calls)",
    week_insight_active_days: "Days active: {n}/7",
    week_insight_cross_project: "Cross-project: {n} distinct working dirs with >= 5 calls",
    about_title: "About FluxMirror",
    about_blurb: "FluxMirror is a local observability layer for AI coding agents. It records every tool call from Claude Code, Gemini CLI, Qwen Code, and (optionally) Claude Desktop's MCP traffic into a single SQLite database, then turns that data into per-day, per-week, and per-agent reports you can read from your own terminal.",
    about_commands_heading: "Available commands",
    about_paths_heading: "Where data lives",
    about_db_label: "Database",
    about_log_label: "Log file",
    compare_title: "Today vs Yesterday",
    compare_no_data: "Not enough activity to compare today and yesterday.",
    compare_columns: ["Metric", "Today", "Yesterday", "Δ"],
    compare_metric_labels: [
        "Total calls",
        "Edits",
        "Reads",
        "Shell commands",
        "Distinct files",
        "Distinct working dirs",
    ],
    compare_insight_calls_trend: "Calls: today is {direction} {pct}% vs yesterday.",
    compare_insight_both_quiet: "Both today and yesterday were quiet.",
    compare_insight_only_today: "Yesterday had no activity; today has {n} calls.",
    compare_insight_only_yesterday: "Today has no activity yet; yesterday had {n} calls.",
    compare_word_up: "up",
    compare_word_down: "down",
    init_demo_row_inserted: "Inserted demo row so /fluxmirror:today returns a meaningful report immediately. Remove with: fluxmirror sqlite \"DELETE FROM agent_events WHERE agent='setup'\"",
    html_title: "fluxmirror weekly digest",
    html_heatmap_heading: "Activity heatmap",
    html_top_files_heading: "Top files (edited)",
    html_top_shells_heading: "Top shell commands",
    html_per_agent_heading: "Per-agent summary",
    html_summary_heading: "Summary",
    html_dow_labels: ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
    html_agent_columns: ["Agent", "Calls", "Sessions", "Active days", "Top tool"],
    html_item_count_columns: ["Item", "Count"],
    html_hour_axis_label: "Hour of day (00 - 23)",
    html_summary_template: "Across the last 7 days you logged {calls} calls across {agents} agents. The busiest day was {dow} with {dow_calls} calls. Heaviest tool: {tool} at {tool_calls} calls.",
    html_summary_empty: "No agent activity recorded in the last 7 days.",
    html_footer_generated: "Generated by fluxmirror",
};

const KOREAN: LangPack = LangPack {
    agents_title: "에이전트 명세",
    agents_columns: [
        "에이전트",
        "호출",
        "세션",
        "활동 일수",
        "주요 도구",
        "쓰기/읽기/셸",
    ],
    agents_no_activity: "지난 7일간 에이전트 활동 없음.",
    insights_heading: "인사이트",
    tz_label: "지난 7일",
    range_label: "기간",
    insight_busiest: "{agent} 가 가장 활발 — 호출 {n}회.",
    insight_one_shot: "{agent} 는 단일 세션만 실행 ({n}회 호출).",
    insight_write_heavy: "{agent} 는 쓰기 중심 (쓰기 {pct}%).",
    today_title: "오늘의 작업",
    today_activity_heading: "활동 통계",
    today_files_edited_heading: "편집한 파일",
    today_files_read_heading: "읽기만 한 파일",
    today_shell_heading: "셸 명령",
    today_cwds_heading: "작업 디렉터리",
    today_mcp_heading: "MCP 호출 메서드",
    today_tool_mix_heading: "도구 분포",
    today_hours_heading: "시간대 분포",
    today_insights_heading: "인사이트",
    today_no_data: "오늘 활동 적음.",
    today_insight_busiest_hour: "가장 활발한 시간: {hour} (호출 {n}회)",
    today_insight_edit_read_ratio: "편집/읽기 비율: {ratio}",
    today_insight_multi_project: "멀티 프로젝트 날: 5회 이상 호출된 작업 디렉터리 {n}개",
    today_columns_calls_sessions: ["에이전트", "호출", "세션"],
    today_columns_file_tool_count: ["경로", "도구", "횟수"],
    today_columns_path_count: ["경로", "횟수"],
    today_columns_time_command: ["시간", "명령"],
    today_columns_method_count: ["메서드", "횟수"],
    today_columns_tool_count: ["도구", "횟수"],
    yesterday_title: "어제의 작업",
    yesterday_no_data: "어제 활동 없음.",
    week_title: "지난 7일",
    week_no_data: "이번 주 활동 적음.",
    week_daily_totals_heading: "일별 합계",
    week_day_distribution_heading: "요일 분포",
    week_columns_date_calls: ["날짜", "호출"],
    week_insight_top_day: "가장 활발한 날: {date} (호출 {n}회)",
    week_insight_active_days: "활동 일수: {n}/7",
    week_insight_cross_project: "여러 프로젝트: 5회 이상 호출된 작업 디렉터리 {n}개",
    about_title: "FluxMirror 소개",
    about_blurb: "FluxMirror는 로컬에서 AI 코딩 에이전트의 활동을 기록하는 관측 도구입니다. Claude Code, Gemini CLI, Qwen Code, (선택적으로) Claude Desktop 의 MCP 트래픽까지 하나의 SQLite 데이터베이스에 모아두고, 그 데이터를 일별/주간/에이전트별 보고서로 보여줍니다.",
    about_commands_heading: "사용 가능한 명령",
    about_paths_heading: "데이터 위치",
    about_db_label: "데이터베이스",
    about_log_label: "로그 파일",
    compare_title: "오늘 vs 어제",
    compare_no_data: "오늘과 어제 모두 활동이 부족해 비교할 수 없습니다.",
    compare_columns: ["항목", "오늘", "어제", "Δ"],
    compare_metric_labels: [
        "총 호출",
        "편집",
        "읽기",
        "셸 명령",
        "고유 파일",
        "고유 작업 디렉터리",
    ],
    compare_insight_calls_trend: "호출: 오늘이 어제 대비 {pct}% {direction}.",
    compare_insight_both_quiet: "오늘과 어제 모두 활동이 적습니다.",
    compare_insight_only_today: "어제는 활동 없음, 오늘 호출 {n}회.",
    compare_insight_only_yesterday: "오늘 아직 활동 없음, 어제 호출 {n}회.",
    compare_word_up: "증가",
    compare_word_down: "감소",
    init_demo_row_inserted: "데모 행을 추가했습니다. 이제 /fluxmirror:today 가 바로 의미 있는 리포트를 반환합니다. 제거하려면: fluxmirror sqlite \"DELETE FROM agent_events WHERE agent='setup'\"",
    html_title: "fluxmirror 주간 다이제스트",
    html_heatmap_heading: "활동 히트맵",
    html_top_files_heading: "상위 편집 파일",
    html_top_shells_heading: "상위 셸 명령",
    html_per_agent_heading: "에이전트별 요약",
    html_summary_heading: "요약",
    html_dow_labels: ["월", "화", "수", "목", "금", "토", "일"],
    html_agent_columns: ["에이전트", "호출", "세션", "활동 일수", "주요 도구"],
    html_item_count_columns: ["항목", "횟수"],
    html_hour_axis_label: "시간대 (00 - 23)",
    html_summary_template: "지난 7일간 {agents}개 에이전트에서 총 {calls}회 호출이 기록되었습니다. 가장 활발한 요일은 {dow}로 {dow_calls}회였고, 가장 많이 사용된 도구는 {tool}({tool_calls}회)입니다.",
    html_summary_empty: "지난 7일간 에이전트 활동이 기록되지 않았습니다.",
    html_footer_generated: "fluxmirror 생성",
};

const JAPANESE: LangPack = LangPack {
    agents_title: "エージェント一覧",
    agents_columns: [
        "エージェント",
        "呼び出し",
        "セッション",
        "稼働日数",
        "主な道具",
        "書込/読取/シェル",
    ],
    agents_no_activity: "過去7日間にエージェントの活動はありません。",
    insights_heading: "インサイト",
    tz_label: "過去7日間",
    range_label: "期間",
    insight_busiest: "{agent} が最多 — 呼び出し {n} 回。",
    insight_one_shot: "{agent} は単発セッション ({n} 回呼び出し)。",
    insight_write_heavy: "{agent} は書込中心 (書込 {pct}%)。",
    today_title: "本日の作業",
    today_activity_heading: "活動統計",
    today_files_edited_heading: "編集したファイル",
    today_files_read_heading: "閲覧のみのファイル",
    today_shell_heading: "シェルコマンド",
    today_cwds_heading: "作業ディレクトリ",
    today_mcp_heading: "MCP メソッド",
    today_tool_mix_heading: "ツール構成",
    today_hours_heading: "時間帯分布",
    today_insights_heading: "インサイト",
    today_no_data: "本日の活動は少なめです。",
    today_insight_busiest_hour: "最も活発な時間: {hour} ({n} 回)",
    today_insight_edit_read_ratio: "編集/読取 比率: {ratio}",
    today_insight_multi_project: "マルチプロジェクト日: 5 回以上呼ばれた作業ディレクトリ {n} 個",
    today_columns_calls_sessions: ["エージェント", "呼び出し", "セッション"],
    today_columns_file_tool_count: ["パス", "ツール", "回数"],
    today_columns_path_count: ["パス", "回数"],
    today_columns_time_command: ["時刻", "コマンド"],
    today_columns_method_count: ["メソッド", "回数"],
    today_columns_tool_count: ["ツール", "回数"],
    yesterday_title: "昨日の作業",
    yesterday_no_data: "昨日の活動はありませんでした。",
    week_title: "過去7日間",
    week_no_data: "今週の活動は少なめです。",
    week_daily_totals_heading: "日次合計",
    week_day_distribution_heading: "曜日分布",
    week_columns_date_calls: ["日付", "呼び出し"],
    week_insight_top_day: "最も活発な日: {date} ({n} 回)",
    week_insight_active_days: "活動日数: {n}/7",
    week_insight_cross_project: "複数プロジェクト: 5 回以上呼ばれた作業ディレクトリ {n} 個",
    about_title: "FluxMirror について",
    about_blurb: "FluxMirror は AI コーディングエージェントの動作をローカルで記録するツールです。Claude Code、Gemini CLI、Qwen Code、(任意で) Claude Desktop の MCP トラフィックを単一の SQLite データベースに集約し、そのデータを日次・週次・エージェント別レポートとして提示します。",
    about_commands_heading: "利用可能なコマンド",
    about_paths_heading: "データの保存場所",
    about_db_label: "データベース",
    about_log_label: "ログファイル",
    compare_title: "本日 vs 昨日",
    compare_no_data: "本日と昨日の活動が少なく比較できません。",
    compare_columns: ["項目", "本日", "昨日", "Δ"],
    compare_metric_labels: [
        "総呼び出し",
        "編集",
        "読取",
        "シェルコマンド",
        "ユニークファイル",
        "ユニーク作業ディレクトリ",
    ],
    compare_insight_calls_trend: "呼び出し: 昨日比 {pct}% {direction}。",
    compare_insight_both_quiet: "本日と昨日はどちらも活動が少なめです。",
    compare_insight_only_today: "昨日は活動なし。本日 {n} 回。",
    compare_insight_only_yesterday: "本日まだ活動なし。昨日は {n} 回。",
    compare_word_up: "増加",
    compare_word_down: "減少",
    init_demo_row_inserted: "デモ行を追加しました。これで /fluxmirror:today が即座に意味のあるレポートを返します。削除するには: fluxmirror sqlite \"DELETE FROM agent_events WHERE agent='setup'\"",
    html_title: "fluxmirror 週次ダイジェスト",
    html_heatmap_heading: "アクティビティ ヒートマップ",
    html_top_files_heading: "編集が多いファイル",
    html_top_shells_heading: "シェルコマンド",
    html_per_agent_heading: "エージェント別サマリ",
    html_summary_heading: "サマリ",
    html_dow_labels: ["月", "火", "水", "木", "金", "土", "日"],
    html_agent_columns: ["エージェント", "呼び出し", "セッション", "稼働日数", "主な道具"],
    html_item_count_columns: ["項目", "回数"],
    html_hour_axis_label: "時間帯 (00 - 23)",
    html_summary_template: "過去 7 日間で {agents} 個のエージェントから合計 {calls} 回の呼び出しが記録されました。最も活発な曜日は {dow} で {dow_calls} 回、最も使われたツールは {tool} ({tool_calls} 回) です。",
    html_summary_empty: "過去 7 日間にエージェントの活動はありません。",
    html_footer_generated: "生成: fluxmirror",
};

const CHINESE: LangPack = LangPack {
    agents_title: "代理一览",
    agents_columns: [
        "代理",
        "调用次数",
        "会话",
        "活动天数",
        "主要工具",
        "写/读/壳",
    ],
    agents_no_activity: "过去7天内无代理活动。",
    insights_heading: "洞察",
    tz_label: "过去7天",
    range_label: "区间",
    insight_busiest: "{agent} 最为活跃 — 调用 {n} 次。",
    insight_one_shot: "{agent} 仅有单次会话 ({n} 次调用)。",
    insight_write_heavy: "{agent} 以写入为主 (写入 {pct}%)。",
    today_title: "今日工作",
    today_activity_heading: "活动统计",
    today_files_edited_heading: "已编辑文件",
    today_files_read_heading: "仅读取文件",
    today_shell_heading: "Shell 命令",
    today_cwds_heading: "工作目录",
    today_mcp_heading: "MCP 方法",
    today_tool_mix_heading: "工具分布",
    today_hours_heading: "时间分布",
    today_insights_heading: "洞察",
    today_no_data: "今日活动较少。",
    today_insight_busiest_hour: "最活跃时段: {hour} (调用 {n} 次)",
    today_insight_edit_read_ratio: "编辑/读取 比率: {ratio}",
    today_insight_multi_project: "多项目日: 至少 5 次调用的工作目录 {n} 个",
    today_columns_calls_sessions: ["代理", "调用", "会话"],
    today_columns_file_tool_count: ["路径", "工具", "次数"],
    today_columns_path_count: ["路径", "次数"],
    today_columns_time_command: ["时间", "命令"],
    today_columns_method_count: ["方法", "次数"],
    today_columns_tool_count: ["工具", "次数"],
    yesterday_title: "昨日的工作",
    yesterday_no_data: "昨日无活动。",
    week_title: "最近 7 天",
    week_no_data: "本周活动较少。",
    week_daily_totals_heading: "每日合计",
    week_day_distribution_heading: "按日分布",
    week_columns_date_calls: ["日期", "调用"],
    week_insight_top_day: "最活跃日: {date} (调用 {n} 次)",
    week_insight_active_days: "活动天数: {n}/7",
    week_insight_cross_project: "多项目: 至少 5 次调用的工作目录 {n} 个",
    about_title: "关于 FluxMirror",
    about_blurb: "FluxMirror 是一款本地化的 AI 编码代理观测工具。它将 Claude Code、Gemini CLI、Qwen Code 以及 (可选) Claude Desktop 的 MCP 流量统一记录到单个 SQLite 数据库,并将数据呈现为按日、按周、按代理划分的报告。",
    about_commands_heading: "可用命令",
    about_paths_heading: "数据位置",
    about_db_label: "数据库",
    about_log_label: "日志文件",
    compare_title: "今日 vs 昨日",
    compare_no_data: "今日和昨日活动均较少,无法比较。",
    compare_columns: ["指标", "今日", "昨日", "Δ"],
    compare_metric_labels: [
        "总调用",
        "编辑",
        "读取",
        "Shell 命令",
        "独立文件",
        "独立工作目录",
    ],
    compare_insight_calls_trend: "调用: 今日相比昨日 {pct}% {direction}。",
    compare_insight_both_quiet: "今日和昨日活动均较少。",
    compare_insight_only_today: "昨日无活动,今日 {n} 次调用。",
    compare_insight_only_yesterday: "今日尚无活动,昨日 {n} 次调用。",
    compare_word_up: "上升",
    compare_word_down: "下降",
    init_demo_row_inserted: "已插入演示行,/fluxmirror:today 现在会立即返回有意义的报告。移除方式: fluxmirror sqlite \"DELETE FROM agent_events WHERE agent='setup'\"",
    html_title: "fluxmirror 每周摘要",
    html_heatmap_heading: "活动热力图",
    html_top_files_heading: "编辑最多的文件",
    html_top_shells_heading: "Shell 命令",
    html_per_agent_heading: "代理摘要",
    html_summary_heading: "摘要",
    html_dow_labels: ["一", "二", "三", "四", "五", "六", "日"],
    html_agent_columns: ["代理", "调用", "会话", "活动天数", "主要工具"],
    html_item_count_columns: ["项目", "次数"],
    html_hour_axis_label: "时段 (00 - 23)",
    html_summary_template: "过去 7 天 {agents} 个代理共记录 {calls} 次调用。最活跃的是 {dow},共 {dow_calls} 次;使用最多的工具是 {tool}({tool_calls} 次)。",
    html_summary_empty: "过去 7 天没有记录到代理活动。",
    html_footer_generated: "由 fluxmirror 生成",
};

/// Resolve a language code (or canonical name) to a `LangPack`.
///
/// Accepts the same lowercase canonical names the user-facing config
/// stores (`english`, `korean`, `japanese`, `chinese`) plus the locale
/// shortcuts `en`, `ko`, `kr`, `ja`, `zh`. Unknown values fall back to
/// English to match the `Language::from_locale` upstream behaviour.
pub fn pack(lang: &str) -> &'static LangPack {
    match lang.to_ascii_lowercase().as_str() {
        "korean" | "ko" | "kr" => &KOREAN,
        "japanese" | "ja" => &JAPANESE,
        "chinese" | "zh" => &CHINESE,
        _ => &ENGLISH,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_is_the_fallback() {
        let p = pack("garbage");
        assert_eq!(p.agents_title, "Agent Roster");
    }

    #[test]
    fn known_codes_resolve() {
        assert_eq!(pack("korean").agents_title, "에이전트 명세");
        assert_eq!(pack("ko").agents_title, "에이전트 명세");
        assert_eq!(pack("japanese").agents_title, "エージェント一覧");
        assert_eq!(pack("ja").agents_title, "エージェント一覧");
        assert_eq!(pack("chinese").agents_title, "代理一览");
        assert_eq!(pack("zh").agents_title, "代理一览");
        assert_eq!(pack("english").agents_title, "Agent Roster");
        assert_eq!(pack("en").agents_title, "Agent Roster");
    }

    #[test]
    fn each_pack_has_six_columns() {
        for code in ["english", "korean", "japanese", "chinese"] {
            assert_eq!(pack(code).agents_columns.len(), 6);
            for col in pack(code).agents_columns {
                assert!(!col.is_empty(), "{code} has an empty column header");
            }
        }
    }
}
