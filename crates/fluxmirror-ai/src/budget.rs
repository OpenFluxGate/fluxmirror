// Daily USD ceiling.
//
// One file per local-day at `<root>/ai-budget-<YYYY-MM-DD>.txt`. The file
// holds a single floating-point number in USD with cumulative spend so
// far. Every increment goes through `tmp + atomic rename` so a process
// crash mid-write can never corrupt the running tally.
//
// Auto-reset is implicit: when the local date rolls forward, the next
// `check_and_reserve()` simply opens a new dated file (the old one is
// left on disk as a daily ledger).
//
// The "cap" knob is supplied by the caller (typically
// `Config::ai.daily_budget_usd`); $1.00/day default lives in `AiConfig`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Local;

use crate::types::AiError;

/// Default budget root: `~/.fluxmirror/`.
pub fn default_root() -> PathBuf {
    fluxmirror_core::paths::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".fluxmirror")
}

#[derive(Debug, Clone)]
pub struct Budget {
    /// Directory holding the per-day files.
    root: PathBuf,
    /// Daily USD ceiling.
    cap_usd: f64,
}

impl Budget {
    /// Construct a budget bound to a specific root and cap.
    pub fn new(root: PathBuf, cap_usd: f64) -> Self {
        Self { root, cap_usd }
    }

    /// Construct a budget at the default root with the given cap.
    pub fn at_default(cap_usd: f64) -> Self {
        Self::new(default_root(), cap_usd)
    }

    /// Cap accessor (used by callers that want to gate UI on a numeric
    /// "is the budget meaningful" check).
    pub fn cap_usd(&self) -> f64 {
        self.cap_usd
    }

    /// Read the current spend for today's local date. Returns `0.0` if
    /// the file is absent or unreadable; this is best-effort accounting,
    /// not a financial system.
    pub fn current_spend(&self) -> f64 {
        let path = self.path_for_today();
        read_amount(&path).unwrap_or(0.0)
    }

    /// Reserve `est_usd` against today's spend. Returns
    /// `Err(BudgetExceeded)` if doing so would push the day over the cap.
    /// On success the reserve is **not** persisted — that happens in
    /// `record()` after the actual cost is known. Reservation is just a
    /// gate.
    pub fn check_and_reserve(&self, est_usd: f64) -> Result<(), AiError> {
        let est = est_usd.max(0.0);
        let already = self.current_spend();
        if already + est > self.cap_usd {
            return Err(AiError::BudgetExceeded);
        }
        Ok(())
    }

    /// Persist `actual_usd` onto today's running total via tmp + rename.
    pub fn record(&self, actual_usd: f64) -> Result<(), AiError> {
        let amount = actual_usd.max(0.0);
        if amount == 0.0 {
            return Ok(());
        }
        let path = self.path_for_today();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let prev = read_amount(&path).unwrap_or(0.0);
        let next = prev + amount;
        atomic_write(&path, &format!("{next:.6}\n"))?;
        Ok(())
    }

    /// Compute the dated file path. Local date is used so a user in
    /// Asia/Seoul rolls over at local midnight, not UTC midnight.
    pub fn path_for_today(&self) -> PathBuf {
        let date = Local::now().format("%Y-%m-%d").to_string();
        self.root.join(format!("ai-budget-{date}.txt"))
    }
}

fn read_amount(path: &Path) -> Option<f64> {
    let text = fs::read_to_string(path).ok()?;
    text.trim().parse::<f64>().ok()
}

fn atomic_write(target: &Path, body: &str) -> Result<(), AiError> {
    let tmp = match target.extension() {
        Some(_) => target.with_extension("txt.tmp"),
        None => target.with_extension("tmp"),
    };
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(body.as_bytes())?;
        f.sync_all().ok();
    }
    fs::rename(&tmp, target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn record_persists_and_reads() {
        let tmp = tempdir().unwrap();
        let b = Budget::new(tmp.path().to_path_buf(), 1.0);
        b.record(0.10).unwrap();
        assert!((b.current_spend() - 0.10).abs() < 1e-9);
        b.record(0.05).unwrap();
        assert!((b.current_spend() - 0.15).abs() < 1e-9);
    }

    #[test]
    fn reserve_blocks_over_cap() {
        let tmp = tempdir().unwrap();
        let b = Budget::new(tmp.path().to_path_buf(), 0.20);
        b.record(0.15).unwrap();
        assert!(b.check_and_reserve(0.04).is_ok());
        match b.check_and_reserve(0.10) {
            Err(AiError::BudgetExceeded) => (),
            other => panic!("expected BudgetExceeded, got {other:?}"),
        }
    }

    #[test]
    fn fresh_dir_starts_at_zero() {
        let tmp = tempdir().unwrap();
        let b = Budget::new(tmp.path().to_path_buf(), 1.0);
        assert_eq!(b.current_spend(), 0.0);
    }
}
