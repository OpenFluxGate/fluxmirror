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
