// fluxmirror-core::report — small string surface used by the
// `fluxmirror <report>` subcommands plus the canonical data model
// shared by every consumer of the SQLite store.
//
// `lang` is the hand-rolled language pack (no templating crate). `dto`
// holds the public report DTOs. `data` holds the SQL aggregators that
// produce them — the CLI text/HTML reports and the studio JSON API
// both go through `data`, so the SQL queries are not duplicated.

pub mod data;
pub mod dto;
pub mod lang;

pub use dto::{
    AgentCount, DayRow, FileTouch, HourBucket, MethodCount, NowSnapshot, PathCount, ShellEvent,
    ToolMixEntry, TodayData, WeekData, WindowRange,
};
pub use lang::{pack, LangPack};
