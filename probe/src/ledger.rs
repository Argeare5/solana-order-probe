//! Spend ledger and cap. Every fee the probe pays (probes, funding, sweeping) is appended to `spend_ledger.jsonl`
//! with the fee read from the chain, not estimated. The ledger belongs to the keys that pay: the copy in the
//! secrets directory counts for the cap, and the copy in the data directory is the published record. Both are merged
//! by signature when the ledger opens, so a run pointed at another data directory still sees everything these keys
//! have spent. Before a burst goes out its worst case is reserved; the
//! reservation is released when the burst is collected and its real fees are recorded. A burst is refused if
//! spent + reserved + its worst case would pass the cap. A signature is recorded once, so a later reconciliation can
//! add fees that collection missed without counting any twice.

use std::collections::HashSet;
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
    paths: Vec<PathBuf>,
    spent: u64,
    reserved: u64,
    cap: u64,
    recorded: HashSet<String>,
}

#[derive(Clone)]
pub struct Ledger(Arc<Mutex<State>>);

impl Ledger {
    pub fn open(secrets_dir: &Path, data_dir: &Path, cap_sol: f64) -> Result<Self> {
        let mut paths = vec![secrets_dir.join("spend_ledger.jsonl"), data_dir.join("spend_ledger.jsonl")];
        paths.dedup();
        let copies: Vec<Vec<Entry>> = paths.iter().map(|p| crate::store::read_jsonl(p)).collect::<Result<_>>()?;
        let mut spent = 0;
        let mut recorded = HashSet::new();
        let mut union = Vec::new();
        for entry in copies.iter().flatten() {
            if recorded.insert(entry.signature.clone()) {
                spent += entry.fee_lamports;
                union.push(entry);
            }
        }
        // Each copy gets the entries it lacks, so both hold the whole history of these keys
        for (path, copy) in paths.iter().zip(&copies) {
            let present: HashSet<&str> = copy.iter().map(|e| e.signature.as_str()).collect();
            for entry in union.iter().filter(|e| !present.contains(e.signature.as_str())) {
                crate::store::append_jsonl(path, entry)?;
            }
        }
        let cap = (cap_sol * 1e9).round() as u64;
        Ok(Self(Arc::new(Mutex::new(State { paths, spent, reserved: 0, cap, recorded }))))
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

    pub fn is_recorded(&self, signature: &str) -> bool {
        self.0.lock().unwrap().recorded.contains(signature)
    }

    /// Records the fee a landed transaction paid; a signature already in the ledger is not counted again.
    pub fn record(&self, kind: &str, signature: &str, fee_lamports: u64) -> Result<()> {
        let mut state = self.0.lock().unwrap();
        if state.recorded.contains(signature) {
            return Ok(());
        }
        let entry = Entry { at: chrono::Utc::now().to_rfc3339(), kind: kind.to_owned(), signature: signature.to_owned(), fee_lamports };
        for path in &state.paths {
            crate::store::append_jsonl(path, &entry)?;
        }
        state.spent += fee_lamports;
        state.recorded.insert(entry.signature);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_reservations_and_counts_each_signature_once() {
        let dir = std::env::temp_dir().join(format!("sop-ledger-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ledger = Ledger::open(&dir, &dir, 0.0001).unwrap(); // 100 000 lamports
        ledger.reserve(60_000).unwrap();
        assert!(ledger.reserve(50_000).is_err(), "spent 0 + reserved 60 000 + 50 000 > cap");
        ledger.record("probe", "sig-a", 7_000).unwrap();
        ledger.record("probe", "sig-a", 7_000).unwrap();
        assert_eq!(ledger.spent(), 7_000);
        ledger.release(60_000);
        assert!(ledger.reserve(93_000).is_ok());
        assert!(ledger.reserve(1).is_err());

        let reopened = Ledger::open(&dir, &dir, 0.0001).unwrap();
        assert_eq!(reopened.spent(), 7_000);
        assert!(reopened.is_recorded("sig-a"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn another_data_directory_still_sees_what_the_keys_spent() {
        let root = std::env::temp_dir().join(format!("sop-ledger-dirs-{}", std::process::id()));
        let (secrets, data, other) = (root.join("secrets"), root.join("data"), root.join("elsewhere"));
        for dir in [&secrets, &data, &other] {
            std::fs::create_dir_all(dir).unwrap();
        }
        // Spent with the project's data directory...
        Ledger::open(&secrets, &data, 0.0001).unwrap().record("probe", "sig-a", 60_000).unwrap();
        // ...still counts when the same keys run against another one, which also receives the entry
        let elsewhere = Ledger::open(&secrets, &other, 0.0001).unwrap();
        assert_eq!(elsewhere.spent(), 60_000);
        assert!(elsewhere.reserve(50_000).is_err());
        assert_eq!(crate::store::read_jsonl::<Entry>(&other.join("spend_ledger.jsonl")).unwrap().len(), 1);
        // An entry only the data copy holds (written before the secrets copy existed) is merged into the other
        std::fs::remove_file(secrets.join("spend_ledger.jsonl")).unwrap();
        assert_eq!(Ledger::open(&secrets, &data, 0.0001).unwrap().spent(), 60_000);
        assert_eq!(crate::store::read_jsonl::<Entry>(&secrets.join("spend_ledger.jsonl")).unwrap().len(), 1);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
