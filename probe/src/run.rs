//! `run`: bursts of probes into the leader windows of chosen validators.
//!
//! For each burst the tool waits for the next window of one of the targets, builds K probes with a fresh blockhash
//! (one payer each, same shape, prices and send order set by the burst's variant), opens a QUIC connection to the
//! leader's TPU and, at the burst's offset from the predicted start of the window (the slot clock is re-predicted
//! every few ms until then), sends the probes one stream each in the planned order, `spacing_ms` apart. The first
//! evidence of the window that the RPC node then reports is recorded with the burst, so the analysis knows when the
//! burst really went out relative to the window. The burst is written to `bursts.jsonl` before collection starts.
//! Without `--live` nothing is sent: the burst is planned, built and timed, and the connection is opened, so the
//! dry-run shows whether the timing hits the window.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use chrono::{SecondsFormat, Utc};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{RngExt, SeedableRng};
use serde::{Deserialize, Serialize};
use serde_json::json;
use solana_pubkey::Pubkey;
use solana_signer::Signer;

use crate::clock::{self, SlotClock};
use crate::keys::PayerPool;
use crate::ledger::Ledger;
use crate::probe::{self, ProbeSpec};
use crate::quic::{self, TpuClient};
use crate::rpc::Rpc;
use crate::schedule::{self, Window};
use crate::{chain, collect, reconcile};

/// How the probes reach the leader.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SendPath {
    /// Straight to the leader's TPU over QUIC: we control the arrival order.
    #[default]
    Quic,
    /// Through the RPC node's `sendTransaction` (no preflight, no retries), which forwards them itself: for comparison.
    Rpc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum, Serialize, Deserialize)]
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

    /// Compute unit limit of every probe (a probe consumes ~810 CU on mainnet: see `simulate`).
    #[arg(long, default_value_t = 900)]
    cu_limit: u32,

    /// Price ladder in micro-lamports per compute unit, one price per probe.
    #[arg(
        long,
        value_delimiter = ',',
        default_value = "1000,2000,5000,10000,20000,50000,100000,200000,500000,1000000,2000000,5000000"
    )]
    prices: Vec<u64>,

    /// When the first probe goes out, relative to the predicted first evidence of the window's first slot at the
    /// RPC node, in ms (negative = before); used in turn. Probes that reach an Agave leader before its window are
    /// held for it, so a negative offset gives the scheduler the whole burst at once and a positive one streams it in.
    #[arg(long, value_delimiter = ',', allow_hyphen_values = true, default_value = "-300,100")]
    offsets_ms: Vec<f64>,

    /// Gap between consecutive sends of a burst, in ms; used in turn.
    #[arg(long, value_delimiter = ',', default_value = "0,10,50")]
    spacings_ms: Vec<u64>,

    /// How the probes are sent: straight to the leader over QUIC, or through the RPC node for comparison.
    #[arg(long, value_enum, default_value = "quic")]
    path: SendPath,

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

#[derive(Serialize, Deserialize, Clone)]
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
    /// Whether the TPU acknowledged all of the probe's bytes over QUIC (`acked`), or what happened instead.
    #[serde(default)]
    pub tpu_ack: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
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
    #[serde(default)]
    pub path: SendPath,
    pub variant: Variant,
    pub spacing_ms: u64,
    /// Planned fire moment relative to the predicted first evidence of the window start, in ms.
    pub offset_ms: f64,
    pub blockhash: String,
    pub fire_at: String,
    pub fire_unix_us: i64,
    /// Slot by the clock at the fire moment (fractional), the last slot seen, and the fitted slot time.
    pub fire_slot_estimate: f64,
    pub fire_last_seen_slot: u64,
    pub clock_slot_ms: f64,
    /// What the clock predicted from at the fire moment, and the node's clock minus ours (node basis only).
    pub clock_basis: clock::Basis,
    pub clock_node_offset_ms: Option<f64>,
    /// The window start's first evidence as predicted at the fire moment (local Unix µs).
    pub predicted_start_unix_us: i64,
    /// As it was then observed: the local arrival of its first notification, and the node's first-shred timestamp.
    pub observed_arrival_unix_us: Option<i64>,
    pub observed_first_shred_node_ms: Option<u64>,
    /// Fire moment minus the observed window start on the clock's basis, in ms (negative = fired before it).
    pub fire_vs_observed_ms: Option<f64>,
    pub quic_handshake_ms: Option<f64>,
    pub quic_rtt_ms: Option<f64>,
    /// Why the TPU closed the connection each time it had to be reopened during the burst.
    #[serde(default)]
    pub quic_drops: Vec<String>,
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

/// The most a burst of `args` can cost: every probe at its ladder price plus the base fee.
fn probe_burst_worst_case(args: &Args) -> u64 {
    args.prices[..args.probes]
        .iter()
        .map(|&cu_price| {
            probe::max_fee_lamports(&ProbeSpec { tag: String::new(), cu_limit: args.cu_limit, cu_price, target: Pubkey::default() })
        })
        .sum()
}

/// How long before the fire moment the QUIC connection is opened (the handshake takes 30–160 ms on mainnet).
const CONNECT_LEAD_MS: f64 = 700.0;

/// Sleeps until `offset_ms` from the predicted first evidence of `slot`, re-predicting every few ms.
async fn wait_until(clock: &SlotClock, slot: u64, offset_ms: f64) {
    loop {
        let at = clock::shift(clock.predict(slot), offset_ms);
        let now = Instant::now();
        if now >= at {
            return;
        }
        tokio::time::sleep((at - now).min(Duration::from_millis(5))).await;
    }
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
        let node = nodes.as_array().into_iter().flatten().find(|n| n["pubkey"].as_str() == Some(identity));
        let Some(tpu_quic) = node.and_then(|n| n["tpuQuic"].as_str()).and_then(|a| a.parse::<SocketAddr>().ok()) else {
            println!("{identity} is not in gossip or has no TPU QUIC address: skipped");
            continue;
        };
        let node = node.unwrap();
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
    if targets.is_empty() {
        bail!("none of the targets can be measured");
    }

    let slot_ms = schedule::slot_ms(rpc).await?;
    let (genesis_hash, cluster) = chain::cluster(rpc).await?;

    let pool = PayerPool::load_or_create(secrets, args.probes)?;
    if args.live {
        // Every payer pays one probe of every burst; it must be able to pay the most expensive one in each and
        // stay rent-exempt, or the last bursts of the run would be lost
        let rent = chain::rent_exempt_minimum(rpc, 0).await?;
        let top_price = args.prices[..args.probes].iter().copied().max().unwrap_or(0);
        let per_probe = probe::max_fee_lamports(&ProbeSpec {
            tag: String::new(),
            cu_limit: args.cu_limit,
            cu_price: top_price,
            target: Pubkey::default(),
        });
        let bursts = (targets.len() * args.bursts_per_target) as u64;
        let needed = rent + bursts * per_probe;
        for payer in &pool.payers {
            let balance = chain::balance(rpc, &payer.pubkey().to_string()).await?;
            if balance < needed {
                bail!(
                    "payer {} holds {balance} lamports; {bursts} bursts can cost it up to {} plus {rent} rent-exempt: \
                     top the payers up with `fund --live --float-sol {:.4}`",
                    payer.pubkey(),
                    bursts * per_probe,
                    (bursts * per_probe) as f64 * 1.1 / 1e9
                );
            }
        }
    }

    let clock = SlotClock::start(args.ws_url.clone(), slot_ms, None).await?;
    let basis = clock.settle(Duration::from_secs(20)).await;
    println!("slot clock on {basis:?}, slot time {:.1} ms", clock.slot_ms());
    if args.live {
        // Fees that earlier runs paid but did not record count against the cap before anything is sent
        reconcile::all(rpc, data_dir, ledger).await?;
    }

    crate::store::write_json(
        &run_dir.join("run.json"),
        &json!({
            "run_id": run_id,
            "started_at": Utc::now().to_rfc3339(),
            "code_version": format!("{} {}", env!("CARGO_PKG_VERSION"), env!("ORDER_PROBE_COMMIT")),
            "cluster": cluster,
            "genesis_hash": genesis_hash,
            "live": args.live,
            "path": args.path,
            "seed": seed,
            "epoch": bounds.epoch,
            "slot_ms": slot_ms,
            "probes": args.probes,
            "cu_limit": args.cu_limit,
            "prices": args.prices,
            "offsets_ms": args.offsets_ms,
            "spacings_ms": args.spacings_ms,
            "clock_basis": basis,
            "clock_anchors": clock::ANCHORS,
            "clock_arrival_estimator": clock::ESTIMATOR,
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
    println!(
        "run {run_id}  {cluster}  {}  seed {seed}  slot {slot_ms:.0} ms  cap {} lamports, spent {}",
        if args.live { "LIVE" } else { "dry-run" },
        ledger.cap(),
        ledger.spent()
    );
    if args.live {
        // The plan, and a moment to stop it, before anything is sent
        let worst: u64 = targets.len() as u64 * args.bursts_per_target as u64 * probe_burst_worst_case(&args);
        println!(
            "LIVE on {cluster}: {} targets × {} bursts, worst case {worst} lamports, {} left under the cap; \
             sending starts in 10 s (Ctrl-C to stop)",
            targets.len(),
            args.bursts_per_target,
            ledger.cap().saturating_sub(ledger.spent())
        );
        tokio::time::sleep(Duration::from_secs(10)).await;
    }

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
    let mut last_fire: Option<Instant> = None;

    // A failure inside the loop ends it, but never before the bursts already sent are collected and reconciled
    let outcome: Result<()> = async {
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
            let wait = clock.predict(window.start).saturating_duration_since(Instant::now());
            println!(
                "burst {burst}: {} {} window {}+{} in {:.1} s",
                target.client,
                target.identity,
                window.start,
                window.slots,
                wait.as_secs_f64()
            );
            tokio::time::sleep_until((clock.predict(window.start) - Duration::from_millis(2_500)).into()).await;

            // Prepare the burst. Conditions cycle per target: variant fastest, then offset, then spacing
            let (n_variants, n_offsets) = (args.variants.len(), args.offsets_ms.len());
            let variant = args.variants[target.done % n_variants];
            let offset_ms = args.offsets_ms[(target.done / n_variants) % n_offsets];
            let spacing_ms = args.spacings_ms[(target.done / (n_variants * n_offsets)) % args.spacings_ms.len()];
            let blockhash = match chain::latest_blockhash(rpc).await {
                Ok(blockhash) => blockhash,
                Err(error) => {
                    println!("  blockhash: {error:#}; window skipped");
                    continue;
                }
            };
            let shared = Pubkey::new_from_array(rng.random());
            // Prices go to the payers in a fresh random order every burst, so that price is never tied to a payer
            // (a leader that orders by account would otherwise look like one that orders by price)
            let mut price_rank: Vec<usize> = (0..args.probes).collect();
            price_rank.shuffle(&mut rng);
            let mut specs = Vec::with_capacity(args.probes);
            for i in 0..args.probes {
                let cu_price = match variant {
                    Variant::Null => ladder[args.probes / 2],
                    _ => ladder[price_rank[i]],
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
                // Cheapest first: the most expensive probe goes last
                order.sort_by_key(|&i| price_rank[i]);
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
                    tpu_ack: None,
                });
                built.push(probe);
            }
            let worst_case: u64 = records.iter().map(|r| r.max_fee_lamports).sum();
            if args.live && let Err(error) = ledger.reserve(worst_case) {
                println!("{error:#}; stopping");
                break;
            }

            // Connect shortly before the fire moment (an idle unstaked connection is what a busy TPU evicts first),
            // then wait for the moment itself; the window start is re-predicted every few ms on the way
            wait_until(&clock, window.start, offset_ms - CONNECT_LEAD_MS).await;
            let mut link = match args.path {
                SendPath::Quic => match quic::Link::open(&tpu, target.tpu_quic).await {
                    Ok(link) => Some(link),
                    Err(error) => {
                        println!("  QUIC to {}: {error:#}; burst skipped", target.tpu_quic);
                        if args.live {
                            ledger.release(worst_case);
                        }
                        targets[ti].windows.retain(|w| w.start != window.start);
                        continue;
                    }
                },
                SendPath::Rpc => None,
            };
            let handshake_ms = link.as_ref().map(|l| l.handshake.as_secs_f64() * 1e3);
            wait_until(&clock, window.start, offset_ms).await;
            if let Some(link) = link.as_mut()
                && let Err(error) = link.ensure_open().await
            {
                println!("  QUIC to {}: {error:#}", target.tpu_quic);
            }
            let rtt_ms = link.as_ref().map(|l| l.connection.rtt().as_secs_f64() * 1e3);
            let fired = Instant::now();
            let fire_unix_us = clock::unix_us();
            let fire_at = Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true);
            let fire_slot_estimate = clock.now_slot();
            let fire_last_seen_slot = clock.latest_slot();
            let clock_slot_ms = clock.slot_ms();
            let clock_basis = clock.basis();
            let clock_node_offset_ms = clock.node_offset_ms();
            let predicted_start_unix_us = (clock.predict_unix_ms(window.start) * 1e3) as i64;
            let mut streams = Vec::new();
            for (n, &i) in order.iter().enumerate() {
                if n > 0 && spacing_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(spacing_ms)).await;
                }
                if args.live {
                    let sent = match (link.as_mut(), args.path) {
                        (Some(link), SendPath::Quic) => link.send(&built[i].wire).await.map(|s| streams.push((i, s))),
                        (_, SendPath::Rpc) => chain::send_unchecked(rpc, &built[i].wire).await.map(|_| ()),
                        (None, SendPath::Quic) => Ok(()),
                    };
                    if let Err(error) = sent {
                        records[i].send_error = Some(format!("{error:#}"));
                    }
                }
                records[i].sent_offset_us = Some(fired.elapsed().as_micros() as u64);
            }
            last_fire = Some(fired);
            if let Some(link) = &link {
                // Wait until the TPU acknowledges every probe's bytes (or 2 s), then close
                let until = tokio::time::Instant::now() + Duration::from_secs(2);
                let acks = streams.into_iter().map(|(i, stream)| async move {
                    let ack = match tokio::time::timeout_at(until, stream.stopped()).await {
                        Ok(Ok(None)) => "acked".to_owned(),
                        Ok(Ok(Some(code))) => format!("stopped by the TPU with code {code}"),
                        Ok(Err(error)) => format!("{error}"),
                        Err(_) => "no acknowledgement within 2 s".to_owned(),
                    };
                    (i, ack)
                });
                for (i, ack) in futures_util::future::join_all(acks).await {
                    records[i].tpu_ack = Some(ack);
                }
                link.connection.close(0u32.into(), b"done");
            }
            let quic_drops = link.map(|l| l.drops).unwrap_or_default();
            let observed = clock.wait_observed(window.start, clock.predict(window.start) + Duration::from_secs(3)).await;

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
                path: args.path,
                variant,
                spacing_ms,
                offset_ms,
                blockhash: blockhash.to_string(),
                fire_at,
                fire_unix_us,
                fire_slot_estimate,
                fire_last_seen_slot,
                clock_slot_ms,
                clock_basis,
                clock_node_offset_ms,
                predicted_start_unix_us,
                observed_arrival_unix_us: observed.map(|o| o.unix_us),
                observed_first_shred_node_ms: observed.and_then(|o| o.first_shred_node_ms),
                fire_vs_observed_ms: clock.observed_unix_ms(window.start).map(|ms| fire_unix_us as f64 / 1e3 - ms),
                quic_handshake_ms: handshake_ms,
                quic_rtt_ms: rtt_ms,
                quic_drops,
                worst_case_lamports: worst_case,
                probes: records,
            };
            crate::store::append_jsonl(&run_dir.join("bursts.jsonl"), &record)?;
            println!(
                "  fired {:?} (offset {offset_ms:+.0} ms, spacing {spacing_ms} ms): {}, {}",
                variant,
                record
                    .fire_vs_observed_ms
                    .map_or("the window's first slot was not seen (skipped?)".to_owned(), |ms| format!("{ms:+.0} ms vs the window's first shred")),
                handshake_ms.map_or("through the RPC node".to_owned(), |ms| format!("QUIC handshake {ms:.0} ms"))
            );

            if args.live {
                let collect_at = clock.predict(window.end() + 1) + Duration::from_secs(4);
                collectors.spawn(collect::burst(rpc.clone(), run_dir.clone(), record, ledger.clone(), collect_at));
            }
            targets[ti].done += 1;
            last_end = window.end();
            burst += 1;
        }
        Ok(())
    }
    .await;
    if let Err(error) = &outcome {
        println!("run stopped: {error:#}; collecting the bursts already sent");
    }

    while let Some(result) = collectors.join_next().await {
        match result {
            Ok(Err(error)) => eprintln!("collection: {error:#} (the reconciliation retries it)"),
            Err(error) => eprintln!("collection task: {error}"),
            Ok(Ok(())) => {}
        }
    }
    if let (true, Some(fired)) = (args.live, last_fire) {
        let wait = (fired + reconcile::SETTLE).saturating_duration_since(Instant::now());
        println!("waiting {:.0} s until the last probes can no longer land, then reconciling", wait.as_secs_f64());
        tokio::time::sleep(wait).await;
        reconcile::run_dir(rpc, &run_dir, ledger).await?;
    }
    outcome?;
    println!("run {run_id}: {burst} bursts; spent {} of {} lamports", ledger.spent(), ledger.cap());
    Ok(())
}
