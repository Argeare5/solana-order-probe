//! Collection of a live burst once its window has passed: where each probe landed (slot, index in the block, fee),
//! the compact blocks that hold probes, and the real fees into the spend ledger.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use serde::Serialize;
use serde_json::{Value, json};

use crate::chain;
use crate::ledger::Ledger;
use crate::rpc::Rpc;
use crate::run::BurstRecord;

#[derive(Serialize)]
struct ProbeResult {
    index: usize,
    signature: String,
    landed_slot: Option<u64>,
    in_window: bool,
    block_index: Option<usize>,
    block_transactions: Option<usize>,
    fee_lamports: Option<u64>,
    err: Option<Value>,
}

#[derive(Serialize)]
struct BurstResult {
    run_id: String,
    burst: usize,
    identity: String,
    client: String,
    window_start: u64,
    window_slots: u64,
    landed: usize,
    landed_in_window: usize,
    results: Vec<ProbeResult>,
}

pub async fn burst(rpc: Rpc, run_dir: PathBuf, burst: BurstRecord, ledger: Ledger, at: std::time::Instant) -> Result<()> {
    tokio::time::sleep_until(at.into()).await;
    let signatures: Vec<String> = burst.probes.iter().map(|p| p.signature.clone()).collect();

    // Statuses, asked again for a while: a probe can land a few slots after its window
    let mut statuses = chain::statuses(&rpc, &signatures).await?;
    for _ in 0..3 {
        if statuses.iter().all(|s| !s.is_null()) {
            break;
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
        statuses = chain::statuses(&rpc, &signatures).await?;
    }

    // Every block that holds a probe, kept in compact form
    let landed_slots: BTreeSet<u64> = statuses.iter().filter_map(|s| s["slot"].as_u64()).collect();
    let mut positions: HashMap<String, (u64, usize, usize, Option<u64>)> = HashMap::new();
    for slot in &landed_slots {
        let block = fetch_block(&rpc, *slot).await?;
        let path = run_dir.join("blocks").join(format!("{slot}.json.gz"));
        if !path.exists() {
            std::fs::create_dir_all(path.parent().unwrap())?;
            crate::store::write_json_gz(&path, &block)?;
        }
        let count = block["transactions"].as_array().map_or(0, Vec::len);
        for (index, tx) in block["transactions"].as_array().into_iter().flatten().enumerate() {
            if let Some(signature) = tx["signature"].as_str() {
                if signatures.iter().any(|s| s == signature) {
                    positions.insert(signature.to_owned(), (*slot, index, count, tx["fee"].as_u64()));
                }
            }
        }
    }

    let window = burst.window.start..=burst.window.end();
    let mut results = Vec::new();
    for (probe, status) in burst.probes.iter().zip(&statuses) {
        let position = positions.get(&probe.signature);
        let landed_slot = status["slot"].as_u64();
        let fee = position.and_then(|p| p.3);
        if let (Some(_), Some(fee)) = (landed_slot, fee) {
            ledger.record("probe", &probe.signature, fee)?;
        }
        results.push(ProbeResult {
            index: probe.index,
            signature: probe.signature.clone(),
            landed_slot,
            in_window: landed_slot.is_some_and(|s| window.contains(&s)),
            block_index: position.map(|p| p.1),
            block_transactions: position.map(|p| p.2),
            fee_lamports: fee,
            err: status.get("err").filter(|e| !e.is_null()).cloned(),
        });
    }
    ledger.release(burst.worst_case_lamports);

    let result = BurstResult {
        run_id: burst.run_id.clone(),
        burst: burst.burst,
        identity: burst.identity.clone(),
        client: burst.client.clone(),
        window_start: burst.window.start,
        window_slots: burst.window.slots,
        landed: results.iter().filter(|r| r.landed_slot.is_some()).count(),
        landed_in_window: results.iter().filter(|r| r.in_window).count(),
        results,
    };
    println!(
        "  burst {} collected: {} of {} landed, {} in the window",
        result.burst,
        result.landed,
        burst.probes.len(),
        result.landed_in_window
    );
    crate::store::append_jsonl(&run_dir.join("results.jsonl"), &result)?;
    Ok(())
}

/// A block in compact form: its transactions in block order with signature, fee, compute units consumed, error flag,
/// version and, when the message carries them, the compute budget limit and price.
async fn fetch_block(rpc: &Rpc, slot: u64) -> Result<Value> {
    let params = json!([slot, {
        "encoding": "json",
        "transactionDetails": "full",
        "maxSupportedTransactionVersion": 1,
        "rewards": false,
        "commitment": "confirmed"
    }]);
    let block = rpc.call("getBlock", params).await?;
    let txs: Vec<Value> = block["transactions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|tx| {
            let (cu_limit, cu_price) = compute_budget(&tx["transaction"]["message"]);
            json!({
                "signature": tx["transaction"]["signatures"][0],
                "fee": tx["meta"]["fee"],
                "cu": tx["meta"]["computeUnitsConsumed"],
                "err": !tx["meta"]["err"].is_null(),
                "version": tx["version"],
                "cu_limit": cu_limit,
                "cu_price": cu_price,
            })
        })
        .collect();
    Ok(json!({
        "slot": slot,
        "blockhash": block["blockhash"],
        "parent_slot": block["parentSlot"],
        "block_time": block["blockTime"],
        "block_height": block["blockHeight"],
        "transactions": txs,
    }))
}

/// Compute unit limit and price from a JSON-encoded message (legacy and v0 layout), if present.
fn compute_budget(message: &Value) -> (Option<u32>, Option<u64>) {
    const COMPUTE_BUDGET: &str = "ComputeBudget111111111111111111111111111111";
    let keys = message["accountKeys"].as_array();
    let mut limit = None;
    let mut price = None;
    for ix in message["instructions"].as_array().into_iter().flatten() {
        let program = ix["programIdIndex"].as_u64().and_then(|i| keys?.get(i as usize)).and_then(Value::as_str);
        if program != Some(COMPUTE_BUDGET) {
            continue;
        }
        let Some(data) = ix["data"].as_str().and_then(|d| bs58::decode(d).into_vec().ok()) else { continue };
        match data.first() {
            Some(2) if data.len() >= 5 => limit = Some(u32::from_le_bytes(data[1..5].try_into().unwrap())),
            Some(3) if data.len() >= 9 => price = Some(u64::from_le_bytes(data[1..9].try_into().unwrap())),
            _ => {}
        }
    }
    (limit, price)
}
