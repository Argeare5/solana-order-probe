//! Small RPC helpers on top of `Rpc`: blockhash, balances, sending a transaction the ordinary way (for funding and
//! sweeping, never for probes) and waiting for signatures.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::Engine;
use serde_json::{Value, json};
use solana_hash::Hash;

use crate::rpc::Rpc;

pub async fn latest_blockhash(rpc: &Rpc) -> Result<Hash> {
    let result = rpc.call("getLatestBlockhash", json!([{ "commitment": "confirmed" }])).await?;
    let text = result["value"]["blockhash"].as_str().context("blockhash")?;
    text.parse().map_err(|e| anyhow::anyhow!("blockhash {text}: {e:?}"))
}

pub async fn balance(rpc: &Rpc, pubkey: &str) -> Result<u64> {
    let result = rpc.call("getBalance", json!([pubkey, { "commitment": "confirmed" }])).await?;
    result["value"].as_u64().context("balance")
}

pub async fn rent_exempt_minimum(rpc: &Rpc, data_len: usize) -> Result<u64> {
    rpc.call("getMinimumBalanceForRentExemption", json!([data_len])).await?.as_u64().context("rent")
}

/// Sends through the RPC node with preflight, as an ordinary client does.
pub async fn send(rpc: &Rpc, wire: &[u8]) -> Result<String> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(wire);
    let result = rpc
        .call("sendTransaction", json!([encoded, { "encoding": "base64", "preflightCommitment": "confirmed" }]))
        .await?;
    Ok(result.as_str().context("signature")?.to_owned())
}

/// Statuses of `signatures` (null for unknown ones), searching the history so late landings are found too.
pub async fn statuses(rpc: &Rpc, signatures: &[String]) -> Result<Vec<Value>> {
    let result = rpc.call("getSignatureStatuses", json!([signatures, { "searchTransactionHistory": true }])).await?;
    Ok(result["value"].as_array().context("statuses")?.clone())
}

/// Waits until every signature is confirmed or `timeout` passes; fails if any transaction failed or never landed.
pub async fn confirm(rpc: &Rpc, signatures: &[String], timeout: Duration) -> Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let list = statuses(rpc, signatures).await?;
        let mut pending = 0;
        for (signature, status) in signatures.iter().zip(&list) {
            if status.is_null() {
                pending += 1;
            } else if !status["err"].is_null() {
                bail!("{signature} failed: {}", status["err"]);
            } else if !matches!(status["confirmationStatus"].as_str(), Some("confirmed" | "finalized")) {
                pending += 1;
            }
        }
        if pending == 0 {
            return Ok(());
        }
        if tokio::time::Instant::now() > deadline {
            bail!("{pending} of {} transactions not confirmed in time", signatures.len());
        }
        tokio::time::sleep(Duration::from_millis(800)).await;
    }
}

/// The fee a landed transaction paid, in lamports.
pub async fn fee_of(rpc: &Rpc, signature: &str) -> Result<Option<u64>> {
    let result = rpc
        .call("getTransaction", json!([signature, { "commitment": "confirmed", "maxSupportedTransactionVersion": 1, "encoding": "json" }]))
        .await?;
    Ok(result["meta"]["fee"].as_u64())
}
