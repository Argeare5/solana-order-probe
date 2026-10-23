//! `run`: bursts of probes into the leader windows of chosen validators.
//!
//! For each burst the tool waits for the next window of one of the targets, builds K probes with a fresh blockhash
//! (one payer each, same shape, prices and send order set by the burst's variant), opens a QUIC connection to the
//! leader's TPU and, `lead_ms` before the window starts by the slot clock, sends the probes one stream each in the
//! planned order, `spacing_ms` apart. The burst is written to `bursts.jsonl` before collection starts.
//! Without `--live` nothing is sent: the burst is planned, built and timed, and the connection is opened, so the
//! dry-run shows whether the timing hits the window.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use chrono::{SecondsFormat, Utc};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{RngExt, SeedableRng};
use serde::Serialize;
use serde_json::json;
use solana_pubkey::Pubkey;
use solana_signer::Signer;

use crate::clock::SlotClock;
use crate::keys::PayerPool;
use crate::ledger::Ledger;
use crate::probe::{self, ProbeSpec};
use crate::quic::{self, TpuClient};
use crate::rpc::Rpc;
use crate::schedule::{self, Window};
use crate::{chain, collect};

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Variant {
    /// Price ladder, random send order.
    Main,
    /// One price for every probe, random send order: the noise floor of the block.
    Null,
    /// Price ladder sent cheapest first, so the most expensive probe arrives last.
    Reverse,
    /// Price ladder, random order; half of the probes write-lock one shared account.
    Conflict,
}

#[derive(clap::Args)]
pub struct Args {
    /// WebSocket endpoint for the slot clock.
    #[arg(long, env = "WS_URL", default_value = "wss://api.mainnet-beta.solana.com")]
    ws_url: String,

    /// Leader identities to measure.
    #[arg(long, value_delimiter = ',', required = true)]
    targets: Vec<String>,

    /// Bursts per target.
    #[arg(long, default_value_t = 4)]
    bursts_per_target: usize,

    /// Variants, used in turn for each target.
    #[arg(long, value_delimiter = ',', default_value = "main,null,reverse,conflict")]
    variants: Vec<Variant>,

    /// Probes per burst (and payers in the pool).
    #[arg(long, default_value_t = 12)]
    probes: usize,

    /// Compute unit limit of every probe.
    #[arg(long, default_value_t = 1_400)]
    cu_limit: u32,

    /// Price ladder in micro-lamports per compute unit, one price per probe.
    #[arg(
        long,
        value_delimiter = ',',
        default_value = "1000,2000,5000,10000,20000,50000,100000,200000,500000,1000000,2000000,5000000"
    )]
    prices: Vec<u64>,

    /// How long before the window starts (by the slot clock) the first probe goes out, in ms.
    #[arg(long, default_value_t = 150.0)]
    lead_ms: f64,

    /// Gap between consecutive sends of a burst, in ms; used in turn.
    #[arg(long, value_delimiter = ',', default_value = "0,10,50")]
    spacings_ms: Vec<u64>,

    /// Seed of the random send orders and addresses (default: random, written to run.json).
    #[arg(long)]
    seed: Option<u64>,

    /// Stop after this many minutes even if bursts remain.
    #[arg(long, default_value_t = 180)]
    max_minutes: u64,

    /// Send the probes. Without it nothing is sent.
    #[arg(long)]
    live: bool,
}

#[derive(Serialize, Clone)]
pub struct ProbeRecord {
    pub index: usize,
    pub tag: String,
    pub signature: String,
    pub payer: String,
    pub cu_limit: u32,
    pub cu_price: u64,
    pub max_fee_lamports: u64,
    pub shared_target: bool,
    pub send_order: usize,
    /// Microseconds after the burst's fire moment at which the probe was handed to QUIC.
    pub sent_offset_us: Option<u64>,
    pub send_error: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct BurstRecord {
    pub run_id: String,
    pub burst: usize,
    pub live: bool,
    pub identity: String,
    pub client: String,
    pub version: String,
    pub tpu_quic: String,
    pub stake_sol: f64,
    pub window: Window,
    pub variant: Variant,
    pub spacing_ms: u64,
    pub lead_ms: f64,
    pub blockhash: String,
    pub fire_at: String,
    /// Slot by the clock at the fire moment (fractional), and the last notified slot.
    pub fire_slot_estimate: f64,
    pub fire_last_notified_slot: u64,
    pub quic_handshake_ms: Option<f64>,
    pub quic_rtt_ms: Option<f64>,
    pub worst_case_lamports: u64,
    pub probes: Vec<ProbeRecord>,
}

struct Target {
    identity: String,
    client: String,
    version: String,
    tpu_quic: SocketAddr,
    stake_sol: f64,
    windows: Vec<Window>,
    done: usize,
}

pub async fn run(rpc: &Rpc, data_dir: &Path, secrets: &Path, ledger: &Ledger, args: Args) -> Result<()> {
    if args.prices.len() < args.probes {
        bail!("{} prices for {} probes", args.prices.len(), args.probes);
    }
    let seed = args.seed.unwrap_or_else(|| rand::rng().random());
    let mut rng = StdRng::seed_from_u64(seed);
    let run_id = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let run_dir = data_dir.join("runs").join(&run_id);
    std::fs::create_dir_all(&run_dir)?;

    // Targets: client, version and TPU address from gossip, stake from the vote accounts, windows from the schedule
    let nodes = rpc.call("getClusterNodes", json!([])).await?;
    let votes = rpc.call("getVoteAccounts", json!([{ "commitment": "confirmed" }])).await?;
    let stake: HashMap<&str, u64> = votes["current"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| Some((v["nodePubkey"].as_str()?, v["activatedStake"].as_u64()?)))
        .collect();
    let bounds = schedule::epoch_bounds(rpc).await?;
    let mut targets = Vec::new();
    for identity in &args.targets {
        let node = nodes
            .as_array()
            .into_iter()
            .flatten()
            .find(|n| n["pubkey"].as_str() == Some(identity))
            .with_context(|| format!("{identity} is not in gossip"))?;
        let tpu_quic: SocketAddr = node["tpuQuic"].as_str().with_context(|| format!("{identity} has no TPU QUIC address"))?.parse()?;
        let mut windows = schedule::windows(rpc, identity, bounds.first_slot, bounds.first_slot).await?;
        let next_epoch = bounds.first_slot + bounds.slots;
        windows.extend(schedule::windows(rpc, identity, next_epoch, next_epoch).await.unwrap_or_default());
        targets.push(Target {
            identity: identity.clone(),
            client: node["clientId"].as_str().unwrap_or("none").to_owned(),
            version: node["version"].as_str().unwrap_or("none").to_owned(),
            tpu_quic,
            stake_sol: stake.get(identity.as_str()).copied().unwrap_or(0) as f64 / 1e9,
            windows,
            done: 0,
        });
    }

    let samples = rpc.call("getRecentPerformanceSamples", json!([10])).await?;
    let (slots, secs) = samples.as_array().into_iter().flatten().fold((0u64, 0u64), |acc, s| {
        (acc.0 + s["numSlots"].as_u64().unwrap_or(0), acc.1 + s["samplePeriodSecs"].as_u64().unwrap_or(0))
    });
    let slot_ms = secs as f64 * 1000.0 / slots.max(1) as f64;

    let pool = PayerPool::load_or_create(secrets, args.probes)?;
    if args.live {
        let rent = chain::rent_exempt_minimum(rpc, 0).await?;
        for payer in &pool.payers {
            if chain::balance(rpc, &payer.pubkey().to_string()).await? < rent + 100_000 {
                bail!("payer {} is not funded: run `fund --live` first", payer.pubkey());
            }
        }
    }

    crate::store::write_json(
        &run_dir.join("run.json"),
        &json!({
            "run_id": run_id,
            "started_at": Utc::now().to_rfc3339(),
            "live": args.live,
            "seed": seed,
            "epoch": bounds.epoch,
            "slot_ms": slot_ms,
            "probes": args.probes,
            "cu_limit": args.cu_limit,
            "prices": args.prices,
            "lead_ms": args.lead_ms,
            "spacings_ms": args.spacings_ms,
            "variants": args.variants,
            "bursts_per_target": args.bursts_per_target,
            "spend_cap_lamports": ledger.cap(),
            "spent_before_lamports": ledger.spent(),
            "payers": pool.pubkeys(),
            "targets": targets.iter().map(|t| json!({
                "identity": t.identity, "client": t.client, "version": t.version,
                "tpu_quic": t.tpu_quic.to_string(), "stake_sol": t.stake_sol,
            })).collect::<Vec<_>>(),
        }),
    )?;
    println!("run {run_id}  {}  seed {seed}  slot {slot_ms:.0} ms  cap {} lamports, spent {}", if args.live { "LIVE" } else { "dry-run" }, ledger.cap(), ledger.spent());

    let clock = SlotClock::start(args.ws_url.clone(), slot_ms).await?;
    let tpu = TpuClient::new()?;
    let deadline = Instant::now() + Duration::from_secs(args.max_minutes * 60);
    let mut collectors = tokio::task::JoinSet::new();
    let mut last_end = 0u64;
    let mut burst = 0usize;
    let ladder: Vec<u64> = {
        let mut l = args.prices[..args.probes].to_vec();
        l.sort_unstable();
        l
    };
    let prep_slots = (3_000.0 / slot_ms).ceil() as u64;

    loop {
        if Instant::now() > deadline {
            println!("time limit reached");
            break;
        }
        // The soonest window of a target that still needs bursts, far enough ahead to prepare
        let now = clock.now_slot() as u64;
        let next = targets
            .iter()
            .enumerate()
            .filter(|(_, t)| t.done < args.bursts_per_target)
            .filter_map(|(i, t)| t.windows.iter().find(|w| w.start > now + prep_slots && w.start > last_end).map(|w| (i, *w)))
            .min_by_key(|(_, w)| w.start);
        let Some((ti, window)) = next else {
            if targets.iter().all(|t| t.done >= args.bursts_per_target) {
                println!("all bursts done");
            } else {
                println!("no more known windows for the remaining targets");
            }
            break;
        };
        let target = &targets[ti];
        let wait = clock.eta(window.start).saturating_duration_since(Instant::now());
        println!(
            "burst {burst}: {} {} window {}+{} in {:.1} s",
            target.client,
            target.identity,
            window.start,
            window.slots,
            wait.as_secs_f64()
        );
        tokio::time::sleep_until((clock.eta(window.start) - Duration::from_millis(2_500)).into()).await;

        // Prepare the burst
        let variant = args.variants[target.done % args.variants.len()];
        let spacing_ms = args.spacings_ms[(target.done / args.variants.len()) % args.spacings_ms.len()];
        let blockhash = chain::latest_blockhash(rpc).await?;
        let shared = Pubkey::new_from_array(rng.random());
        let mut specs = Vec::with_capacity(args.probes);
        for i in 0..args.probes {
            let cu_price = match variant {
                Variant::Null => ladder[args.probes / 2],
                _ => ladder[i],
            };
            let shared_target = variant == Variant::Conflict && i % 2 == 1;
            specs.push((
                ProbeSpec {
                    tag: format!("sop:{run_id}:{burst}:{i}"),
                    cu_limit: args.cu_limit,
                    cu_price,
                    target: if shared_target { shared } else { Pubkey::new_from_array(rng.random()) },
                },
                shared_target,
            ));
        }
        let mut order: Vec<usize> = (0..args.probes).collect();
        if variant == Variant::Reverse {
            // Already ascending by price: the most expensive probe goes last
        } else {
            order.shuffle(&mut rng);
        }
        let mut built = Vec::with_capacity(args.probes);
        let mut records = Vec::with_capacity(args.probes);
        for (i, (spec, shared_target)) in specs.iter().enumerate() {
            let payer = &pool.payers[i];
            let probe = probe::build(payer, spec, blockhash.clone())?;
            records.push(ProbeRecord {
                index: i,
                tag: spec.tag.clone(),
                signature: probe.signature.to_string(),
                payer: payer.pubkey().to_string(),
                cu_limit: spec.cu_limit,
                cu_price: spec.cu_price,
                max_fee_lamports: probe::max_fee_lamports(spec),
                shared_target: *shared_target,
                send_order: order.iter().position(|&o| o == i).unwrap(),
                sent_offset_us: None,
                send_error: None,
            });
            built.push(probe);
        }
        let worst_case: u64 = records.iter().map(|r| r.max_fee_lamports).sum();
        if args.live {
            if let Err(error) = ledger.reserve(worst_case) {
                println!("{error:#}; stopping");
                break;
            }
        }

        let (connection, handshake_ms, rtt_ms) = match tpu.connect(target.tpu_quic).await {
            Ok((connection, handshake)) => {
                let rtt = connection.rtt().as_secs_f64() * 1e3;
                (Some(connection), Some(handshake.as_secs_f64() * 1e3), Some(rtt))
            }
            Err(error) => {
                println!("  QUIC to {}: {error:#}; burst skipped", target.tpu_quic);
                if args.live {
                    ledger.release(worst_case);
                }
                targets[ti].windows.retain(|w| w.start != window.start);
                continue;
            }
        };

        // Fire
        let fire = clock.eta(window.start) - Duration::from_secs_f64(args.lead_ms / 1e3);
        tokio::time::sleep_until(fire.into()).await;
        let fired = Instant::now();
        let fire_at = Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true);
        let fire_slot_estimate = clock.now_slot();
        let fire_last_notified_slot = clock.latest().slot;
        for (n, &i) in order.iter().enumerate() {
            if n > 0 && spacing_ms > 0 {
                tokio::time::sleep(Duration::from_millis(spacing_ms)).await;
            }
            if args.live {
                if let Some(connection) = &connection {
                    if let Err(error) = quic::send(connection, &built[i].wire).await {
                        records[i].send_error = Some(format!("{error:#}"));
                    }
                }
            }
            records[i].sent_offset_us = Some(fired.elapsed().as_micros() as u64);
        }
        if let Some(connection) = &connection {
            // Let the streams drain before closing
            tokio::time::sleep(Duration::from_millis(300)).await;
            connection.close(0u32.into(), b"done");
        }

        let record = BurstRecord {
            run_id: run_id.clone(),
            burst,
            live: args.live,
            identity: target.identity.clone(),
            client: target.client.clone(),
            version: target.version.clone(),
            tpu_quic: target.tpu_quic.to_string(),
            stake_sol: target.stake_sol,
            window,
            variant,
            spacing_ms,
            lead_ms: args.lead_ms,
            blockhash: blockhash.to_string(),
            fire_at,
            fire_slot_estimate,
            fire_last_notified_slot,
            quic_handshake_ms: handshake_ms,
            quic_rtt_ms: rtt_ms,
            worst_case_lamports: worst_case,
            probes: records,
        };
        crate::store::append_jsonl(&run_dir.join("bursts.jsonl"), &record)?;
        println!(
            "  fired {:?} at slot ≈ {:.2} (window {}), handshake {:.0} ms",
            variant,
            fire_slot_estimate,
            window.start,
            handshake_ms.unwrap_or(f64::NAN)
        );

        if args.live {
            let collect_at = clock.eta(window.end() + 1) + Duration::from_secs(4);
            collectors.spawn(collect::burst(rpc.clone(), run_dir.clone(), record, ledger.clone(), collect_at));
        }
        targets[ti].done += 1;
        last_end = window.end();
        burst += 1;
    }

    while let Some(result) = collectors.join_next().await {
        if let Err(error) = result? {
            eprintln!("collection: {error:#}");
        }
    }
    println!("run {run_id}: {burst} bursts; spent {} of {} lamports", ledger.spent(), ledger.cap());
    Ok(())
}

pub fn run_dir(data_dir: &Path, run_id: &str) -> PathBuf {
    data_dir.join("runs").join(run_id)
}
