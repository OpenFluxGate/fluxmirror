// Self-contained HTML renderer for the M5.4 day-shaped digest cards.
//
// Three render entry points share this module:
//
//   - `render_today_card` / `render_yesterday_card` — full single-day
//     snapshot for the everyone-scope today and yesterday reports.
//   - `render_agent_card` — same shape as the day card but title carries
//     a leading "{agent}:" prefix so the user knows the scope.
//   - `render_compare_card` — two-column today-vs-yesterday split-pane
//     plus a Δ row per metric.
//
// The card is a complete `<!DOCTYPE html>` document. CSS is appended
// inline on top of the existing weekly-card palette so a bundle install
// drops the same look-and-feel across every report. No JS, no external
// fonts, no images.

use chrono::NaiveDate;
use fluxmirror_core::report::LangPack;

use super::day::{busiest_hour, DayStats};
use super::html::{html_escape, replace_all};

/// Width of the trailing color block in the hour-distribution row. The
/// renderer always emits 24 cells; cells with zero events stay neutral
/// gray, matching the heatmap palette on the weekly card.
const HOUR_CELL_GAP: u32 = 24;

/// Bucket a per-cell call count to one of five color steps. Mirrors
/// `html::heatmap_color` so the day card and the weekly heatmap share
/// the same palette without duplicating the constants module-private.
fn hour_color(count: u64) -> &'static str {
    match count {
        0 => "#f3f4f6",
        1..=2 => "#dbeafe",
        3..=5 => "#93c5fd",
        6..=20 => "#3b82f6",
        _ => "#1d4ed8",
    }
}

/// Which day-shaped card to render. Picks the right title and empty
/// message from the lang pack without growing a parameter list.
#[derive(Copy, Clone)]
pub enum DayCardKind {
    Today,
    Yesterday,
}

impl DayCardKind {
    fn title<'a>(self, lp: &'a LangPack) -> &'a str {
        match self {
            DayCardKind::Today => lp.html_today_title,
            DayCardKind::Yesterday => lp.html_yesterday_title,
        }
    }
    fn empty<'a>(self, lp: &'a LangPack) -> &'a str {
        match self {
            DayCardKind::Today => lp.html_today_no_data,
            DayCardKind::Yesterday => lp.html_yesterday_no_data,
        }
    }
}

/// Render the full HTML document for a single day's report scoped to
/// every agent. The card always emits a complete document — caller
/// hands it to the user's writer (file or stdout).
pub(crate) fn render_day_card(
    day: &DayStats,
    date: NaiveDate,
    tz_label: &str,
    kind: DayCardKind,
    lp: &LangPack,
    generated_footer: &str,
) -> String {
    let title = kind.title(lp);
    let header_line = format!("{} ({}, {})", title, date.format("%Y-%m-%d"), tz_label);
    render_day_document(day, &header_line, title, kind.empty(lp), lp, generated_footer)
}

/// Same shape as `render_day_card` but with the agent prefix layered into
/// the title and the empty message. Used by the agent subcommand for the
/// today / yesterday periods.
pub(crate) fn render_agent_day_card(
    day: &DayStats,
    date: NaiveDate,
    tz_label: &str,
    kind: DayCardKind,
    agent: &str,
    lp: &LangPack,
    generated_footer: &str,
) -> String {
    let period_title = kind.title(lp);
    let title = replace_all(
        lp.html_agent_card_title_template,
        &[("agent", agent), ("title", period_title)],
    );
    let header_line = format!("{} ({}, {})", title, date.format("%Y-%m-%d"), tz_label);
    let empty_line = format!("{}: {}", agent, kind.empty(lp));
    render_day_document(day, &header_line, &title, &empty_line, lp, generated_footer)
}

/// Input for `render_agent_week_card` — gathered by the caller from
/// the per-agent week aggregate plus the date window.
pub(crate) struct AgentWeekHtmlInput<'a> {
    pub agent: &'a str,
    pub range_start: NaiveDate,
    pub range_end: NaiveDate,
    pub tz_label: &'a str,
    pub total_calls: u64,
    pub sessions: u64,
    /// `(date, calls)` rows in chronological order.
    pub daily_calls: &'a [(NaiveDate, u64)],
    pub top_files: &'a [(String, u64)],
    pub top_reads: &'a [(String, u64)],
    pub tool_mix: &'a [(String, u64)],
    pub cwds: &'a [(String, u64)],
}

/// Render the agent card scoped to a week. Reuses the daily-card layout
/// primitives (per-agent table dropped, top-files / top-shells trimmed
/// to top 10) so the visual shape stays consistent.
pub(crate) fn render_agent_week_card(
    stats: &AgentWeekHtmlInput<'_>,
    lp: &LangPack,
    generated_footer: &str,
) -> String {
    let title = replace_all(
        lp.html_agent_card_title_template,
        &[("agent", stats.agent), ("title", lp.week_title)],
    );
    let header_line = format!(
        "{} ({} ~ {}, {})",
        title,
        stats.range_start.format("%Y-%m-%d"),
        stats.range_end.format("%Y-%m-%d"),
        stats.tz_label,
    );
    let empty_line = format!("{}: {}", stats.agent, lp.week_no_data);
    if stats.total_calls == 0 {
        return render_empty_card(&title, &header_line, &empty_line, generated_footer);
    }

    let mut out = String::with_capacity(8 * 1024);
    push_doc_open(&mut out, &title);
    push_card_header(&mut out, &title, &header_line);

    // Activity stats block (single row).
    out.push_str("<section class=\"day-stats\">\n");
    out.push_str("<ul>\n");
    out.push_str(&format!(
        "<li><strong>{}:</strong> {}</li>\n",
        html_escape(lp.html_table_calls),
        stats.total_calls
    ));
    out.push_str(&format!(
        "<li><strong>{}:</strong> {}</li>\n",
        html_escape(lp.today_columns_calls_sessions[2]),
        stats.sessions
    ));
    out.push_str("</ul>\n");
    out.push_str("</section>\n");

    // Per-day totals.
    if !stats.daily_calls.is_empty() {
        out.push_str("<section class=\"daily-totals\">\n");
        out.push_str(&format!(
            "<h2>{}</h2>\n",
            html_escape(lp.week_daily_totals_heading)
        ));
        out.push_str("<table>\n<thead><tr>\n");
        out.push_str(&format!(
            "<th>{}</th><th class=\"num\">{}</th>\n",
            html_escape(lp.week_columns_date_calls[0]),
            html_escape(lp.week_columns_date_calls[1]),
        ));
        out.push_str("</tr></thead>\n<tbody>\n");
        for (date, calls) in stats.daily_calls {
            out.push_str(&format!(
                "<tr><td>{} ({})</td><td class=\"num\">{}</td></tr>\n",
                html_escape(&date.format("%Y-%m-%d").to_string()),
                html_escape(&date.format("%a").to_string()),
                calls
            ));
        }
        out.push_str("</tbody>\n</table>\n</section>\n");
    }

    push_top_files_section(&mut out, lp.today_files_edited_heading, stats.top_files, lp);
    push_top_files_section(&mut out, lp.html_section_files_read, stats.top_reads, lp);
    push_tool_mix_section(&mut out, stats.tool_mix, lp);
    push_cwds_section(&mut out, stats.cwds, lp);

    push_card_footer(&mut out, generated_footer);
    out
}

/// Render the body of a day-shaped card given a precomputed title and
/// header line. Also handles the empty-data branch.
fn render_day_document(
    day: &DayStats,
    header_line: &str,
    title: &str,
    empty_line: &str,
    lp: &LangPack,
    generated_footer: &str,
) -> String {
    if day.total_events == 0 {
        return render_empty_card(title, header_line, empty_line, generated_footer);
    }

    let mut out = String::with_capacity(8 * 1024);
    push_doc_open(&mut out, title);
    push_card_header(&mut out, title, header_line);

    push_per_agent_section(&mut out, day, lp);
    push_hour_strip(&mut out, day, lp);
    push_files_edited_section(&mut out, day, lp);
    push_files_read_section(&mut out, day, lp);
    push_shell_timeline_section(&mut out, day, lp);
    push_cwds_section_day(&mut out, day, lp);
    push_tool_mix_section_day(&mut out, day, lp);
    push_day_summary_section(&mut out, day, lp);

    push_card_footer(&mut out, generated_footer);
    out
}

fn render_empty_card(
    title: &str,
    header_line: &str,
    empty_line: &str,
    generated_footer: &str,
) -> String {
    let mut out = String::with_capacity(2 * 1024);
    push_doc_open(&mut out, title);
    push_card_header(&mut out, title, header_line);
    out.push_str("<section class=\"empty-day\">\n");
    out.push_str(&format!("<p>{}</p>\n", html_escape(empty_line)));
    out.push_str("</section>\n");
    push_card_footer(&mut out, generated_footer);
    out
}

fn push_doc_open(out: &mut String, title: &str) {
    out.push_str("<!DOCTYPE html>\n");
    out.push_str("<html lang=\"en\">\n");
    out.push_str("<head>\n");
    out.push_str("<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str(&format!("<title>{}</title>\n", html_escape(title)));
    out.push_str("<style>\n");
    out.push_str(BASE_CSS);
    out.push_str(EXTRA_CSS);
    out.push_str("</style>\n");
    out.push_str("</head>\n");
    out.push_str("<body>\n");
    out.push_str("<main class=\"card\">\n");
}

fn push_card_header(out: &mut String, title: &str, header_line: &str) {
    out.push_str("<header class=\"hd\">\n");
    out.push_str(&format!("<h1>{}</h1>\n", html_escape(title)));
    out.push_str(&format!(
        "<p class=\"sub\">{}</p>\n",
        html_escape(header_line)
    ));
    out.push_str("</header>\n");
}

fn push_card_footer(out: &mut String, generated_footer: &str) {
    out.push_str(&format!(
        "<footer class=\"ft\">{}</footer>\n",
        html_escape(generated_footer)
    ));
    out.push_str("</main>\n</body>\n</html>\n");
}

fn push_per_agent_section(out: &mut String, day: &DayStats, lp: &LangPack) {
    out.push_str("<section class=\"ag\">\n");
    out.push_str(&format!(
        "<h2>{}</h2>\n",
        html_escape(lp.today_activity_heading)
    ));
    if day.agents.is_empty() {
        out.push_str("<p class=\"empty\">&mdash;</p>\n</section>\n");
        return;
    }
    out.push_str("<table>\n<thead><tr>\n");
    let cols = lp.today_columns_calls_sessions;
    out.push_str(&format!(
        "<th>{}</th><th class=\"num\">{}</th><th class=\"num\">{}</th>\n",
        html_escape(cols[0]),
        html_escape(cols[1]),
        html_escape(cols[2])
    ));
    out.push_str("</tr></thead>\n<tbody>\n");
    let mut rows: Vec<(&String, &super::day::AgentRow)> = day.agents.iter().collect();
    rows.sort_by(|a, b| b.1.calls.cmp(&a.1.calls).then_with(|| a.0.cmp(b.0)));
    for (agent, row) in rows {
        out.push_str(&format!(
            "<tr><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>\n",
            html_escape(agent),
            row.calls,
            row.sessions.len()
        ));
    }
    out.push_str("</tbody>\n</table>\n</section>\n");
}

fn push_hour_strip(out: &mut String, day: &DayStats, lp: &LangPack) {
    let max = *day.hours.iter().max().unwrap_or(&0);
    if max == 0 {
        return;
    }
    out.push_str("<section class=\"hr-strip\">\n");
    out.push_str(&format!(
        "<h2>{}</h2>\n",
        html_escape(lp.html_section_hours)
    ));
    out.push_str(&format!(
        "<p class=\"axis\">{}</p>\n",
        html_escape(lp.html_hour_axis_label)
    ));
    out.push_str("<div class=\"hour-grid\">\n");
    for h in 0..HOUR_CELL_GAP {
        out.push_str(&format!("<div class=\"hcell hdr\">{:02}</div>\n", h));
    }
    for h in 0..24usize {
        let n = day.hours[h];
        let bg = hour_color(n);
        out.push_str(&format!(
            "<div class=\"hcell\" style=\"background:{};\" data-count=\"{}\" title=\"{:02}:00 - {} calls\"></div>\n",
            bg, n, h, n
        ));
    }
    out.push_str("</div>\n</section>\n");
}

fn push_files_edited_section(out: &mut String, day: &DayStats, lp: &LangPack) {
    if day.files_edited.is_empty() {
        return;
    }
    out.push_str("<section class=\"tt\">\n");
    out.push_str(&format!(
        "<h2>{}</h2>\n",
        html_escape(lp.today_files_edited_heading)
    ));
    let mut rows: Vec<(&(String, String), &u64)> = day.files_edited.iter().collect();
    rows.sort_by(|a, b| {
        b.1.cmp(a.1)
            .then_with(|| a.0 .0.cmp(&b.0 .0))
            .then_with(|| a.0 .1.cmp(&b.0 .1))
    });
    out.push_str("<table>\n<thead><tr>\n");
    let cols = lp.today_columns_file_tool_count;
    out.push_str(&format!(
        "<th>{}</th><th>{}</th><th class=\"num\">{}</th>\n",
        html_escape(cols[0]),
        html_escape(cols[1]),
        html_escape(cols[2])
    ));
    out.push_str("</tr></thead>\n<tbody>\n");
    for ((path, tool), n) in rows.into_iter().take(10) {
        out.push_str(&format!(
            "<tr><td class=\"item\">{}</td><td>{}</td><td class=\"num\">{}</td></tr>\n",
            html_escape(path),
            html_escape(tool),
            n
        ));
    }
    out.push_str("</tbody>\n</table>\n</section>\n");
}

fn push_files_read_section(out: &mut String, day: &DayStats, lp: &LangPack) {
    if day.files_read.is_empty() {
        return;
    }
    out.push_str("<section class=\"tt\">\n");
    out.push_str(&format!(
        "<h2>{}</h2>\n",
        html_escape(lp.html_section_files_read)
    ));
    let mut rows: Vec<(&String, &u64)> = day.files_read.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    out.push_str("<table>\n<thead><tr>\n");
    let cols = lp.today_columns_path_count;
    out.push_str(&format!(
        "<th>{}</th><th class=\"num\">{}</th>\n",
        html_escape(cols[0]),
        html_escape(cols[1])
    ));
    out.push_str("</tr></thead>\n<tbody>\n");
    for (path, n) in rows.into_iter().take(10) {
        out.push_str(&format!(
            "<tr><td class=\"item\">{}</td><td class=\"num\">{}</td></tr>\n",
            html_escape(path),
            n
        ));
    }
    out.push_str("</tbody>\n</table>\n</section>\n");
}

fn push_shell_timeline_section(out: &mut String, day: &DayStats, lp: &LangPack) {
    if day.shells.is_empty() {
        return;
    }
    out.push_str("<section class=\"tt\">\n");
    out.push_str(&format!(
        "<h2>{}</h2>\n",
        html_escape(lp.html_section_shell_timeline)
    ));
    out.push_str("<table>\n<thead><tr>\n");
    let cols = lp.today_columns_time_command;
    out.push_str(&format!(
        "<th>{}</th><th>{}</th>\n",
        html_escape(cols[0]),
        html_escape(cols[1])
    ));
    out.push_str("</tr></thead>\n<tbody>\n");
    for s in day.shells.iter().take(20) {
        out.push_str(&format!(
            "<tr><td class=\"num\">{}</td><td class=\"item\">{}</td></tr>\n",
            html_escape(&s.time_local),
            html_escape(&s.detail)
        ));
    }
    out.push_str("</tbody>\n</table>\n</section>\n");
}

fn push_cwds_section_day(out: &mut String, day: &DayStats, lp: &LangPack) {
    if day.cwds.is_empty() {
        return;
    }
    push_count_table(
        out,
        lp.html_section_cwds,
        lp.today_columns_path_count,
        &day.cwds,
        10,
    );
}

fn push_tool_mix_section_day(out: &mut String, day: &DayStats, lp: &LangPack) {
    if day.tool_mix.is_empty() {
        return;
    }
    push_count_table(
        out,
        lp.html_section_tool_mix,
        lp.today_columns_tool_count,
        &day.tool_mix,
        usize::MAX,
    );
}

fn push_count_table(
    out: &mut String,
    heading: &str,
    cols: [&'static str; 2],
    data: &std::collections::HashMap<String, u64>,
    limit: usize,
) {
    out.push_str("<section class=\"tt\">\n");
    out.push_str(&format!("<h2>{}</h2>\n", html_escape(heading)));
    out.push_str("<table>\n<thead><tr>\n");
    out.push_str(&format!(
        "<th>{}</th><th class=\"num\">{}</th>\n",
        html_escape(cols[0]),
        html_escape(cols[1])
    ));
    out.push_str("</tr></thead>\n<tbody>\n");
    let mut rows: Vec<(&String, &u64)> = data.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    for (k, n) in rows.into_iter().take(limit) {
        out.push_str(&format!(
            "<tr><td class=\"item\">{}</td><td class=\"num\">{}</td></tr>\n",
            html_escape(k),
            n
        ));
    }
    out.push_str("</tbody>\n</table>\n</section>\n");
}

fn push_top_files_section(
    out: &mut String,
    heading: &str,
    rows: &[(String, u64)],
    lp: &LangPack,
) {
    if rows.is_empty() {
        return;
    }
    out.push_str("<section class=\"tt\">\n");
    out.push_str(&format!("<h2>{}</h2>\n", html_escape(heading)));
    out.push_str("<table>\n<thead><tr>\n");
    let cols = lp.today_columns_path_count;
    out.push_str(&format!(
        "<th>{}</th><th class=\"num\">{}</th>\n",
        html_escape(cols[0]),
        html_escape(cols[1])
    ));
    out.push_str("</tr></thead>\n<tbody>\n");
    for (path, n) in rows.iter().take(10) {
        out.push_str(&format!(
            "<tr><td class=\"item\">{}</td><td class=\"num\">{}</td></tr>\n",
            html_escape(path),
            n
        ));
    }
    out.push_str("</tbody>\n</table>\n</section>\n");
}

fn push_tool_mix_section(
    out: &mut String,
    rows: &[(String, u64)],
    lp: &LangPack,
) {
    if rows.is_empty() {
        return;
    }
    out.push_str("<section class=\"tt\">\n");
    out.push_str(&format!(
        "<h2>{}</h2>\n",
        html_escape(lp.html_section_tool_mix)
    ));
    out.push_str("<table>\n<thead><tr>\n");
    let cols = lp.today_columns_tool_count;
    out.push_str(&format!(
        "<th>{}</th><th class=\"num\">{}</th>\n",
        html_escape(cols[0]),
        html_escape(cols[1])
    ));
    out.push_str("</tr></thead>\n<tbody>\n");
    for (tool, n) in rows {
        out.push_str(&format!(
            "<tr><td>{}</td><td class=\"num\">{}</td></tr>\n",
            html_escape(tool),
            n
        ));
    }
    out.push_str("</tbody>\n</table>\n</section>\n");
}

fn push_cwds_section(out: &mut String, rows: &[(String, u64)], lp: &LangPack) {
    if rows.is_empty() {
        return;
    }
    out.push_str("<section class=\"tt\">\n");
    out.push_str(&format!(
        "<h2>{}</h2>\n",
        html_escape(lp.html_section_cwds)
    ));
    out.push_str("<table>\n<thead><tr>\n");
    let cols = lp.today_columns_path_count;
    out.push_str(&format!(
        "<th>{}</th><th class=\"num\">{}</th>\n",
        html_escape(cols[0]),
        html_escape(cols[1])
    ));
    out.push_str("</tr></thead>\n<tbody>\n");
    for (path, n) in rows.iter().take(10) {
        out.push_str(&format!(
            "<tr><td class=\"item\">{}</td><td class=\"num\">{}</td></tr>\n",
            html_escape(path),
            n
        ));
    }
    out.push_str("</tbody>\n</table>\n</section>\n");
}

fn push_day_summary_section(out: &mut String, day: &DayStats, lp: &LangPack) {
    out.push_str("<section class=\"sm\">\n");
    out.push_str(&format!(
        "<h2>{}</h2>\n",
        html_escape(lp.html_summary_heading)
    ));
    let body = if day.total_events == 0 {
        lp.html_day_summary_empty.to_string()
    } else {
        let (busiest_h, busiest_n) = busiest_hour(&day.hours).unwrap_or((0, 0));
        let (top_tool, top_n) = day
            .tool_mix
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
            .map(|(t, n)| (t.clone(), *n))
            .unwrap_or_else(|| ("-".to_string(), 0));
        let calls = day.total_events.to_string();
        let agents = day.agents.len().to_string();
        let hour_label = format!("{:02}:00", busiest_h);
        let busiest_n_str = busiest_n.to_string();
        let top_n_str = top_n.to_string();
        replace_all(
            lp.html_day_summary_template,
            &[
                ("calls", calls.as_str()),
                ("agents", agents.as_str()),
                ("hour", hour_label.as_str()),
                ("hour_calls", busiest_n_str.as_str()),
                ("tool", top_tool.as_str()),
                ("tool_calls", top_n_str.as_str()),
            ],
        )
    };
    out.push_str(&format!("<p>{}</p>\n", html_escape(&body)));
    out.push_str("</section>\n");
}

/// Compute per-day metrics for the compare card. Mirrors the human-mode
/// `Metrics::from_day` shape but exported here as a standalone struct so
/// the compare HTML renderer doesn't pull in the human renderer.
#[derive(Debug, Default, Copy, Clone)]
pub(crate) struct CompareMetrics {
    pub total: u64,
    pub edits: u64,
    pub reads: u64,
    pub shells: u64,
    pub distinct_files: u64,
    pub distinct_cwds: u64,
}

impl CompareMetrics {
    pub(crate) fn from_day(day: &DayStats) -> Self {
        CompareMetrics {
            total: day.total_events,
            edits: day.writes_total,
            reads: day.reads_total,
            shells: day.shells.len() as u64,
            distinct_files: day.distinct_files.len() as u64,
            distinct_cwds: day.cwds.len() as u64,
        }
    }
    fn pairs(&self) -> [u64; 6] {
        [
            self.total,
            self.edits,
            self.reads,
            self.shells,
            self.distinct_files,
            self.distinct_cwds,
        ]
    }
}

/// Render the compare HTML card. Two-column layout (today | yesterday)
/// with a Δ row per metric. Trailing summary block surfaces the
/// localised insight text used by the human compare report.
pub(crate) fn render_compare_card(
    today: &DayStats,
    yesterday: &DayStats,
    today_date: NaiveDate,
    yest_date: NaiveDate,
    tz_label: &str,
    lp: &LangPack,
    generated_footer: &str,
) -> String {
    let title = lp.html_compare_title;
    let header_line = format!(
        "{} ({} vs {}, {})",
        title,
        today_date.format("%Y-%m-%d"),
        yest_date.format("%Y-%m-%d"),
        tz_label
    );

    let t = CompareMetrics::from_day(today);
    let y = CompareMetrics::from_day(yesterday);

    if t.total == 0 && y.total == 0 {
        return render_empty_card(title, &header_line, lp.html_compare_no_data, generated_footer);
    }

    let mut out = String::with_capacity(6 * 1024);
    push_doc_open(&mut out, title);
    push_card_header(&mut out, title, &header_line);

    out.push_str("<section class=\"compare-grid\">\n");
    out.push_str("<table>\n<thead><tr>\n");
    out.push_str(&format!(
        "<th>{}</th><th class=\"num\">{}</th><th class=\"num\">{}</th><th class=\"num\">{}</th>\n",
        html_escape(lp.compare_columns[0]),
        html_escape(lp.html_compare_today_label),
        html_escape(lp.html_compare_yesterday_label),
        html_escape(lp.html_compare_delta_label),
    ));
    out.push_str("</tr></thead>\n<tbody>\n");
    let labels = lp.compare_metric_labels;
    let t_pairs = t.pairs();
    let y_pairs = y.pairs();
    for (i, label) in labels.iter().enumerate() {
        let (today_v, yest_v) = (t_pairs[i], y_pairs[i]);
        let delta = format_delta_html(today_v, yest_v);
        out.push_str(&format!(
            "<tr><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>\n",
            html_escape(label),
            today_v,
            yest_v,
            delta
        ));
    }
    out.push_str("</tbody>\n</table>\n</section>\n");

    out.push_str("<section class=\"sm\">\n");
    out.push_str(&format!(
        "<h2>{}</h2>\n",
        html_escape(lp.today_insights_heading)
    ));
    let body = compare_summary_line(t.total, y.total, lp);
    out.push_str(&format!("<p>{}</p>\n", html_escape(&body)));
    out.push_str("</section>\n");

    push_card_footer(&mut out, generated_footer);
    out
}

fn compare_summary_line(today_total: u64, yest_total: u64, lp: &LangPack) -> String {
    match (today_total, yest_total) {
        (0, 0) => lp.compare_insight_both_quiet.to_string(),
        (n, 0) => lp.compare_insight_only_today.replace("{n}", &n.to_string()),
        (0, n) => lp.compare_insight_only_yesterday.replace("{n}", &n.to_string()),
        (t, y) => {
            let diff = t as i128 - y as i128;
            let pct = (diff * 100) / y as i128;
            let direction = if pct >= 0 {
                lp.compare_word_up
            } else {
                lp.compare_word_down
            };
            lp.compare_insight_calls_trend
                .replace("{direction}", direction)
                .replace("{pct}", &pct.unsigned_abs().to_string())
        }
    }
}

/// Format a Δ cell (HTML-friendly). Uses the same arrow rules as the
/// human compare report so the cards stay in step.
fn format_delta_html(today: u64, yest: u64) -> String {
    if yest == 0 && today == 0 {
        return "0%".to_string();
    }
    if yest == 0 {
        return "n/a".to_string();
    }
    let diff = today as i128 - yest as i128;
    let pct = (diff * 100) / yest as i128;
    let arrow = if pct.unsigned_abs() >= 50 {
        if pct > 0 {
            " \u{2191}"
        } else {
            " \u{2193}"
        }
    } else {
        ""
    };
    if pct >= 0 {
        format!("+{}%{}", pct, arrow)
    } else {
        format!("{}%{}", pct, arrow)
    }
}

/// Base CSS shared with the weekly card. Inlined here (rather than
/// re-exported from `html.rs`) to keep the two renderers fully
/// independent — the day card never had a heatmap grid like the weekly
/// one, so the rules diverge slightly.
const BASE_CSS: &str = r#"
*, *::before, *::after { box-sizing: border-box; }
body {
  margin: 0;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
  background: #f9fafb;
  color: #111827;
  line-height: 1.45;
  padding: 24px 12px;
}
.card {
  max-width: 960px;
  margin: 0 auto;
  background: #ffffff;
  border: 1px solid #e5e7eb;
  border-radius: 12px;
  padding: 32px;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.05);
}
.hd h1 { margin: 0; font-size: 1.6rem; color: #1d4ed8; }
.hd .sub { margin: 4px 0 24px 0; color: #6b7280; font-size: 0.95rem; }
section { margin-bottom: 28px; }
section h2 { font-size: 1.05rem; margin: 0 0 12px 0; color: #111827; }
.axis { margin: 0 0 8px 0; color: #6b7280; font-size: 0.8rem; }
table { width: 100%; border-collapse: collapse; font-size: 0.9rem; }
th, td { text-align: left; padding: 6px 8px; border-bottom: 1px solid #e5e7eb; }
th { font-weight: 600; background: #f3f4f6; color: #374151; }
td.num, th.num { text-align: right; font-variant-numeric: tabular-nums; }
td.item { font-family: ui-monospace, "SFMono-Regular", Menlo, Consolas, monospace; font-size: 0.85rem; word-break: break-all; }
.empty { color: #9ca3af; margin: 0; }
.sm p { font-size: 0.95rem; color: #1f2937; margin: 0; }
.ft { color: #9ca3af; font-size: 0.75rem; text-align: right; margin-top: 16px; }
"#;

/// Extra CSS deltas layered on top of the base palette so the day /
/// agent / compare cards adopt the new layout primitives without
/// duplicating the base rules.
const EXTRA_CSS: &str = r#"
.day-stats ul {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  gap: 24px;
  flex-wrap: wrap;
}
.day-stats li {
  font-size: 0.95rem;
  color: #1f2937;
}
.hour-grid {
  display: grid;
  grid-template-columns: repeat(24, 1fr);
  gap: 2px;
}
.hour-grid .hcell {
  height: 18px;
  border-radius: 2px;
  font-size: 0.65rem;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #6b7280;
}
.hour-grid .hcell.hdr { background: transparent; color: #6b7280; }
.compare-grid table { width: 100%; }
.empty-day p { color: #6b7280; font-size: 1rem; margin: 0; }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use fluxmirror_core::report::pack;

    #[test]
    fn render_today_card_starts_with_doctype_and_includes_title() {
        let lp = pack("english");
        let mut day = DayStats::default();
        day.total_events = 4;
        day.hours[10] = 4;
        let mut row = super::super::day::AgentRow::default();
        row.calls = 4;
        row.sessions.insert("s1".into());
        day.agents.insert("claude-code".into(), row);
        let s = render_day_card(
            &day,
            NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
            "Asia/Seoul",
            DayCardKind::Today,
            lp,
            "Generated by fluxmirror v0.5.7 - 2026-04-27T00:00:00Z",
        );
        assert!(s.starts_with("<!DOCTYPE html>"), "got: {}", &s[..40]);
        assert!(s.contains("</html>"));
        assert!(
            s.contains("Today&#39;s Work") || s.contains("Today's Work"),
            "missing today title:\n{s}"
        );
        assert!(s.contains("claude-code"));
        assert!(s.contains("2026-04-26"));
    }

    #[test]
    fn render_yesterday_card_uses_yesterday_strings() {
        let lp = pack("english");
        let mut day = DayStats::default();
        day.total_events = 3;
        day.hours[12] = 3;
        let mut row = super::super::day::AgentRow::default();
        row.calls = 3;
        row.sessions.insert("s1".into());
        day.agents.insert("gemini-cli".into(), row);
        let s = render_day_card(
            &day,
            NaiveDate::from_ymd_opt(2026, 4, 25).unwrap(),
            "UTC",
            DayCardKind::Yesterday,
            lp,
            "test-footer",
        );
        assert!(
            s.contains("Yesterday&#39;s Work") || s.contains("Yesterday's Work"),
            "missing yesterday title:\n{s}"
        );
        assert!(s.contains("2026-04-25"));
    }

    #[test]
    fn render_today_card_empty_uses_empty_message() {
        let lp = pack("english");
        let day = DayStats::default();
        let s = render_day_card(
            &day,
            NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
            "UTC",
            DayCardKind::Today,
            lp,
            "test-footer",
        );
        assert!(s.contains("No agent activity recorded today"));
    }

    #[test]
    fn render_compare_card_shows_today_yesterday_columns() {
        let lp = pack("english");
        let mut today = DayStats::default();
        today.total_events = 10;
        today.writes_total = 4;
        today.reads_total = 3;
        let mut yesterday = DayStats::default();
        yesterday.total_events = 5;
        yesterday.writes_total = 2;
        yesterday.reads_total = 1;
        let s = render_compare_card(
            &today,
            &yesterday,
            NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 25).unwrap(),
            "UTC",
            lp,
            "test-footer",
        );
        assert!(s.starts_with("<!DOCTYPE html>"));
        assert!(s.contains("Today vs Yesterday"));
        assert!(s.contains("2026-04-26"));
        assert!(s.contains("2026-04-25"));
        assert!(s.contains("+100%"));
    }

    #[test]
    fn render_compare_card_empty_uses_no_data_line() {
        let lp = pack("english");
        let day = DayStats::default();
        let s = render_compare_card(
            &day,
            &day,
            NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 25).unwrap(),
            "UTC",
            lp,
            "test-footer",
        );
        assert!(s.contains("No agent activity recorded on either day"));
    }

    #[test]
    fn agent_card_carries_agent_prefix_in_title() {
        let lp = pack("english");
        let mut day = DayStats::default();
        day.total_events = 2;
        day.hours[9] = 2;
        let mut row = super::super::day::AgentRow::default();
        row.calls = 2;
        row.sessions.insert("s1".into());
        day.agents.insert("claude-code".into(), row);
        let s = render_agent_day_card(
            &day,
            NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
            "UTC",
            DayCardKind::Today,
            "claude-code",
            lp,
            "test-footer",
        );
        assert!(
            s.contains("claude-code: Today&#39;s Work")
                || s.contains("claude-code: Today's Work"),
            "missing prefixed title:\n{s}"
        );
    }
}
