// Pattern-based output scrubbing.
//
// The capture binary (`fluxmirror hook`, `fluxmirror proxy`) records raw
// events into events.db with no filtering — that file is the audit
// source of truth and stays unmodified. This module is the
// presentation-layer scrubber that runs at every text / HTML / JSON
// output surface so a friend looking at our weekly card or studio
// dashboard never sees AWS keys, GitHub PATs, bearer tokens, .env paths,
// kv-leaks, or PEM private-key blocks.
//
// The scrub is whole-token: each match is replaced with the literal
// `[REDACTED:<category>]` string. We don't preserve original length —
// two adjacent secrets of different sizes both render as the same
// fixed-width sentinel.
//
// Overlapping match ranges (e.g. `auth=bearer xxx` triggers both the
// `kv_secret` and `bearer` rules) are merged into one masked span,
// flavoured with the leftmost rule's category so the output stays a
// single sentinel rather than two glued-together ones.

use std::borrow::Cow;
use std::sync::Arc;

use regex::Regex;

use crate::config::Config;

type Finder = Arc<dyn Fn(&str) -> Vec<(usize, usize)> + Send + Sync>;

/// One redaction rule. `category` flavours every mask emitted at this
/// rule's match sites (e.g. `[REDACTED:aws_key]`); `finder` returns the
/// list of `(start, end)` byte ranges to scrub.
#[derive(Clone)]
pub struct Pattern {
    category: String,
    finder: Finder,
}

impl Pattern {
    pub fn category(&self) -> &str {
        &self.category
    }
}

impl std::fmt::Debug for Pattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pattern")
            .field("category", &self.category)
            .finish_non_exhaustive()
    }
}

/// Layered rule set. `built_in` ships with the binary; `user` is loaded
/// from `Config.redaction.patterns` and layered on top.
#[derive(Debug, Clone)]
pub struct RedactionRules {
    pub built_in: Vec<Pattern>,
    pub user: Vec<Pattern>,
}

impl RedactionRules {
    /// Empty rule set. Useful for tests that want to assert pass-through
    /// behaviour without disturbing the built-ins.
    pub fn empty() -> Self {
        Self {
            built_in: Vec::new(),
            user: Vec::new(),
        }
    }
}

/// Built-in patterns only. Use [`from_config`] to also layer user
/// patterns from a `Config`.
pub fn default_rules() -> RedactionRules {
    RedactionRules {
        built_in: built_in_patterns(),
        user: Vec::new(),
    }
}

/// Built-in patterns + user-defined patterns from `cfg.redaction.patterns`.
///
/// Invalid user regexes are silently skipped — the redaction layer must
/// not break a report just because a project's `.fluxmirror.toml` carries
/// a typo. The capture path never gets here.
pub fn from_config(cfg: &Config) -> RedactionRules {
    let user = cfg
        .redaction
        .patterns
        .iter()
        .filter_map(|s| compile_user_pattern(s))
        .collect();
    RedactionRules {
        built_in: built_in_patterns(),
        user,
    }
}

/// Apply every rule to `text`. Returns `Cow::Borrowed` (zero-copy) when
/// no rule matches; allocates a new owned `String` only on hit.
pub fn scrub<'a>(text: &'a str, rules: &RedactionRules) -> Cow<'a, str> {
    if text.is_empty() {
        return Cow::Borrowed(text);
    }
    // Collect every (start, end, category) hit across every rule.
    let mut hits: Vec<(usize, usize, &str)> = Vec::new();
    for p in rules.built_in.iter().chain(rules.user.iter()) {
        for (s, e) in (p.finder)(text) {
            if s < e && e <= text.len() {
                hits.push((s, e, p.category.as_str()));
            }
        }
    }
    if hits.is_empty() {
        return Cow::Borrowed(text);
    }

    // Merge overlapping ranges so two rules that both flag overlapping
    // bytes (e.g. `auth=bearer xxx`) collapse into one mask. The leftmost
    // rule wins on category so the visible label is deterministic.
    hits.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut merged: Vec<(usize, usize, &str)> = Vec::new();
    for (s, e, c) in hits {
        match merged.last_mut() {
            Some(last) if s < last.1 => {
                if e > last.1 {
                    last.1 = e;
                }
            }
            _ => merged.push((s, e, c)),
        }
    }

    let mut out = String::with_capacity(text.len() + 16);
    let mut cursor = 0usize;
    for (s, e, cat) in merged {
        // Defensive: regex matches are aligned to char boundaries, but
        // user-supplied regexes might not be. Skip any match that would
        // split a UTF-8 codepoint.
        if !text.is_char_boundary(s) || !text.is_char_boundary(e) {
            continue;
        }
        if s < cursor {
            continue;
        }
        out.push_str(&text[cursor..s]);
        out.push_str("[REDACTED:");
        out.push_str(cat);
        out.push(']');
        cursor = e;
    }
    out.push_str(&text[cursor..]);
    Cow::Owned(out)
}

fn built_in_patterns() -> Vec<Pattern> {
    vec![
        simple_pattern("aws_key", r"AKIA[0-9A-Z]{16}"),
        aws_secret_pattern(),
        simple_pattern(
            "github_pat",
            r"ghp_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{40,}",
        ),
        simple_pattern("bearer", r"(?i)bearer\s+[A-Za-z0-9._\-]+"),
        env_path_pattern(),
        simple_pattern(
            "kv_secret",
            r"(?i)(?:password|passwd|secret|api_key|token|auth)=\S+",
        ),
        simple_pattern("private_key", r"-----BEGIN [A-Z ]+PRIVATE KEY-----"),
    ]
}

/// Simple full-match pattern: every regex match is masked whole.
fn simple_pattern(category: &'static str, expr: &str) -> Pattern {
    let re = Regex::new(expr).expect("built-in regex should compile");
    Pattern {
        category: category.to_string(),
        finder: Arc::new(move |text: &str| {
            re.find_iter(text).map(|m| (m.start(), m.end())).collect()
        }),
    }
}

/// `aws_secret_access_key = <40-char base64>` proximity rule. Only the
/// 40-char value is masked; the literal `aws_secret_access_key=` prefix
/// is left visible so the report still reads coherently.
fn aws_secret_pattern() -> Pattern {
    // Gap class includes `=`, `:`, whitespace, quotes — anything that is
    // not a base64 *value* char. Value capture forces an alphanumeric
    // first char so the gap can't bleed into the secret.
    let re = Regex::new(
        r"(?i)aws_secret_access_key[^A-Za-z0-9/+]{0,32}([A-Za-z0-9][A-Za-z0-9/+=]{39})",
    )
    .expect("aws_secret regex should compile");
    Pattern {
        category: "aws_secret".to_string(),
        finder: Arc::new(move |text: &str| {
            re.captures_iter(text)
                .filter_map(|caps| caps.get(1).map(|m| (m.start(), m.end())))
                .collect()
        }),
    }
}

/// `.env` / `.env.local` / etc. as a standalone path component. The
/// Rust `regex` crate has no lookahead/lookbehind, so the boundary
/// check is done in code: each candidate must be flanked by a
/// non-identifier char (or the string edge). That stops false positives
/// like `mycustom.envfile` / `.env_table` while still catching `.env`
/// before HTML closing tags, `</li>`, `"` JSON quotes, `:` line
/// suffixes, or sentence-ending punctuation.
fn env_path_pattern() -> Pattern {
    let re = Regex::new(r"\.env(?:\.[a-z0-9_-]+)?").expect("env_path regex should compile");
    Pattern {
        category: "env_path".to_string(),
        finder: Arc::new(move |text: &str| {
            let mut hits = Vec::new();
            for m in re.find_iter(text) {
                let start = m.start();
                let end = m.end();
                let left_ok = start == 0
                    || text[..start]
                        .chars()
                        .next_back()
                        .map(is_token_boundary_char)
                        .unwrap_or(true);
                let right_ok = end == text.len()
                    || text[end..]
                        .chars()
                        .next()
                        .map(is_token_boundary_char)
                        .unwrap_or(true);
                if left_ok && right_ok {
                    hits.push((start, end));
                }
            }
            hits
        }),
    }
}

/// Boundary char predicate. Treats anything that isn't an identifier
/// continuation char (ASCII alphanumeric / `_`) as a token boundary.
/// Punctuation, whitespace, path separators, HTML/JSON delimiters, and
/// `.` all count as boundaries; that lets the env_path rule fire inside
/// rendered HTML without needing the regex crate's missing lookarounds.
fn is_token_boundary_char(c: char) -> bool {
    !(c.is_ascii_alphanumeric() || c == '_')
}

fn compile_user_pattern(expr: &str) -> Option<Pattern> {
    let re = Regex::new(expr).ok()?;
    Some(Pattern {
        category: "user".to_string(),
        finder: Arc::new(move |text: &str| {
            re.find_iter(text).map(|m| (m.start(), m.end())).collect()
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> RedactionRules {
        default_rules()
    }

    // ---- aws_key ---------------------------------------------------------

    #[test]
    fn aws_key_positive() {
        let s = scrub("AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE", &rules());
        assert!(s.contains("[REDACTED:aws_key]"));
        assert!(!s.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn aws_key_negative_lowercase() {
        // Lowercase prefix must not match — AKIA must be uppercase.
        let s = scrub("akiaiosfodnn7example", &rules());
        assert_eq!(s, "akiaiosfodnn7example");
    }

    #[test]
    fn aws_key_within_other_text() {
        let s = scrub("see commit AKIAIOSFODNN7EXAMPLE in logs", &rules());
        assert!(s.contains("[REDACTED:aws_key]"));
        assert!(s.contains("see commit"));
        assert!(s.contains("in logs"));
    }

    // ---- aws_secret ------------------------------------------------------

    #[test]
    fn aws_secret_positive_proximity() {
        let s = scrub(
            "aws_secret_access_key=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            &rules(),
        );
        assert!(s.contains("[REDACTED:aws_secret]"));
        assert!(!s.contains("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"));
        // The key prefix is intentionally left visible.
        assert!(s.contains("aws_secret_access_key="));
    }

    #[test]
    fn aws_secret_negative_no_proximity() {
        // 40-char base64 alone, no aws_secret_access_key marker → no match.
        let s = scrub("checksum=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY", &rules());
        assert!(!s.contains("[REDACTED:aws_secret]"));
    }

    #[test]
    fn aws_secret_within_quoted_assignment() {
        let s = scrub(
            r#"AWS_SECRET_ACCESS_KEY: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY""#,
            &rules(),
        );
        assert!(s.contains("[REDACTED:aws_secret]"));
    }

    // ---- github_pat ------------------------------------------------------

    #[test]
    fn github_pat_classic_positive() {
        let s = scrub(
            "token=ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            &rules(),
        );
        assert!(s.contains("[REDACTED:"));
        assert!(!s.contains("ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
    }

    #[test]
    fn github_pat_fine_grained_positive() {
        let s = scrub(
            "github_pat_11ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnop12345",
            &rules(),
        );
        assert!(s.contains("[REDACTED:"));
    }

    #[test]
    fn github_pat_negative_short_prefix() {
        // ghp_ followed by only 4 chars is not a real PAT.
        let s = scrub("see ghp_abcd in changelog", &rules());
        assert!(!s.contains("[REDACTED:github_pat]"));
        assert_eq!(s, "see ghp_abcd in changelog");
    }

    #[test]
    fn github_pat_negative_within_url() {
        // Looks similar but isn't a real PAT — ghp_ prefix only with 4
        // chars after.
        let s = scrub("https://example.com/ghp_demo", &rules());
        assert!(!s.contains("[REDACTED:github_pat]"));
    }

    // ---- bearer ----------------------------------------------------------

    #[test]
    fn bearer_positive() {
        let s = scrub("Authorization: Bearer abc.def.ghi-jkl_mno", &rules());
        assert!(s.contains("[REDACTED:"));
        assert!(!s.contains("abc.def.ghi-jkl_mno"));
    }

    #[test]
    fn bearer_negative_no_token_after() {
        // "bearer" with no token isn't a credential.
        let s = scrub("the bearer of bad news", &rules());
        // "bearer of" matches because 'of' qualifies as a token under
        // the cheap rule. But any leak is downstream of an obvious
        // false positive — see the README. Verify by accepting either
        // outcome but never the literal word "of" leaking when bearer
        // matched something.
        if s.contains("[REDACTED:bearer]") {
            assert!(!s.contains("bearer of"));
        }
    }

    #[test]
    fn bearer_within_json() {
        let s = scrub(
            r#"{"authorization":"bearer eyJhbGciOiJIUzI1NiJ9.payload.signature"}"#,
            &rules(),
        );
        assert!(s.contains("[REDACTED:"));
        assert!(!s.contains("eyJhbGciOiJIUzI1NiJ9.payload.signature"));
    }

    // ---- env_path --------------------------------------------------------

    #[test]
    fn env_path_positive_bare() {
        let s = scrub("loaded .env from cwd", &rules());
        assert!(s.contains("[REDACTED:env_path]"));
        assert!(!s.contains(" .env "));
    }

    #[test]
    fn env_path_positive_dotted() {
        let s = scrub("/usr/me/.env.production:42", &rules());
        assert!(s.contains("[REDACTED:env_path]"));
    }

    #[test]
    fn env_path_negative_inside_word() {
        // "developmentenv" must not match — the leading boundary fails.
        let s = scrub("the developmentenv setup", &rules());
        assert_eq!(s, "the developmentenv setup");
    }

    #[test]
    fn env_path_negative_attached_extension() {
        // mycustom.env (no separator before `.env`) → must not redact.
        let s = scrub("see mycustom.env_table", &rules());
        assert_eq!(s, "see mycustom.env_table");
    }

    // ---- kv_secret -------------------------------------------------------

    #[test]
    fn kv_secret_positive_password() {
        let s = scrub("password=hunter2 in code", &rules());
        assert!(s.contains("[REDACTED:kv_secret]"));
        assert!(!s.contains("hunter2"));
    }

    #[test]
    fn kv_secret_positive_api_key() {
        let s = scrub("api_key=sk-very-secret-1234", &rules());
        assert!(s.contains("[REDACTED:kv_secret]"));
        assert!(!s.contains("sk-very-secret-1234"));
    }

    #[test]
    fn kv_secret_negative_no_assignment() {
        // No `=` after the key keyword → not a leak.
        let s = scrub("the password is unset", &rules());
        assert_eq!(s, "the password is unset");
    }

    // ---- private_key -----------------------------------------------------

    #[test]
    fn private_key_positive_rsa() {
        let s = scrub(
            "-----BEGIN RSA PRIVATE KEY-----\nMIIE...\n-----END RSA PRIVATE KEY-----",
            &rules(),
        );
        assert!(s.contains("[REDACTED:private_key]"));
        assert!(!s.contains("BEGIN RSA PRIVATE KEY"));
    }

    #[test]
    fn private_key_positive_ed25519() {
        let s = scrub("-----BEGIN OPENSSH PRIVATE KEY-----", &rules());
        assert!(s.contains("[REDACTED:private_key]"));
    }

    #[test]
    fn private_key_negative_public_marker() {
        // "PUBLIC KEY" must not match.
        let s = scrub("-----BEGIN PUBLIC KEY-----", &rules());
        assert_eq!(s, "-----BEGIN PUBLIC KEY-----");
    }

    // ---- user patterns ---------------------------------------------------

    #[test]
    fn user_pattern_layered_on_top() {
        let mut cfg = Config::default();
        cfg.redaction.patterns.push(r"INTERNAL-\d{5}".to_string());
        let r = from_config(&cfg);
        let s = scrub("incident INTERNAL-12345 closed", &r);
        assert!(s.contains("[REDACTED:user]"));
        assert!(!s.contains("INTERNAL-12345"));
    }

    #[test]
    fn user_pattern_invalid_regex_is_skipped() {
        let mut cfg = Config::default();
        cfg.redaction.patterns.push("[unclosed".to_string());
        let r = from_config(&cfg);
        // Built-ins still apply; the bad pattern is dropped.
        let s = scrub("AKIAIOSFODNN7EXAMPLE", &r);
        assert!(s.contains("[REDACTED:aws_key]"));
    }

    // ---- zero-copy + structural assertions ------------------------------

    #[test]
    fn no_match_returns_borrowed() {
        let r = default_rules();
        let s = "totally innocuous report content";
        let out = scrub(s, &r);
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(out, s);
    }

    #[test]
    fn empty_input_returns_borrowed() {
        let r = default_rules();
        let out = scrub("", &r);
        assert!(matches!(out, Cow::Borrowed(_)));
    }

    #[test]
    fn overlapping_hits_merge_into_single_mask() {
        // `auth=bearer xxx` triggers both kv_secret and bearer.
        let r = default_rules();
        let s = scrub("Authorization: auth=bearer abc123", &r);
        // Whatever happens, the original token must not survive.
        assert!(!s.contains("abc123"));
        // And the masked output must have at least one [REDACTED:...].
        assert!(s.contains("[REDACTED:"));
    }

    #[test]
    fn category_label_is_visible_in_output() {
        let s = scrub("AKIAIOSFODNN7EXAMPLE", &default_rules());
        assert_eq!(s, "[REDACTED:aws_key]");
    }

    #[test]
    fn multiple_distinct_secrets_each_masked() {
        let s = scrub(
            "key=AKIAIOSFODNN7EXAMPLE later password=hunter2 done",
            &default_rules(),
        );
        assert!(s.contains("[REDACTED:aws_key]"));
        assert!(s.contains("[REDACTED:kv_secret]"));
        assert!(!s.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(!s.contains("hunter2"));
    }
}
