//! Payer pool management. `fund` tops every payer up to the rent-exempt minimum plus a fee float; `sweep` returns
//! every lamport to the funder at the end (the payer accounts close). Both are dry-runs unless `--live` is given,
//! and their fees go into the spend ledger like the probes' fees.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

use crate::chain;
use crate::keys::{self, PayerPool};
use crate::ledger::Ledger;
use crate::rpc::Rpc;

#[derive(clap::Args)]
pub struct FundArgs {
    /// Payers in the pool (one per probe of a burst).
    #[arg(long, default_value_t = 12)]
    payers: usize,
    /// Fee float per payer on top of the rent-exempt minimum, in SOL.
    #[arg(long, default_value_t = 0.003)]
    float_sol: f64,
    /// Send the funding transactions. Without it the plan is only printed.
    #[arg(long)]
    live: bool,
}

#[derive(clap::Args)]
pub struct SweepArgs {
    #[arg(long, default_value_t = 12)]
    payers: usize,
    /// After the payers, send the funder's whole balance to this address too (the funder account closes).
    #[arg(long)]
    to: Option<String>,
    /// Send the sweep transactions. Without it the plan is only printed.
    #[arg(long)]
    live: bool,
}

#[derive(clap::Args)]
pub struct AirdropArgs {
    /// SOL to request for the funder (devnet, testnet or a local validator).
    #[arg(long, default_value_t = 1.0)]
    sol: f64,
}

pub fn funder_path(secrets: &Path) -> std::path::PathBuf {
    secrets.join("funder.json")
}

pub async fn fund(rpc: &Rpc, secrets: &Path, ledger: &Ledger, args: FundArgs) -> Result<()> {
    let funder = keys::load(&funder_path(secrets)).context("the funder keypair (secrets/funder.json)")?;
    let pool = PayerPool::load_or_create(secrets, args.payers)?;
    let rent = chain::rent_exempt_minimum(rpc, 0).await?;
    let target = rent + (args.float_sol * 1e9).round() as u64;

    let mut transfers: Vec<(Pubkey, u64)> = Vec::new();
    for payer in &pool.payers {
        let balance = chain::balance(rpc, &payer.pubkey().to_string()).await?;
        if balance < target {
            transfers.push((payer.pubkey(), target - balance));
        }
    }
    let needed: u64 = transfers.iter().map(|(_, l)| l).sum();
    let funder_balance = chain::balance(rpc, &funder.pubkey().to_string()).await?;
    println!(
        "funder {} has {:.6} SOL; {} payers need {:.6} SOL (rent-exempt {} + float {} lamports each)",
        funder.pubkey(),
        funder_balance as f64 / 1e9,
        transfers.len(),
        needed as f64 / 1e9,
        rent,
        target - rent
    );
    if transfers.is_empty() {
        return Ok(());
    }
    if !args.live {
        println!("dry-run: nothing sent (add --live to fund)");
        return Ok(());
    }
    if funder_balance < needed + 50_000 {
        bail!("the funder does not have enough SOL");
    }
    // Up to 10 transfers per transaction keeps it well under the packet size
    for chunk in transfers.chunks(10) {
        let instructions: Vec<_> =
            chunk.iter().map(|(to, lamports)| solana_system_interface::instruction::transfer(&funder.pubkey(), to, *lamports)).collect();
        let tx = Transaction::new_signed_with_payer(&instructions, Some(&funder.pubkey()), &[&funder], chain::latest_blockhash(rpc).await?);
        submit(rpc, ledger, "fund", &tx).await?;
    }
    println!("funded");
    Ok(())
}

pub async fn sweep(rpc: &Rpc, secrets: &Path, ledger: &Ledger, args: SweepArgs) -> Result<()> {
    let funder = keys::load(&funder_path(secrets)).context("the funder keypair (secrets/funder.json)")?;
    let pool = PayerPool::load_or_create(secrets, args.payers)?;
    let mut holders: Vec<(&Keypair, u64)> = Vec::new();
    for payer in &pool.payers {
        let balance = chain::balance(rpc, &payer.pubkey().to_string()).await?;
        if balance > 0 {
            holders.push((payer, balance));
        }
    }
    let total: u64 = holders.iter().map(|(_, b)| b).sum();
    println!("{} payers hold {:.6} SOL in total", holders.len(), total as f64 / 1e9);
    let to: Option<Pubkey> = match &args.to {
        Some(text) => Some(text.parse().map_err(|e| anyhow::anyhow!("--to {text}: {e:?}"))?),
        None => None,
    };
    if let Some(to) = &to {
        println!("then the funder {} sends its whole balance to {to}", funder.pubkey());
    }
    if !args.live {
        println!("dry-run: nothing sent (add --live to sweep)");
        return Ok(());
    }
    // The funder pays the fees, so every payer can send its whole balance and close
    for chunk in holders.chunks(4) {
        let instructions: Vec<_> = chunk
            .iter()
            .map(|(payer, balance)| solana_system_interface::instruction::transfer(&payer.pubkey(), &funder.pubkey(), *balance))
            .collect();
        let mut signers: Vec<&Keypair> = vec![&funder];
        signers.extend(chunk.iter().map(|(payer, _)| *payer));
        let tx = Transaction::new_signed_with_payer(&instructions, Some(&funder.pubkey()), &signers, chain::latest_blockhash(rpc).await?);
        submit(rpc, ledger, "sweep", &tx).await?;
    }
    if let Some(to) = to {
        const FEE: u64 = 5_000;
        let balance = chain::balance(rpc, &funder.pubkey().to_string()).await?;
        if balance > FEE {
            let ix = solana_system_interface::instruction::transfer(&funder.pubkey(), &to, balance - FEE);
            let tx = Transaction::new_signed_with_payer(&[ix], Some(&funder.pubkey()), &[&funder], chain::latest_blockhash(rpc).await?);
            submit(rpc, ledger, "sweep", &tx).await?;
            println!("sent {:.9} SOL from the funder to {to}", (balance - FEE) as f64 / 1e9);
        }
    }
    println!("swept");
    Ok(())
}

pub async fn airdrop(rpc: &Rpc, secrets: &Path, args: AirdropArgs) -> Result<()> {
    let (_, cluster) = chain::cluster(rpc).await?;
    if cluster == "mainnet-beta" {
        bail!("there are no airdrops on mainnet");
    }
    let funder = keys::load(&funder_path(secrets)).context("the funder keypair (create one with `keys --new-funder`)")?;
    let lamports = (args.sol * 1e9).round() as u64;
    let result = rpc.call("requestAirdrop", serde_json::json!([funder.pubkey().to_string(), lamports])).await?;
    let signature = result.as_str().context("airdrop signature")?.to_owned();
    chain::confirm(rpc, std::slice::from_ref(&signature), Duration::from_secs(60)).await?;
    let balance = chain::balance(rpc, &funder.pubkey().to_string()).await?;
    println!("airdrop {signature}: funder {} now holds {:.6} SOL on {cluster}", funder.pubkey(), balance as f64 / 1e9);
    Ok(())
}

async fn submit(rpc: &Rpc, ledger: &Ledger, kind: &str, tx: &Transaction) -> Result<()> {
    let signature = chain::send(rpc, &wincode::serialize(tx)?).await?;
    chain::confirm(rpc, std::slice::from_ref(&signature), Duration::from_secs(60)).await?;
    let fee = chain::fee_of(rpc, &signature).await?.unwrap_or(5_000 * tx.signatures.len() as u64);
    ledger.record(kind, &signature, fee)?;
    println!("{kind}: {signature} (fee {fee} lamports)");
    Ok(())
}
