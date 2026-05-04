// Prompt registry.
//
// Templates live in `prompts/<name>.txt`, embedded at compile time via
// `include_str!()`. Each file starts with a `# version: <N>` comment;
// bumping the version invalidates cached responses (the version digest
// is folded into the cache_key in `synthesise()`).
//
// Format inside each file:
//
//   # version: 1
//   ## system
//   <system prompt body, possibly multi-line>
//   ## user
//   <user prompt body with {placeholder} markers>
//
// Substitution is intentionally minimal: `{name}` looks up `name` in
// the supplied JSON object and inlines its string / number / bool form.
// Missing keys render literally as `{name}` so a template never fails
// silently. No Tera, no Handlebars — keep the dep budget intact.

use serde_json::Value;

use crate::types::AiError;

const DAILY: &str = include_str!("../prompts/daily.txt");
const SESSION: &str = include_str!("../prompts/session.txt");
const PROJECT: &str = include_str!("../prompts/project.txt");
const ANOMALY: &str = include_str!("../prompts/anomaly.txt");

/// Look up a prompt by name. Returns the raw template body, including
/// the `# version: ...` line.
pub fn raw_template(name: &str) -> Result<&'static str, AiError> {
    match name {
        "daily" => Ok(DAILY),
        "session" => Ok(SESSION),
        "project" => Ok(PROJECT),
        "anomaly" => Ok(ANOMALY),
        other => Err(AiError::Prompt(format!("unknown prompt: {other}"))),
    }
}

/// Parse the leading `# version: <N>` line. Returns `0` when the file
/// is missing the marker — that's a strong signal in tests but never
/// fatal.
pub fn version_of(name: &str) -> Result<u32, AiError> {
    let body = raw_template(name)?;
    Ok(parse_version(body))
}

fn parse_version(body: &str) -> u32 {
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("# version:") {
            if let Ok(n) = rest.trim().parse::<u32>() {
                return n;
            }
        }
        if !trimmed.is_empty() {
            // First non-empty line was not a version marker.
            return 0;
        }
    }
    0
}

/// Render a prompt by substituting `{key}` placeholders against the
/// supplied JSON object. Returns `(system, user)`.
pub fn render_prompt(name: &str, ctx: &Value) -> Result<(String, String), AiError> {
    let body = raw_template(name)?;
    let (sys_tpl, user_tpl) = split_sections(body)?;
    let system = substitute(sys_tpl, ctx);
    let user = substitute(user_tpl, ctx);
    Ok((system, user))
}

/// Split a template body into its `## system` and `## user` halves.
/// Lines before either marker are dropped (that's where the version
/// comment lives).
fn split_sections(body: &str) -> Result<(&str, &str), AiError> {
    // Find both markers. They must appear in order, system before user.
    let mut sys_start: Option<usize> = None;
    let mut user_start: Option<usize> = None;
    let mut cursor = 0usize;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("## system") && sys_start.is_none() {
            sys_start = Some(cursor + line.len());
        } else if trimmed.starts_with("## user") && user_start.is_none() {
            user_start = Some(cursor);
        }
        cursor += line.len();
    }
    let sys_start = sys_start
        .ok_or_else(|| AiError::Prompt("missing `## system` section".to_string()))?;
    let user_start = user_start
        .ok_or_else(|| AiError::Prompt("missing `## user` section".to_string()))?;
    if sys_start > user_start {
        return Err(AiError::Prompt(
            "`## system` must precede `## user`".to_string(),
        ));
    }
    let system = body[sys_start..user_start].trim_matches('\n');
    // user_start points at the `## user` line; skip past it.
    let user_after_marker = match body[user_start..].find('\n') {
        Some(n) => user_start + n + 1,
        None => body.len(),
    };
    let user = body[user_after_marker..].trim_matches('\n');
    Ok((system, user))
}

fn substitute(tpl: &str, ctx: &Value) -> String {
    if !tpl.contains('{') {
        return tpl.to_string();
    }
    let mut out = String::with_capacity(tpl.len());
    let mut chars = tpl.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '{' {
            out.push(c);
            continue;
        }
        // Look ahead for a closing `}` on this line. If it's missing
        // or the placeholder name has whitespace / nested braces, treat
        // the literal `{` as untouched.
        let mut name = String::new();
        let mut closed = false;
        let mut spent = 0usize;
        for next in chars.by_ref() {
            spent += 1;
            if next == '}' {
                closed = true;
                break;
            }
            if next == '\n' || next == '{' || spent > 64 {
                break;
            }
            name.push(next);
        }
        if !closed || name.is_empty() {
            out.push('{');
            out.push_str(&name);
            continue;
        }
        match ctx.get(&name) {
            Some(Value::String(s)) => out.push_str(s),
            Some(Value::Number(n)) => out.push_str(&n.to_string()),
            Some(Value::Bool(b)) => out.push_str(&b.to_string()),
            Some(Value::Null) => {}
            Some(other) => out.push_str(&other.to_string()),
            None => {
                // Missing key: render the placeholder literally so a
                // reviewer can spot it instead of having an LLM see a
                // truncated prompt.
                out.push('{');
                out.push_str(&name);
                out.push('}');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn version_marker_parses() {
        assert!(version_of("daily").unwrap() >= 1);
        assert!(version_of("session").unwrap() >= 1);
        assert!(version_of("project").unwrap() >= 1);
        assert!(version_of("anomaly").unwrap() >= 1);
    }

    #[test]
    fn unknown_prompt_errors() {
        assert!(matches!(
            raw_template("nope"),
            Err(AiError::Prompt(_))
        ));
    }

    #[test]
    fn render_substitutes_placeholders() {
        let (sys, user) = render_prompt("daily", &json!({
            "agent_total": 384,
            "top_tool": "Bash",
            "summary_window": "yesterday",
            "session_count": 3,
            "edit_to_read_ratio": "0.25",
            "primary_languages": "Rust",
        }))
        .expect("daily renders");
        assert!(!sys.is_empty());
        assert!(user.contains("384"));
    }

    #[test]
    fn missing_key_keeps_placeholder() {
        let out = substitute("hello {who}", &json!({}));
        assert_eq!(out, "hello {who}");
    }
}
