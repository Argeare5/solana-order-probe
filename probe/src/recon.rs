//! `recon`: a read-only snapshot of the cluster at the time of measurement. It records which client and version
//! every staked validator runs (`clientId` and `version` from gossip), its stake and TPU QUIC address, the slot time,
//! whether vote transactions still appear in blocks (they leave the block with Alpenglow), and the next leader
//! windows of the candidates, so that every later measurement can name the software of its leader.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};

use crate::rpc::Rpc;

/// Slot at which TowerBFT retires and Alpenglow (Votor) takes over, as announced by Anza for October 21, 2026.
const ALPENGLOW_SWITCH_SLOT: u64 = 459_653_000;
const VOTE_PROGRAM: &str = "Vote111111111111111111111111111111111111111";

#[derive(clap::Args)]
pub struct Args {
    /// How far ahead to look for leader windows, in minutes.
    #[arg(long, default_value_t = 120)]
    horizon_minutes: u64,

    /// Candidates listed per client, by stake.
    #[arg(long, default_value_t = 3)]
    per_client: usize,

    /// Recent blocks inspected for vote transactions and transaction versions.
    #[arg(long, default_value_t = 3)]
    block_sample: usize,
}

#[derive(Serialize)]
struct ClientSummary {
    client: String,
    validators: usize,
    stake_share: f64,
    versions: BTreeMap<String, usize>,
}

#[derive(Serialize)]
struct Candidate {
    client: String,
    identity: String,
    vote_account: String,
    stake_sol: f64,
    version: String,
    tpu_quic: Option<String>,
    /// First slots of the next leader windows inside the horizon.
    next_windows: Vec<u64>,
}

#[derive(Serialize)]
struct BlockSample {
    slot: u64,
    transactions: usize,
    vote_transactions: usize,
    versions: BTreeMap<String, usize>,
}

#[derive(Serialize)]
struct Summary {
    taken_at: String,
    rpc_host: String,
    epoch: u64,
    current_slot: u64,
    slot_ms: f64,
    leader_window_slots: Vec<usize>,
    alpenglow: Value,
    block_samples: Vec<BlockSample>,
    clients: Vec<ClientSummary>,
    candidates: Vec<Candidate>,
}

struct Node {
    client: String,
    version: String,
    tpu_quic: Option<String>,
}

pub async fn run(rpc: &Rpc, data_dir: &Path, args: Args) -> Result<()> {
    let taken_at = Utc::now();
    let epoch_info = rpc.call("getEpochInfo", json!([{ "commitment": "confirmed" }])).await?;
    let current_slot = epoch_info["absoluteSlot"].as_u64().context("absoluteSlot")?;
    let epoch = epoch_info["epoch"].as_u64().context("epoch")?;
    let epoch_start = current_slot - epoch_info["slotIndex"].as_u64().context("slotIndex")?;
    let epoch_end = epoch_start + epoch_info["slotsInEpoch"].as_u64().context("slotsInEpoch")?;

    // Slot time from the node's own performance samples (60 s each)
    let samples = rpc.call("getRecentPerformanceSamples", json!([10])).await?;
    let (slots, secs) = samples.as_array().context("performance samples")?.iter().fold((0u64, 0u64), |acc, s| {
        (acc.0 + s["numSlots"].as_u64().unwrap_or(0), acc.1 + s["samplePeriodSecs"].as_u64().unwrap_or(0))
    });
    let slot_ms = if slots > 0 { secs as f64 * 1000.0 / slots as f64 } else { f64::NAN };

    let cluster_nodes = rpc.call("getClusterNodes", json!([])).await?;
    let vote_accounts = rpc.call("getVoteAccounts", json!([{ "commitment": "confirmed" }])).await?;
    let schedule = rpc.call("getLeaderSchedule", json!([null, { "commitment": "confirmed" }])).await?;

    let nodes: HashMap<String, Node> = cluster_nodes
        .as_array()
        .context("cluster nodes")?
        .iter()
        .filter_map(|n| {
            Some((
                n["pubkey"].as_str()?.to_owned(),
                Node {
                    client: n["clientId"].as_str().unwrap_or("none").to_owned(),
                    version: n["version"].as_str().unwrap_or("none").to_owned(),
                    tpu_quic: n["tpuQuic"].as_str().map(str::to_owned),
                },
            ))
        })
        .collect();

    // Staked validators: identity → (vote account, stake in lamports)
    let mut stake: HashMap<String, (String, u64)> = HashMap::new();
    for v in vote_accounts["current"].as_array().context("current vote accounts")? {
        if let (Some(identity), Some(vote), Some(lamports)) =
            (v["nodePubkey"].as_str(), v["votePubkey"].as_str(), v["activatedStake"].as_u64())
        {
            stake.insert(identity.to_owned(), (vote.to_owned(), lamports));
        }
    }
    let total_stake: u64 = stake.values().map(|(_, s)| s).sum();

    // Leader windows inside the horizon (and inside this epoch, whose schedule is known)
    let horizon_slots = (args.horizon_minutes as f64 * 60_000.0 / slot_ms) as u64;
    let horizon_end = (current_slot + horizon_slots).min(epoch_end);
    let mut windows: HashMap<String, Vec<u64>> = HashMap::new();
    let mut runs: Vec<usize> = Vec::new();
    for (identity, indices) in schedule.as_object().context("leader schedule")? {
        let mut slots: Vec<u64> = indices.as_array().into_iter().flatten().filter_map(Value::as_u64).map(|i| epoch_start + i).collect();
        slots.sort_unstable();
        let mut window_start: Option<u64> = None;
        let mut previous = 0;
        let mut length = 0;
        for slot in slots {
            if window_start.is_some() && slot == previous + 1 {
                length += 1;
            } else {
                if let Some(start) = window_start
                    && (current_slot..horizon_end).contains(&start)
                {
                    windows.entry(identity.clone()).or_default().push(start);
                    runs.push(length);
                }
                window_start = Some(slot);
                length = 1;
            }
            previous = slot;
        }
        if let Some(start) = window_start
            && (current_slot..horizon_end).contains(&start)
        {
            windows.entry(identity.clone()).or_default().push(start);
            runs.push(length);
        }
    }
    let mut window_lengths: BTreeMap<usize, usize> = BTreeMap::new();
    for length in &runs {
        *window_lengths.entry(*length).or_default() += 1;
    }

    // Vote transactions and transaction versions in recent blocks
    let mut block_samples = Vec::new();
    let mut slot = current_slot.saturating_sub(40);
    while block_samples.len() < args.block_sample && slot > current_slot.saturating_sub(400) {
        let params = json!([slot, {
            "encoding": "json",
            "transactionDetails": "accounts",
            "maxSupportedTransactionVersion": 1,
            "rewards": false,
            "commitment": "confirmed"
        }]);
        match rpc.call("getBlock", params).await {
            Ok(block) => {
                let txs = block["transactions"].as_array().cloned().unwrap_or_default();
                let mut versions: BTreeMap<String, usize> = BTreeMap::new();
                let mut votes = 0;
                for tx in &txs {
                    *versions.entry(tx["version"].to_string().trim_matches('"').to_owned()).or_default() += 1;
                    let is_vote = tx["transaction"]["accountKeys"]
                        .as_array()
                        .is_some_and(|keys| keys.iter().any(|k| k["pubkey"].as_str() == Some(VOTE_PROGRAM)));
                    votes += usize::from(is_vote);
                }
                block_samples.push(BlockSample { slot, transactions: txs.len(), vote_transactions: votes, versions });
            }
            // Skipped slots have no block
            Err(error) => eprintln!("slot {slot}: {error}"),
        }
        slot -= 7;
    }
    let vote_txs: usize = block_samples.iter().map(|b| b.vote_transactions).sum();

    // Clients by stake
    let mut by_client: BTreeMap<String, (usize, u64, BTreeMap<String, usize>)> = BTreeMap::new();
    for (identity, (_, lamports)) in &stake {
        let (client, version) = nodes
            .get(identity)
            .map(|n| (n.client.clone(), n.version.clone()))
            .unwrap_or(("not in gossip".to_owned(), "none".to_owned()));
        let entry = by_client.entry(client).or_default();
        entry.0 += 1;
        entry.1 += lamports;
        *entry.2.entry(version).or_default() += 1;
    }
    let mut clients: Vec<ClientSummary> = by_client
        .into_iter()
        .map(|(client, (validators, lamports, versions))| ClientSummary {
            client,
            validators,
            stake_share: lamports as f64 / total_stake as f64,
            versions,
        })
        .collect();
    clients.sort_by(|a, b| b.stake_share.total_cmp(&a.stake_share));

    // Candidates: per client, the largest validators that lead inside the horizon and publish a TPU QUIC address
    let mut candidates = Vec::new();
    for summary in &clients {
        let mut pool: Vec<(&String, &(String, u64))> = stake
            .iter()
            .filter(|(identity, _)| {
                nodes.get(*identity).is_some_and(|n| n.client == summary.client && n.tpu_quic.is_some())
                    && windows.contains_key(*identity)
            })
            .collect();
        pool.sort_by_key(|p| std::cmp::Reverse(p.1.1));
        for (identity, (vote, lamports)) in pool.into_iter().take(args.per_client) {
            let node = &nodes[identity];
            let mut next = windows[identity].clone();
            next.sort_unstable();
            next.truncate(5);
            candidates.push(Candidate {
                client: summary.client.clone(),
                identity: identity.clone(),
                vote_account: vote.clone(),
                stake_sol: *lamports as f64 / 1e9,
                version: node.version.clone(),
                tpu_quic: node.tpu_quic.clone(),
                next_windows: next,
            });
        }
    }

    let summary = Summary {
        taken_at: taken_at.to_rfc3339(),
        rpc_host: rpc_host(&std::env::var("RPC_URL").unwrap_or_default()),
        epoch,
        current_slot,
        slot_ms,
        leader_window_slots: window_lengths.keys().copied().collect(),
        alpenglow: json!({
            "announced_switch_slot": ALPENGLOW_SWITCH_SLOT,
            "current_slot_after_switch": current_slot > ALPENGLOW_SWITCH_SLOT,
            "vote_transactions_in_sampled_blocks": vote_txs,
            "window_length_counts": window_lengths,
        }),
        block_samples,
        clients,
        candidates,
    };

    let dir = data_dir.join("snapshots").join(taken_at.format("%Y%m%dT%H%M%SZ").to_string());
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    // Raw responses compressed; the sampled blocks are not kept (their slots are in the summary and getBlock
    // returns them again)
    crate::store::write_json_gz(&dir.join("epoch_info.json.gz"), &epoch_info)?;
    crate::store::write_json_gz(&dir.join("performance_samples.json.gz"), &samples)?;
    crate::store::write_json_gz(&dir.join("cluster_nodes.json.gz"), &cluster_nodes)?;
    crate::store::write_json_gz(&dir.join("vote_accounts.json.gz"), &vote_accounts)?;
    crate::store::write_json(&dir.join("summary.json"), &serde_json::to_value(&summary)?)?;

    print_summary(&summary);
    println!("\nSnapshot written to {}", dir.display());
    Ok(())
}

/// Keeps only the host of the RPC URL, so that a provider key in the path or query never lands in the data.
pub fn rpc_host(url: &str) -> String {
    url.split("://").nth(1).unwrap_or(url).split(['/', '?']).next().unwrap_or("").to_owned()
}

fn print_summary(s: &Summary) {
    println!("epoch {}  slot {}  slot time {:.0} ms  window lengths {:?}", s.epoch, s.current_slot, s.slot_ms, s.leader_window_slots);
    println!(
        "Alpenglow: switch slot {}  passed: {}  vote txs in {} sampled blocks: {}",
        ALPENGLOW_SWITCH_SLOT,
        s.current_slot > ALPENGLOW_SWITCH_SLOT,
        s.block_samples.len(),
        s.alpenglow["vote_transactions_in_sampled_blocks"]
    );
    println!("\n{:<22} {:>10} {:>8}  versions", "client", "validators", "stake");
    for c in &s.clients {
        println!("{:<22} {:>10} {:>7.1}%  {:?}", c.client, c.validators, c.stake_share * 100.0, c.versions);
    }
    println!("\n{:<20} {:<45} {:>12} {:<12} next windows", "client", "identity", "stake SOL", "version");
    for c in &s.candidates {
        println!("{:<20} {:<45} {:>12.0} {:<12} {:?}", c.client, c.identity, c.stake_sol, c.version, c.next_windows);
    }
}
