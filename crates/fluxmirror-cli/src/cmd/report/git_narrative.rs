// Git-narrative collection for the M5.1 "Shipped this week" section.
//
// Surfaces *what got done* (commit subjects per repo) alongside the
// data-only sections (heatmap, top files, top shells) the rest of the
// week report carries. Solves the "I see when and where I worked but
// not what I shipped" gap M5 left open.
//
// Strategy:
//   1. For each distinct cwd in the activity window, ask `git
//      rev-parse --show-toplevel` to map the cwd to its repo root.
//   2. Dedup the toplevels (multiple cwds often share one repo).
//   3. For each unique repo, run `git log --since --until --no-merges
//      --format=%s` (and optionally `--author=<email>`) to collect
//      commit subjects.
//   4. Truncate subjects to 100 chars (UTF-8 safe), keep at most 10
//      per repo, sort repos by total commits desc then alphabetically.
//
// Errors during git invocation never fail the report. Missing git,
// non-repo cwd, exit-128 — all degrade to "this repo contributes
// nothing to the narrative". The caller always gets back a
// `GitNarrative`, possibly with `repos.is_empty()`.
//
// We deliberately shell out to `git` rather than depend on `git2` /
// libgit2: the call count is small (one rev-parse + one log per
// unique repo, typically <5 repos per week), and the binary stays
// dependency-free.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

use chrono::{DateTime, Utc};

/// All commit-narrative data needed by the week report renderers.
#[derive(Debug, Default, Clone)]
pub struct GitNarrative {
    /// Repos with at least one commit in the window. Pre-sorted by
    /// total commit count desc, ties alphabetical.
    pub repos: Vec<RepoCommits>,
}

/// One repo's contribution to the narrative.
#[derive(Debug, Clone)]
pub struct RepoCommits {
    /// Basename of the repo toplevel (e.g. `fluxmirror`).
    pub repo_name: String,
    /// Absolute path to the repo toplevel.
    pub repo_path: PathBuf,
    /// First N commit subjects in `git log` order (newest first),
    /// truncated to 100 chars. May be shorter than `total_commits`
    /// when the repo had more than `MAX_COMMITS_PER_REPO`.
    pub commits: Vec<String>,
    /// Full count of matching commits in the window — drives the
    /// "X commits" badge so the truncation in `commits` stays
    /// transparent.
    pub total_commits: u32,
}

/// Maximum commit subjects retained per repo.
const MAX_COMMITS_PER_REPO: usize = 10;

/// Maximum commit-subject length (chars). Truncation is UTF-8 safe
/// because `take()` operates on `chars()`.
const MAX_SUBJECT_CHARS: usize = 100;

/// Collect the narrative.
///
/// `cwds` is the distinct list of working directories observed in the
/// activity window — typically pulled from `agent_events.cwd`.
/// `since_utc` and `until_utc` are the inclusive-start / exclusive-end
/// bounds matching the rest of the week aggregation. `author` is an
/// optional `--author=<value>` filter forwarded verbatim to `git log`.
/// Pass `None` for "all authors" — that's the safe default for a
/// single-user dev box where multiple identities may legitimately land
/// in the same repo.
pub fn collect(
    cwds: &[String],
    since_utc: DateTime<Utc>,
    until_utc: DateTime<Utc>,
    author: Option<&str>,
) -> GitNarrative {
    // Phase 1: cwd → toplevel resolution + dedup.
    let mut toplevels: BTreeSet<PathBuf> = BTreeSet::new();
    for cwd in cwds {
        if cwd.is_empty() {
            continue;
        }
        if let Some(top) = resolve_toplevel(cwd) {
            toplevels.insert(top);
        }
    }

    // Phase 2: per-toplevel `git log`.
    let since_str = since_utc.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let until_str = until_utc.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut repos: Vec<RepoCommits> = Vec::new();
    for top in &toplevels {
        if let Some(rc) = collect_repo(top, &since_str, &until_str, author) {
            if rc.total_commits > 0 {
                repos.push(rc);
            }
        }
    }

    // Phase 3: stable ordering — total commits desc, then name asc.
    repos.sort_by(|a, b| {
        b.total_commits
            .cmp(&a.total_commits)
            .then_with(|| a.repo_name.cmp(&b.repo_name))
    });

    GitNarrative { repos }
}

/// Run `git -C <cwd> rev-parse --show-toplevel` and parse the result.
/// Returns `None` if git is missing, the cwd isn't a repo, or any
/// other error — every failure degrades silently with a stderr note.
fn resolve_toplevel(cwd: &str) -> Option<PathBuf> {
    let out = match Command::new("git")
        .args(["-C", cwd, "rev-parse", "--show-toplevel"])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            eprintln!("fluxmirror week: git rev-parse failed for {cwd}: {e}");
            return None;
        }
    };
    if !out.status.success() {
        // Exit 128 is the canonical "not a repo" — not worth a note.
        return None;
    }
    let s = match std::str::from_utf8(&out.stdout) {
        Ok(s) => s.trim(),
        Err(_) => return None,
    };
    if s.is_empty() {
        return None;
    }
    Some(PathBuf::from(s))
}

/// Run `git log` for one repo and build its `RepoCommits`. Returns
/// `None` on git invocation errors; an empty repo (zero commits in
/// window) returns `Some` with `total_commits == 0` so the caller can
/// skip it uniformly with the toplevel filter above.
fn collect_repo(
    toplevel: &std::path::Path,
    since: &str,
    until: &str,
    author: Option<&str>,
) -> Option<RepoCommits> {
    let mut args: Vec<String> = vec![
        "-C".into(),
        toplevel.display().to_string(),
        "log".into(),
        format!("--since={}", since),
        format!("--until={}", until),
        "--no-merges".into(),
        "--format=%s".into(),
    ];
    if let Some(a) = author {
        args.push(format!("--author={}", a));
    }

    let out = match Command::new("git").args(&args).output() {
        Ok(o) => o,
        Err(e) => {
            eprintln!(
                "fluxmirror week: git log failed for {}: {e}",
                toplevel.display()
            );
            return None;
        }
    };
    if !out.status.success() {
        eprintln!(
            "fluxmirror week: git log non-zero exit for {} (status: {:?})",
            toplevel.display(),
            out.status.code()
        );
        return None;
    }

    let body = match std::str::from_utf8(&out.stdout) {
        Ok(s) => s,
        Err(_) => return None,
    };

    let mut total: u32 = 0;
    let mut subjects: Vec<String> = Vec::new();
    for line in body.lines() {
        if line.is_empty() {
            continue;
        }
        total = total.saturating_add(1);
        if subjects.len() < MAX_COMMITS_PER_REPO {
            subjects.push(truncate_chars(line, MAX_SUBJECT_CHARS));
        }
    }

    let repo_name = toplevel
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    Some(RepoCommits {
        repo_name,
        repo_path: toplevel.to_path_buf(),
        commits: subjects,
        total_commits: total,
    })
}

/// UTF-8 safe truncation: keep up to `max_chars` Unicode scalar values.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;
    use tempfile::TempDir;

    /// Initialize a minimal git repo with deterministic identity. Skip
    /// the test if `git` isn't on PATH so the suite stays portable.
    fn init_repo(dir: &std::path::Path) -> bool {
        if StdCommand::new("git").arg("--version").output().is_err() {
            return false;
        }
        let runs = [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "test@example.test"],
            vec!["config", "user.name", "test"],
            vec!["config", "commit.gpgsign", "false"],
        ];
        for argv in &runs {
            let st = StdCommand::new("git")
                .args(argv)
                .current_dir(dir)
                .status();
            if !matches!(st, Ok(s) if s.success()) {
                return false;
            }
        }
        true
    }

    fn make_commit(dir: &std::path::Path, file: &str, subject: &str) {
        std::fs::write(dir.join(file), subject).unwrap();
        let st = StdCommand::new("git")
            .args(["add", file])
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(st.success());
        let st = StdCommand::new("git")
            .args(["commit", "-q", "--no-gpg-sign", "-m", subject])
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(st.success(), "commit failed for {subject}");
    }

    #[test]
    fn collect_picks_up_three_commits_in_a_temp_repo() {
        let dir = TempDir::new().unwrap();
        if !init_repo(dir.path()) {
            eprintln!("skipping: git not available");
            return;
        }
        make_commit(dir.path(), "a.txt", "feat: first");
        make_commit(dir.path(), "b.txt", "fix: second");
        make_commit(dir.path(), "c.txt", "docs: third");

        let since = "2000-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let until = "2099-12-31T23:59:59Z".parse::<DateTime<Utc>>().unwrap();
        let cwd = dir.path().display().to_string();
        let n = collect(&[cwd], since, until, None);

        assert_eq!(n.repos.len(), 1, "got: {:?}", n.repos);
        let r = &n.repos[0];
        assert_eq!(r.total_commits, 3);
        assert_eq!(r.commits.len(), 3);
        // git log default order is newest first.
        assert_eq!(r.commits[0], "docs: third");
        assert_eq!(r.commits[1], "fix: second");
        assert_eq!(r.commits[2], "feat: first");
        // Repo name matches the temp dir basename.
        let expected_name = dir.path().file_name().unwrap().to_str().unwrap();
        assert_eq!(r.repo_name, expected_name);
    }

    #[test]
    fn collect_returns_empty_for_non_git_directory() {
        let dir = TempDir::new().unwrap();
        let since = "2000-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let until = "2099-12-31T23:59:59Z".parse::<DateTime<Utc>>().unwrap();
        let cwd = dir.path().display().to_string();
        let n = collect(&[cwd], since, until, None);
        assert!(n.repos.is_empty(), "non-git dir leaked into narrative");
    }

    #[test]
    fn dedup_collapses_two_cwds_in_the_same_repo() {
        let dir = TempDir::new().unwrap();
        if !init_repo(dir.path()) {
            eprintln!("skipping: git not available");
            return;
        }
        // Sub-directory inside the same repo so rev-parse resolves to
        // the same toplevel for both cwds.
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        make_commit(dir.path(), "x.txt", "feat: only");

        let since = "2000-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let until = "2099-12-31T23:59:59Z".parse::<DateTime<Utc>>().unwrap();
        let cwds = vec![
            dir.path().display().to_string(),
            sub.display().to_string(),
        ];
        let n = collect(&cwds, since, until, None);
        assert_eq!(n.repos.len(), 1, "expected dedup: got {:?}", n.repos);
        assert_eq!(n.repos[0].total_commits, 1);
    }

    #[test]
    fn truncate_chars_handles_multibyte() {
        // Ensure 100-char truncation is char-based, not byte-based.
        let s: String = std::iter::repeat('한').take(150).collect();
        let t = truncate_chars(&s, 100);
        assert_eq!(t.chars().count(), 100);
    }

    #[test]
    fn truncate_chars_preserves_short_strings() {
        assert_eq!(truncate_chars("hi", 100), "hi");
    }

    #[test]
    fn sort_orders_repos_by_count_desc_then_name_asc() {
        // Build two repos with different commit counts and assert the
        // sort comparator (the public one inside `collect`).
        let mut repos = vec![
            RepoCommits {
                repo_name: "zeta".into(),
                repo_path: PathBuf::from("/tmp/zeta"),
                commits: vec!["a".into()],
                total_commits: 1,
            },
            RepoCommits {
                repo_name: "alpha".into(),
                repo_path: PathBuf::from("/tmp/alpha"),
                commits: vec!["a".into(), "b".into(), "c".into()],
                total_commits: 3,
            },
            RepoCommits {
                repo_name: "beta".into(),
                repo_path: PathBuf::from("/tmp/beta"),
                commits: vec!["a".into()],
                total_commits: 1,
            },
        ];
        repos.sort_by(|a, b| {
            b.total_commits
                .cmp(&a.total_commits)
                .then_with(|| a.repo_name.cmp(&b.repo_name))
        });
        assert_eq!(repos[0].repo_name, "alpha");
        assert_eq!(repos[1].repo_name, "beta");
        assert_eq!(repos[2].repo_name, "zeta");
    }
}
