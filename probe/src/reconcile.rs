//! Reconciliation of live runs against the chain. Every probe that was sent is checked once it can no longer land
//! (its blockhash has expired): a probe that landed but whose fee is not in the spend ledger (its collection failed,
//! or it landed after collection, possibly in another leader's block) gets its real fee recorded, and a burst whose
//! collection never finished is collected now. Each probe checked is appended to `reconciled.jsonl`, so the ledger
//! holds every fee paid and the next run starts from the true amount spent. `run --live` reconciles all earlier runs
//! before it sends anything and its own run at the end.

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chain;
use crate::clock::unix_us;
use crate::collect;
use crate::ledger::Ledger;
use crate::rpc::Rpc;
use crate::run::BurstRecord;
use crate::store::{append_jsonl, read_jsonl};

/// A probe's blockhash lives ~150 blocks (~35 s); after this long an unlanded probe never lands.
pub const SETTLE: Duration = Duration::from_secs(120);

#[derive(Serialize, Deserialize)]
struct Checked {
    signature: String,
    burst: usize,
    index: usize,
    landed_slot: Option<u64>,
    in_window: bool,
    fee_lamports: Option<u64>,
    err: Option<Value>,
    checked_at: String,
}

/// Reconciles every live run under `data_dir/runs`.
pub async fn all(rpc: &Rpc, data_dir: &Path, ledger: &Ledger) -> Result<()> {
    let mut dirs: Vec<_> = std::fs::read_dir(data_dir.join("runs"))
        .map(|entries| entries.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    dirs.sort();
    for dir in dirs {
        if dir.join("run.json").exists() {
            run_dir(rpc, &dir, ledger).await.with_context(|| format!("reconciling {}", dir.display()))?;
        }
    }
    Ok(())
}

/// Reconciles one run directory (a dry-run has nothing to reconcile).
pub async fn run_dir(rpc: &Rpc, dir: &Path, ledger: &Ledger) -> Result<()> {
    let run: Value = serde_json::from_slice(&std::fs::read(dir.join("run.json"))?)?;
    if run["live"] != true {
        return Ok(());
    }
    let settled_before = unix_us() - SETTLE.as_micros() as i64;
    let bursts: Vec<BurstRecord> = read_jsonl(&dir.join("bursts.jsonl"))?;
    let bursts: Vec<&BurstRecord> = bursts.iter().filter(|b| b.live && b.fire_unix_us < settled_before).collect();

    // Bursts whose collection never finished are collected now
    let collected: HashSet<usize> =
        read_jsonl::<Value>(&dir.join("results.jsonl"))?.iter().filter_map(|r| r["burst"].as_u64()).map(|b| b as usize).collect();
    let mut late_collections = 0;
    for burst in bursts.iter().filter(|b| !collected.contains(&b.burst)) {
        match collect::collect(rpc, dir, burst, ledger).await {
            Ok(()) => late_collections += 1,
            Err(error) => eprintln!("  burst {}: collection failed again: {error:#}", burst.burst),
        }
    }

    // Every probe whose fee is not in the ledger and that was not checked before
    let checked: HashSet<String> =
        read_jsonl::<Checked>(&dir.join("reconciled.jsonl"))?.into_iter().map(|c| c.signature).collect();
    let pending: Vec<(&BurstRecord, usize, &str)> = bursts
        .iter()
        .flat_map(|b| b.probes.iter().map(move |p| (*b, p.index, p.signature.as_str())))
        .filter(|(_, _, signature)| !ledger.is_recorded(signature) && !checked.contains(*signature))
        .collect();
    let (mut landed, mut added) = (0, 0u64);
    for chunk in pending.chunks(256) {
        let signatures: Vec<String> = chunk.iter().map(|(_, _, s)| s.to_string()).collect();
        let statuses = chain::landed(&chain::statuses(rpc, &signatures).await?);
        for ((burst, index, signature), status) in chunk.iter().zip(&statuses) {
            let landed_slot = status["slot"].as_u64();
            let mut fee = None;
            if landed_slot.is_some() {
                // Without its fee the probe stays unchecked and the next reconciliation tries again
                fee = Some(chain::fee_of(rpc, signature).await?.context("a landed probe without a fee")?);
                ledger.record("probe", signature, fee.unwrap())?;
                landed += 1;
                added += fee.unwrap();
            }
            let window = burst.window.start..=burst.window.end();
            append_jsonl(
                &dir.join("reconciled.jsonl"),
                &Checked {
                    signature: signature.to_string(),
                    burst: burst.burst,
                    index: *index,
                    landed_slot,
                    in_window: landed_slot.is_some_and(|s| window.contains(&s)),
                    fee_lamports: fee,
                    err: status.get("err").filter(|e| !e.is_null()).cloned(),
                    checked_at: chrono::Utc::now().to_rfc3339(),
                },
            )?;
        }
    }
    if late_collections + pending.len() > 0 {
        println!(
            "reconciled {}: {late_collections} bursts collected now; {} probes checked, {landed} landed after collection, {added} lamports of fees added",
            dir.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(),
            pending.len()
        );
    }
    Ok(())
}
