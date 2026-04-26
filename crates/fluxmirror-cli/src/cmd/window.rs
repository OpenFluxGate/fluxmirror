// fluxmirror window — compute a TZ-aware time range.
//
// Output is a single space-separated line on stdout, consumed by the
// slash commands via `read VAR1 VAR2 ... <<EOF`. The shape depends on
// the period:
//
//   today      LOCAL_DATE START_UTC END_UTC START_MS END_MS
//   yesterday  LOCAL_DATE START_UTC END_UTC START_MS END_MS
//   week       WEEK_START_LOCAL WEEK_END_LOCAL START_UTC END_UTC START_MS END_MS
//
// All dates are formatted as YYYY-MM-DD in the requested timezone, all
// UTC timestamps as ISO 8601 with second precision and trailing `Z`,
// and all millisecond values as integers.

use std::process::ExitCode;

use chrono::{DateTime, Duration, NaiveDate, SecondsFormat, TimeZone, Utc};
use chrono_tz::Tz;

use super::util::{err_exit2, parse_tz};

/// Resolve the inclusive 7-day local window ending today in `tz`.
///
/// Returns `(week_start_local, week_end_local, start_utc, end_utc)`:
/// - `week_start_local` is the first local date in the inclusive window.
/// - `week_end_local` is today's local date (the inclusive last day).
/// - `start_utc` / `end_utc` are the half-open `[start, end)` UTC bounds
///   that align with `local_midnight(start_date)` and
///   `local_midnight(today + 1)` respectively.
///
/// Errors (DST gap with no resolvable midnight) propagate as a string
/// so the caller can frame them in its own command name.
pub(crate) fn week_range(
    tz: Tz,
) -> Result<(NaiveDate, NaiveDate, DateTime<Utc>, DateTime<Utc>), String> {
    let now_local = Utc::now().with_timezone(&tz);
    let today = now_local.date_naive();
    let tomorrow = today + Duration::days(1);
    let start_date = tomorrow - Duration::days(7);

    let end_local = local_midnight(tz, tomorrow).ok_or_else(|| {
        format!("cannot resolve local midnight for {tomorrow} in {tz}")
    })?;
    let start_local = local_midnight(tz, start_date).ok_or_else(|| {
        format!("cannot resolve local midnight for {start_date} in {tz}")
    })?;

    let week_end_local = tomorrow - Duration::days(1);
    Ok((
        start_date,
        week_end_local,
        start_local.with_timezone(&Utc),
        end_local.with_timezone(&Utc),
    ))
}

/// Resolve the local-midnight `[start, end)` UTC window for a single
/// day offset relative to today in `tz`.
///
/// `day_offset == 0` is today (local), `-1` is yesterday, etc. Returns
/// `(target_local_date, start_utc, end_utc)`. Errors mirror
/// `week_range` (a DST gap on the requested local midnight that the
/// resolver could not skip past).
pub(crate) fn day_range(
    tz: Tz,
    day_offset: i64,
) -> Result<(NaiveDate, DateTime<Utc>, DateTime<Utc>), String> {
    let now_local = Utc::now().with_timezone(&tz);
    let target_date = now_local.date_naive() + Duration::days(day_offset);
    let next_date = target_date + Duration::days(1);

    let start_local = local_midnight(tz, target_date).ok_or_else(|| {
        format!("cannot resolve local midnight for {target_date} in {tz}")
    })?;
    let end_local = local_midnight(tz, next_date)
        .ok_or_else(|| format!("cannot resolve local midnight for {next_date} in {tz}"))?;

    Ok((
        target_date,
        start_local.with_timezone(&Utc),
        end_local.with_timezone(&Utc),
    ))
}

/// Convenience wrapper around `day_range(tz, 0)`. Used by both the
/// `today` report and any future caller that wants today's window
/// without the day-offset boilerplate.
pub(crate) fn today_range(
    tz: Tz,
) -> Result<(NaiveDate, DateTime<Utc>, DateTime<Utc>), String> {
    day_range(tz, 0)
}

pub fn run(tz: String, period: String) -> ExitCode {
    let tz = match parse_tz(&tz) {
        Ok(t) => t,
        Err(e) => return err_exit2(format!("fluxmirror window: {e}")),
    };

    match period.as_str() {
        "today" => emit_day(tz, 0),
        "yesterday" => emit_day(tz, -1),
        "week" => emit_week(tz),
        other => err_exit2(format!(
            "fluxmirror window: unknown period {other:?} (expected today | yesterday | week)"
        )),
    }
}

/// Emit the 5-field "today"/"yesterday" line. `day_offset` is the
/// signed number of days relative to today (0 = today, -1 = yesterday).
fn emit_day(tz: Tz, day_offset: i64) -> ExitCode {
    let now_local = Utc::now().with_timezone(&tz);
    let target_date = now_local.date_naive() + Duration::days(day_offset);

    let start_local = match local_midnight(tz, target_date) {
        Some(t) => t,
        None => {
            return err_exit2(format!(
                "fluxmirror window: cannot resolve local midnight for {target_date} in {tz}"
            ))
        }
    };
    let end_local = match local_midnight(tz, target_date + Duration::days(1)) {
        Some(t) => t,
        None => {
            return err_exit2(format!(
                "fluxmirror window: cannot resolve local midnight for {} in {tz}",
                target_date + Duration::days(1)
            ))
        }
    };

    let start_utc = start_local.with_timezone(&Utc);
    let end_utc = end_local.with_timezone(&Utc);

    println!(
        "{date} {start} {end} {start_ms} {end_ms}",
        date = target_date.format("%Y-%m-%d"),
        start = start_utc.to_rfc3339_opts(SecondsFormat::Secs, true),
        end = end_utc.to_rfc3339_opts(SecondsFormat::Secs, true),
        start_ms = start_utc.timestamp_millis(),
        end_ms = end_utc.timestamp_millis(),
    );
    ExitCode::SUCCESS
}

/// Emit the 6-field "week" line: last 7 days inclusive of today.
///
/// `end = local_midnight_tomorrow`, `start = end - 7 days`. The label
/// dates use `start` and `end - 1 day` so the range reads as a closed
/// interval to the user.
fn emit_week(tz: Tz) -> ExitCode {
    let (start_date, week_end_local, start_utc, end_utc) = match week_range(tz) {
        Ok(r) => r,
        Err(e) => return err_exit2(format!("fluxmirror window: {e}")),
    };

    println!(
        "{ws} {we} {start} {end} {start_ms} {end_ms}",
        ws = start_date.format("%Y-%m-%d"),
        we = week_end_local.format("%Y-%m-%d"),
        start = start_utc.to_rfc3339_opts(SecondsFormat::Secs, true),
        end = end_utc.to_rfc3339_opts(SecondsFormat::Secs, true),
        start_ms = start_utc.timestamp_millis(),
        end_ms = end_utc.timestamp_millis(),
    );
    ExitCode::SUCCESS
}

/// Resolve `00:00:00` on `date` in `tz` to a concrete `DateTime<Tz>`.
///
/// Local midnight can be ambiguous (DST fall-back) or skipped (DST
/// spring-forward). We pick the earlier instant when ambiguous and the
/// next valid second forward when skipped — matching python's
/// `zoneinfo` default fold behaviour the original slash commands relied
/// on.
fn local_midnight(tz: Tz, date: NaiveDate) -> Option<chrono::DateTime<Tz>> {
    let naive = date.and_hms_opt(0, 0, 0)?;
    match tz.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => Some(dt),
        chrono::LocalResult::Ambiguous(earliest, _latest) => Some(earliest),
        chrono::LocalResult::None => {
            // DST gap — try advancing one second at a time until we land
            // in a valid local instant. The gap is always < 2 hours in
            // practice, so a small bounded loop is fine.
            for offset_secs in 1..=7200 {
                let candidate = naive + Duration::seconds(offset_secs);
                if let chrono::LocalResult::Single(dt) = tz.from_local_datetime(&candidate) {
                    return Some(dt);
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(tz: &str, period: &str) -> Vec<String> {
        // The clap-derive `run` writes to stdout via println!. To keep
        // tests fast and self-contained we invoke the underlying logic
        // by recomputing the same fields via the public helpers — i.e.
        // we mirror what `emit_day` / `emit_week` print.
        //
        // This is a structural check (field count + format), not a
        // black-box stdout capture.
        let tz = parse_tz(tz).unwrap();
        let now_local = Utc::now().with_timezone(&tz);
        let today = now_local.date_naive();
        match period {
            "today" => render_day(tz, today),
            "yesterday" => render_day(tz, today - Duration::days(1)),
            "week" => render_week(tz, today),
            _ => panic!("bad period"),
        }
    }

    fn render_day(tz: Tz, date: NaiveDate) -> Vec<String> {
        let start = local_midnight(tz, date).unwrap().with_timezone(&Utc);
        let end = local_midnight(tz, date + Duration::days(1))
            .unwrap()
            .with_timezone(&Utc);
        vec![
            date.format("%Y-%m-%d").to_string(),
            start.to_rfc3339_opts(SecondsFormat::Secs, true),
            end.to_rfc3339_opts(SecondsFormat::Secs, true),
            start.timestamp_millis().to_string(),
            end.timestamp_millis().to_string(),
        ]
    }

    fn render_week(tz: Tz, today: NaiveDate) -> Vec<String> {
        let tomorrow = today + Duration::days(1);
        let end = local_midnight(tz, tomorrow).unwrap().with_timezone(&Utc);
        let start_date = tomorrow - Duration::days(7);
        let start = local_midnight(tz, start_date).unwrap().with_timezone(&Utc);
        let week_end_local = tomorrow - Duration::days(1);
        vec![
            start_date.format("%Y-%m-%d").to_string(),
            week_end_local.format("%Y-%m-%d").to_string(),
            start.to_rfc3339_opts(SecondsFormat::Secs, true),
            end.to_rfc3339_opts(SecondsFormat::Secs, true),
            start.timestamp_millis().to_string(),
            end.timestamp_millis().to_string(),
        ]
    }

    #[test]
    fn today_has_five_fields() {
        let fields = capture("Asia/Seoul", "today");
        assert_eq!(fields.len(), 5);
        // ISO format check
        assert!(fields[1].ends_with('Z'));
        assert!(fields[2].ends_with('Z'));
        // 86_400_000 ms in a day (no DST in Asia/Seoul)
        let s: i64 = fields[3].parse().unwrap();
        let e: i64 = fields[4].parse().unwrap();
        assert_eq!(e - s, 86_400_000);
    }

    #[test]
    fn yesterday_has_five_fields_and_24h_window() {
        let fields = capture("UTC", "yesterday");
        assert_eq!(fields.len(), 5);
        let s: i64 = fields[3].parse().unwrap();
        let e: i64 = fields[4].parse().unwrap();
        assert_eq!(e - s, 86_400_000);
    }

    #[test]
    fn week_has_six_fields_and_7day_window() {
        let fields = capture("UTC", "week");
        assert_eq!(fields.len(), 6);
        // ISO format check on the two UTC slots
        assert!(fields[2].ends_with('Z'));
        assert!(fields[3].ends_with('Z'));
        let s: i64 = fields[4].parse().unwrap();
        let e: i64 = fields[5].parse().unwrap();
        // 7 days in ms, with no DST in UTC
        assert_eq!(e - s, 7 * 86_400_000);
    }

    #[test]
    fn week_label_is_inclusive_today() {
        // WEEK_END_LOCAL must be today's local date; WEEK_START_LOCAL
        // must be 6 days before that.
        let fields = capture("UTC", "week");
        let ws = NaiveDate::parse_from_str(&fields[0], "%Y-%m-%d").unwrap();
        let we = NaiveDate::parse_from_str(&fields[1], "%Y-%m-%d").unwrap();
        assert_eq!((we - ws).num_days(), 6, "week label spans 7 days inclusive");
        assert_eq!(we, Utc::now().date_naive());
    }

    #[test]
    fn local_midnight_known_offset() {
        // Asia/Seoul is fixed UTC+9, no DST. 2026-04-26 00:00 KST →
        // 2026-04-25T15:00:00Z.
        let tz: Tz = "Asia/Seoul".parse().unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 4, 26).unwrap();
        let mid = local_midnight(tz, date).unwrap().with_timezone(&Utc);
        assert_eq!(
            mid.to_rfc3339_opts(SecondsFormat::Secs, true),
            "2026-04-25T15:00:00Z"
        );
    }
}
