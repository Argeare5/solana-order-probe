//! Spend ledger and cap. Every fee the probe pays (probes, funding, sweeping) is appended to `spend_ledger.jsonl`
//! with the fee read from the chain, not estimated. Before a burst goes out its worst case is reserved; the
//! reservation is released when the burst is collected and its real fees are recorded. A burst is refused if
//! spent + reserved + its worst case would pass the cap.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Entry {
    pub at: String,
    pub kind: String,
    pub signature: String,
    pub fee_lamports: u64,
}

struct State {
    path: PathBuf,
    spent: u64,
    reserved: u64,
    cap: u64,
}

#[derive(Clone)]
pub struct Ledger(Arc<Mutex<State>>);

impl Ledger {
    pub fn open(data_dir: &Path, cap_sol: f64) -> Result<Self> {
        let path = data_dir.join("spend_ledger.jsonl");
        let mut spent = 0;
        if path.exists() {
            for line in std::fs::read_to_string(&path)?.lines().filter(|l| !l.trim().is_empty()) {
                spent += serde_json::from_str::<Entry>(line)?.fee_lamports;
            }
        }
        let cap = (cap_sol * 1e9).round() as u64;
        Ok(Self(Arc::new(Mutex::new(State { path, spent, reserved: 0, cap }))))
    }

    pub fn spent(&self) -> u64 {
        self.0.lock().unwrap().spent
    }

    pub fn cap(&self) -> u64 {
        self.0.lock().unwrap().cap
    }

    /// Reserves `worst_case` lamports, or fails if the cap would be passed.
    pub fn reserve(&self, worst_case: u64) -> Result<()> {
        let mut state = self.0.lock().unwrap();
        if state.spent + state.reserved + worst_case > state.cap {
            bail!(
                "spend cap: spent {} + reserved {} + this burst {} > cap {} lamports",
                state.spent,
                state.reserved,
                worst_case,
                state.cap
            );
        }
        state.reserved += worst_case;
        Ok(())
    }

    pub fn release(&self, reserved: u64) {
        let mut state = self.0.lock().unwrap();
        state.reserved = state.reserved.saturating_sub(reserved);
    }

    pub fn record(&self, kind: &str, signature: &str, fee_lamports: u64) -> Result<()> {
        let mut state = self.0.lock().unwrap();
        let entry = Entry { at: chrono::Utc::now().to_rfc3339(), kind: kind.to_owned(), signature: signature.to_owned(), fee_lamports };
        crate::store::append_jsonl(&state.path, &entry)?;
        state.spent += fee_lamports;
        Ok(())
    }
}
