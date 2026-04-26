// fluxmirror-core::report — small string surface used by the
// `fluxmirror <report>` subcommands.
//
// Phase 2 M1 ships a hand-rolled language pack rather than a full
// templating engine: every report has a fixed structure, the only thing
// that varies per language is a handful of column headers, titles and
// fixed strings. Pulling in a templating crate (handlebars / askama)
// would be more code than the current four packs combined.

pub mod lang;

pub use lang::{pack, LangPack};
