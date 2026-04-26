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
