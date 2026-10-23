//! `simulate`: runs a probe through `simulateTransaction` and reports the compute units it consumes, so that
//! `--cu-limit` is just above what a probe needs (the priority fee is limit × price). The probe is simulated
//! unsigned (`sigVerify: false`) with a fresh blockhash; its fee payer only has to exist and hold some SOL, so any
//! funded account works and no key is needed. Read-only: nothing is sent.

use std::path::Path;

use anyhow::{Context, Result, bail};
use base64::Engine;
use serde_json::{Value, json};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

use crate::probe::{self, ProbeSpec};
use crate::rpc::Rpc;
use crate::{fund, keys};

#[derive(clap::Args)]
pub struct Args {
    /// Fee payer of the simulated probe: any funded system account (default: the funder in the secrets directory).
    #[arg(long)]
    fee_payer: Option<String>,

    /// Compute unit price of the simulated probe, in micro-lamports.
    #[arg(long, default_value_t = 5_000_000)]
    cu_price: u64,

    /// Headroom over the consumed units for the suggested limit, as a fraction.
    #[arg(long, default_value_t = 0.1)]
    headroom: f64,
}

pub async fn run(rpc: &Rpc, secrets: &Path, args: Args) -> Result<()> {
    let payer: Pubkey = match &args.fee_payer {
        Some(text) => text.parse().map_err(|e| anyhow::anyhow!("--fee-payer {text}: {e:?}"))?,
        None => keys::load(&fund::funder_path(secrets))
            .context("no --fee-payer given and no funder keypair to take it from")?
            .pubkey(),
    };
    // The longest tag a run writes (run id, burst number up to 999, probe index up to 99)
    let tag = "sop:20261023T080130Z:999:99".to_owned();
    let spec = |cu_limit| ProbeSpec { tag: tag.clone(), cu_limit, cu_price: args.cu_price, target: Pubkey::new_unique() };

    let measured = simulate(rpc, &payer, &spec(200_000)).await?;
    if !measured["err"].is_null() {
        print_logs(&measured);
        bail!("the probe fails in simulation: {}", measured["err"]);
    }
    let consumed = measured["unitsConsumed"].as_u64().context("unitsConsumed")?;
    let suggested = ((consumed as f64 * (1.0 + args.headroom)) / 50.0).ceil() as u32 * 50;
    let check = simulate(rpc, &payer, &spec(suggested)).await?;
    print_logs(&measured);
    println!("consumed {consumed} CU; suggested --cu-limit {suggested}: {}", if check["err"].is_null() { "passes".to_owned() } else { format!("fails: {}", check["err"]) });
    println!(
        "max fee per probe at {} µlamports/CU: {} lamports",
        args.cu_price,
        probe::max_fee_lamports(&spec(suggested))
    );
    Ok(())
}

async fn simulate(rpc: &Rpc, payer: &Pubkey, spec: &ProbeSpec) -> Result<Value> {
    let tx = Transaction::new_with_payer(&probe::instructions(payer, spec), Some(payer));
    let wire = base64::engine::general_purpose::STANDARD.encode(wincode::serialize(&tx)?);
    let params = json!([wire, {
        "encoding": "base64",
        "sigVerify": false,
        "replaceRecentBlockhash": true,
        "commitment": "confirmed"
    }]);
    Ok(rpc.call("simulateTransaction", params).await?["value"].clone())
}

fn print_logs(result: &Value) {
    for line in result["logs"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        println!("  {line}");
    }
}
