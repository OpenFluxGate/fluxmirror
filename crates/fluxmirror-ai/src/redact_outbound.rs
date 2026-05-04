// Outbound-only scrubber.
//
// Wraps `fluxmirror_core::redact::scrub` with two extra layers that are
// specific to the outbound-LLM path:
//
//   1. `$HOME` → `~` substitution (and the same for any value of the
//      `HOME` / `USERPROFILE` env vars). Anthropic / Ollama / any other
//      remote endpoint must never see the operator's full path.
//   2. Hard size cap: per-prompt user messages are clamped to
//      `max_user_chars` characters with a sentinel; an over-eager
//      template that pastes 5 MB of git log into the user field can't
//      blow our token budget.
//
// The capture-side scrubber (`fluxmirror_core::redact::scrub`) covers
// secret-leaking patterns; this module sits on top.

use fluxmirror_core::redact::{scrub, RedactionRules};

/// Max char fallback when no AI config is in scope.
pub const DEFAULT_MAX_USER_CHARS: usize = 8 * 1024;

/// Truncation sentinel — appended on overflow. Kept short so the LLM
/// gets some signal without burning budget on a long marker.
pub const TRUNCATION_SENTINEL: &str = "... [truncated to fit prompt budget]";

/// Single-shot outbound-redact pass. Order matters:
///
///   1. M7 secret-pattern scrub (AWS keys, GitHub PATs, .env paths, ...).
///   2. `$HOME` / `USERPROFILE` → `~`.
///   3. cap at `max_user_chars` chars with sentinel.
///
/// The result is always borrowed-or-owned safely; no panics on empty
/// input or non-UTF-8 boundaries.
pub fn redact_outbound(text: &str, rules: &RedactionRules, max_user_chars: usize) -> String {
    if text.is_empty() {
        return String::new();
    }

    // Step 1: secret patterns.
    let scrubbed = scrub(text, rules).into_owned();

    // Step 2: `$HOME` / `USERPROFILE` → `~`. We swap the longest match
    // first so a path like /Users/alice/Library doesn't end up as
    // /Users/alice~rary if HOME is /Users/alice/Library/foo.
    let mut homes: Vec<String> = Vec::new();
    if let Ok(h) = std::env::var("HOME") {
        if !h.is_empty() {
            homes.push(h);
        }
    }
    if let Ok(h) = std::env::var("USERPROFILE") {
        if !h.is_empty() {
            homes.push(h);
        }
    }
    homes.sort_by_key(|s| std::cmp::Reverse(s.len()));
    homes.dedup();
    let mut path_scrubbed = scrubbed;
    for h in &homes {
        if path_scrubbed.contains(h) {
            path_scrubbed = path_scrubbed.replace(h, "~");
        }
    }

    // Step 3: char-count truncation at user-configured max.
    let cap = if max_user_chars == 0 {
        DEFAULT_MAX_USER_CHARS
    } else {
        max_user_chars
    };
    truncate_at_chars(&path_scrubbed, cap)
}

fn truncate_at_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    // Reserve room for the sentinel so the final string fits the cap.
    let sentinel_chars = TRUNCATION_SENTINEL.chars().count();
    let keep = max_chars.saturating_sub(sentinel_chars);
    let mut out = String::with_capacity(s.len().min(max_chars * 4));
    for (i, c) in s.chars().enumerate() {
        if i >= keep {
            break;
        }
        out.push(c);
    }
    out.push_str(TRUNCATION_SENTINEL);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluxmirror_core::redact::default_rules;

    #[test]
    fn passthrough_when_clean() {
        let r = default_rules();
        let out = redact_outbound("nothing to scrub here", &r, 8192);
        assert_eq!(out, "nothing to scrub here");
    }

    #[test]
    fn truncates_long_messages() {
        let r = default_rules();
        let long = "x".repeat(20_000);
        let out = redact_outbound(&long, &r, 100);
        assert!(out.chars().count() <= 100);
        assert!(out.ends_with(TRUNCATION_SENTINEL));
    }
}
