mod calibrate;
mod chain;
mod clock;
mod collect;
mod fund;
mod keys;
mod ledger;
mod probe;
mod quic;
mod recon;
mod reconcile;
mod rpc;
mod run;
mod schedule;
mod simulate;
mod store;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Sends probe transactions into chosen Solana leader windows and records where they land in the block.
#[derive(Parser)]
#[command(name = "order-probe", version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("ORDER_PROBE_COMMIT"), ")"))]
struct Cli {
    /// JSON-RPC endpoint. Measurements are taken on mainnet.
    #[arg(long, env = "RPC_URL", default_value = "https://api.mainnet-beta.solana.com", global = true)]
    rpc_url: String,

    /// Directory for raw data (snapshots, runs, blocks, the spend ledger).
    #[arg(long, env = "DATA_DIR", default_value = "data", global = true)]
    data_dir: PathBuf,

    /// Directory with the keypairs (funder.json, payers/). Never committed.
    #[arg(long, env = "SECRETS_DIR", default_value = "secrets", global = true)]
    secrets_dir: PathBuf,

    /// Hard cap on fees spent by live commands, in SOL.
    #[arg(long, env = "SPEND_CAP_SOL", default_value_t = 0.05, global = true)]
    spend_cap_sol: f64,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Snapshots the cluster (clients, stake, leader schedule, slot time, Alpenglow state) and lists candidate
    /// validators per client with their next leader windows. Read-only.
    Recon(recon::Args),
    /// Opens a QUIC connection to the TPU of each given address and reports the handshake time. Sends nothing.
    QuicPing {
        /// TPU QUIC addresses (ip:port)
        #[arg(required = true)]
        peers: Vec<std::net::SocketAddr>,
    },
    /// Shows the funder and the payer pool with balances (creates missing payer keypairs).
    Keys {
        #[arg(long, default_value_t = 12)]
        payers: usize,
        /// Create a fresh funder keypair if there is none. Fund its address from your wallet with what the runs
        /// need, and `sweep --live --to <your wallet>` returns what is left: your wallet's key never enters the tool.
        #[arg(long)]
        new_funder: bool,
    },
    /// Requests an airdrop for the funder (not on mainnet).
    Airdrop(fund::AirdropArgs),
    /// Tops the payers up from the funder (dry-run unless --live).
    Fund(fund::FundArgs),
    /// Returns every payer's lamports to the funder, and with --to the funder's to that address (dry-run unless --live).
    Sweep(fund::SweepArgs),
    /// Sends bursts of probes into the leader windows of the targets and collects where they land (dry-run unless --live).
    Run(run::Args),
    /// Records the slot notifications for a while and measures how well the slot clock predicts window starts.
    /// Read-only.
    Calibrate(calibrate::Args),
    /// Simulates a probe through the RPC node and reports the compute units it consumes (sets --cu-limit). Read-only.
    Simulate(simulate::Args),
    /// Checks every probe of the live runs against the chain and records any fee the spend ledger is missing
    /// (`run --live` does this itself before and after it sends). Sends nothing.
    Reconcile,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    // Both rustls backends are linked (through different dependencies): use the one Agave's TLS utils use
    let _ = rustls::crypto::CryptoProvider::install_default(solana_tls_utils::crypto_provider());
    let rpc = rpc::Rpc::new(&cli.rpc_url)?;
    let ledger = ledger::Ledger::open(&cli.secrets_dir, &cli.data_dir, cli.spend_cap_sol)?;
    match cli.command {
        Command::Recon(args) => recon::run(&rpc, &cli.data_dir, args).await,
        Command::QuicPing { peers } => {
            let client = quic::TpuClient::new()?;
            for peer in peers {
                match client.connect(peer).await {
                    Ok((connection, handshake)) => {
                        println!(
                            "{peer}: connected in {:.1} ms, rtt {:.1} ms",
                            handshake.as_secs_f64() * 1e3,
                            connection.rtt().as_secs_f64() * 1e3
                        );
                        connection.close(0u32.into(), b"done");
                    }
                    Err(error) => println!("{peer}: {error:#}"),
                }
            }
            Ok(())
        }
        Command::Keys { payers, new_funder } => {
            use solana_signer::Signer;
            let funder_path = fund::funder_path(&cli.secrets_dir);
            if new_funder && !funder_path.exists() {
                keys::save(&funder_path, &solana_keypair::Keypair::new())?;
                println!("created a new funder keypair at {}", funder_path.display());
            }
            match keys::load(&funder_path) {
                Ok(funder) => println!(
                    "funder {}  {:.6} SOL",
                    funder.pubkey(),
                    chain::balance(&rpc, &funder.pubkey().to_string()).await? as f64 / 1e9
                ),
                Err(_) => println!("no funder keypair at {}", fund::funder_path(&cli.secrets_dir).display()),
            }
            let pool = keys::PayerPool::load_or_create(&cli.secrets_dir, payers)?;
            for pubkey in pool.pubkeys() {
                println!("payer {pubkey}  {:.6} SOL", chain::balance(&rpc, &pubkey).await? as f64 / 1e9);
            }
            println!("spent {} of {} lamports", ledger.spent(), ledger.cap());
            Ok(())
        }
        Command::Airdrop(args) => fund::airdrop(&rpc, &cli.secrets_dir, args).await,
        Command::Fund(args) => fund::fund(&rpc, &cli.secrets_dir, &ledger, args).await,
        Command::Sweep(args) => fund::sweep(&rpc, &cli.secrets_dir, &ledger, args).await,
        Command::Run(args) => run::run(&rpc, &cli.data_dir, &cli.secrets_dir, &ledger, args).await,
        Command::Calibrate(args) => calibrate::run(&rpc, &cli.data_dir, args).await,
        Command::Simulate(args) => simulate::run(&rpc, &cli.secrets_dir, args).await,
        Command::Reconcile => {
            reconcile::all(&rpc, &cli.data_dir, &ledger).await?;
            println!("spent {} of {} lamports", ledger.spent(), ledger.cap());
            Ok(())
        }
    }
}
