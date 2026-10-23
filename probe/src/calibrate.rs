//! `calibrate`: records the RPC node's slot notifications for a while and measures how well the slot clock predicts
//! the start of a leader window ahead. For every window start seen, each method predicts it from what had arrived
//! at this machine `horizon` ms before, as `run` would at its fire moment; the error is prediction minus observation.
//! Node-timestamp methods are scored against the node's first shred of the window, arrival methods against the
//! local arrival of its first notification (which itself carries the WebSocket's batching).
//! It also reports the slot time by position in the leader window, the share of skipped slots, the delay from the
//! node's first shred to this machine and, with the leader of every recorded slot (`getSlotLeaders`, saved as
//! `leaders.json.gz`), the hand-over between leaders and the length of their windows by client. Read-only: nothing
//! is sent.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::clock::{self, Estimator, RawEvent, SlotClock, quantile};
use crate::rpc::Rpc;
use crate::schedule;

#[derive(clap::Args)]
pub struct Args {
    /// WebSocket endpoint whose slot notifications are recorded.
    #[arg(long, env = "WS_URL", default_value = "wss://api.mainnet-beta.solana.com")]
    ws_url: String,

    /// How long to record, in minutes.
    #[arg(long, default_value_t = 5.0)]
    minutes: f64,

    /// Prediction horizons in ms: how long before the window's first evidence the prediction is made.
    #[arg(long, value_delimiter = ',', default_value = "20,100,300,600,1000,2500")]
    horizons_ms: Vec<f64>,

    /// Re-evaluate a recorded calibration (its directory) instead of recording a new one.
    #[arg(long)]
    from: Option<std::path::PathBuf>,
}

/// Which notifications count as the evidence the estimator sees.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum Series {
    /// `slotSubscribe` only (what the clock used before).
    SlotNotify,
    /// The earliest notification of any kind.
    Any,
}

#[derive(Clone, Copy, Serialize)]
#[serde(tag = "method", rename_all = "snake_case")]
enum Method {
    /// Local arrival times projected at the fitted slot time.
    Arrival { series: Series, estimator: Estimator },
    /// The node's first-shred timestamps moved forward by the per-position slot durations (what `run` uses).
    NodePhased { anchors: usize },
    /// The node's first-shred timestamps projected at the fitted slot time.
    NodeLinear { estimator: Estimator },
}

impl Method {
    fn label(&self) -> String {
        match self {
            Method::Arrival { series, estimator } => format!(
                "arrival {} fit {} recent {} q {}",
                match series {
                    Series::SlotNotify => "slot",
                    Series::Any => "any",
                },
                estimator.fit_slots,
                estimator.recent,
                estimator.quantile
            ),
            Method::NodePhased { anchors } => format!("node phased, {anchors} anchors"),
            Method::NodeLinear { estimator } => {
                format!("node linear fit {} recent {} q {}", estimator.fit_slots, estimator.recent, estimator.quantile)
            }
        }
    }
}

/// Who led each recorded slot, and the client each of those leaders ran by gossip when the list was fetched.
#[derive(Serialize, Deserialize)]
struct Leaders {
    first_slot: u64,
    leaders: Vec<String>,
    clients: BTreeMap<String, String>,
    fetched_at: String,
}

impl Leaders {
    fn client(&self, slot: u64) -> Option<&str> {
        let leader = self.leaders.get(usize::try_from(slot.checked_sub(self.first_slot)?).ok()?)?;
        Some(self.clients.get(leader).map_or("not in gossip", String::as_str))
    }
}

async fn fetch_leaders(rpc: &Rpc, first: u64, last: u64) -> Result<Leaders> {
    let mut leaders = Vec::new();
    let mut slot = first;
    while slot <= last {
        let limit = (last - slot + 1).min(5_000);
        let chunk = rpc.call("getSlotLeaders", json!([slot, limit])).await?;
        leaders.extend(chunk.as_array().context("slot leaders")?.iter().filter_map(|v| v.as_str().map(str::to_owned)));
        slot += limit;
    }
    let wanted: HashSet<&String> = leaders.iter().collect();
    let nodes = rpc.call("getClusterNodes", json!([])).await?;
    let clients = nodes
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| Some((n["pubkey"].as_str()?.to_owned(), n["clientId"].as_str().unwrap_or("none").to_owned())))
        .filter(|(identity, _)| wanted.contains(identity))
        .collect();
    Ok(Leaders { first_slot: first, leaders, clients, fetched_at: Utc::now().to_rfc3339() })
}

/// The slots a recording covers (first evidence of a start, not roots or confirmations).
fn slot_range(events: &[RawEvent]) -> Option<(u64, u64)> {
    let slots = events
        .iter()
        .filter(|e| !matches!(e.kind.as_str(), "root" | "optimisticConfirmation" | "dead" | "connected"))
        .map(|e| e.slot);
    Some((slots.clone().min()?, slots.max()?))
}

#[derive(Serialize)]
struct ErrorStats {
    method: Method,
    label: String,
    horizon_ms: f64,
    n: usize,
    median_ms: f64,
    p10_ms: f64,
    p90_ms: f64,
    mean_abs_ms: f64,
}

pub async fn run(rpc: &Rpc, data_dir: &Path, args: Args) -> Result<()> {
    if let Some(dir) = &args.from {
        let events: Vec<RawEvent> =
            serde_json::from_value(crate::store::read_json_gz(&dir.join("events.json.gz"))?).context("events.json.gz")?;
        let previous: Value = serde_json::from_slice(&std::fs::read(dir.join("summary.json"))?)?;
        let hint_ms = previous["summary"]["slot_time_hint_ms"].as_f64().context("slot_time_hint_ms in summary.json")?;
        let leaders_path = dir.join("leaders.json.gz");
        let leaders: Option<Leaders> = if leaders_path.exists() {
            Some(serde_json::from_value(crate::store::read_json_gz(&leaders_path)?)?)
        } else if let Some((first, last)) = slot_range(&events) {
            // Recorded before leaders were saved: the schedule is the same, the clients are today's gossip
            match fetch_leaders(rpc, first, last).await {
                Ok(leaders) => {
                    crate::store::write_json_gz(&leaders_path, &serde_json::to_value(&leaders)?)?;
                    Some(leaders)
                }
                Err(error) => {
                    eprintln!("leaders of the recorded slots: {error:#}");
                    None
                }
            }
        } else {
            None
        };
        let summary =
            evaluate(&events, hint_ms, &args.horizons_ms, previous["summary"]["slots_updates"] == true, leaders.as_ref());
        let mut written = previous.clone();
        written["summary"] = summary.clone();
        crate::store::write_json(&dir.join("summary.json"), &written)?;
        print(&summary);
        return Ok(());
    }
    let hint_ms = schedule::slot_ms(rpc).await?;
    let id = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let dir = data_dir.join("calibration").join(&id);
    std::fs::create_dir_all(&dir)?;
    println!("calibration {id}: recording slot notifications for {} min (slot time {hint_ms:.1} ms by the samples)", args.minutes);

    let (tx, mut rx) = mpsc::unbounded_channel::<RawEvent>();
    let clock = SlotClock::start(args.ws_url.clone(), hint_ms, Some(tx)).await?;
    let until = Instant::now() + Duration::from_secs_f64(args.minutes * 60.0);
    let mut events = Vec::new();
    let mut next_report = Instant::now() + Duration::from_secs(30);
    // Ctrl-C or SIGTERM ends the recording early; what was recorded is still evaluated and written
    let stop = stop_signal();
    tokio::pin!(stop);
    while Instant::now() < until {
        tokio::select! {
            received = tokio::time::timeout(Duration::from_millis(200), rx.recv()) => match received {
                Ok(Some(event)) => events.push(event),
                Ok(None) => break,
                Err(_) => {}
            },
            _ = &mut stop => {
                println!("  stopped after {} events", events.len());
                break;
            }
        }
        if Instant::now() > next_report {
            println!("  {} events, slot {}, slot time {:.1} ms", events.len(), clock.latest_slot(), clock.slot_ms());
            next_report += Duration::from_secs(30);
        }
    }
    let slots_updates = clock.has_slots_updates();

    let mut lines = Vec::with_capacity(events.len());
    for event in &events {
        lines.push(serde_json::to_value(event)?);
    }
    crate::store::write_json_gz(&dir.join("events.json.gz"), &Value::Array(lines))?;
    let leaders = match slot_range(&events) {
        Some((first, last)) => Some(fetch_leaders(rpc, first, last).await?),
        None => None,
    };
    if let Some(leaders) = &leaders {
        crate::store::write_json_gz(&dir.join("leaders.json.gz"), &serde_json::to_value(leaders)?)?;
    }

    let summary = evaluate(&events, hint_ms, &args.horizons_ms, slots_updates, leaders.as_ref());
    crate::store::write_json(
        &dir.join("summary.json"),
        &json!({ "id": id, "ws_host": crate::recon::rpc_host(&args.ws_url), "minutes": args.minutes, "summary": summary }),
    )?;
    print(&summary);
    println!("\nCalibration written to {}", dir.display());
    Ok(())
}

/// Completes on Ctrl-C or, on Unix, SIGTERM.
async fn stop_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

fn evaluate(events: &[RawEvent], hint_ms: f64, horizons: &[f64], slots_updates: bool, leaders: Option<&Leaders>) -> Value {
    let Some(t0) = events.first().map(|e| e.unix_us) else { return json!({ "events": 0 }) };
    let ms = |unix_us: i64| (unix_us - t0) as f64 / 1e3;

    // Every (re)connection may reach another node behind the endpoint, with another clock: events are split into
    // connections, and each connection's node timestamps map to local time by its own delivery envelope
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    let mut any: BTreeMap<u64, f64> = BTreeMap::new();
    let mut notify: BTreeMap<u64, f64> = BTreeMap::new();
    let mut first_shred: BTreeMap<u64, (f64, u64, usize)> = BTreeMap::new();
    let mut connection = 0usize;
    for event in events {
        *kinds.entry(event.kind.as_str()).or_default() += 1;
        if event.kind == "connected" {
            connection += 1;
            continue;
        }
        if matches!(event.kind.as_str(), "root" | "optimisticConfirmation" | "dead") {
            continue;
        }
        let at = ms(event.unix_us);
        any.entry(event.slot).or_insert(at);
        if event.kind == "slot" {
            notify.entry(event.slot).or_insert(at);
        }
        if let ("firstShredReceived", Some(node_ms)) = (event.kind.as_str(), event.node_ms) {
            first_shred.entry(event.slot).or_insert((at, node_ms, connection));
        }
    }

    // Delay from the node's first shred to this machine (includes the offset between the two clocks)
    let delivery_of = |&(at, node_ms, _): &(f64, u64, usize)| (t0 as f64 / 1e3 + at) - node_ms as f64;
    let delivery: Vec<f64> = first_shred.values().map(delivery_of).collect();
    let mut by_connection: BTreeMap<usize, Vec<f64>> = BTreeMap::new();
    for value in first_shred.values() {
        by_connection.entry(value.2).or_default().push(delivery_of(value));
    }
    let low_delivery = quantile(&delivery, 0.02);
    let envelope: BTreeMap<usize, f64> = by_connection
        .iter()
        .map(|(c, d)| (*c, if d.len() >= 30 { quantile(d, 0.02) } else { low_delivery }))
        .collect();
    // The node's first shred of each slot on the local timeline (ms since the first event)
    let shred_local: BTreeMap<u64, f64> = first_shred
        .iter()
        .map(|(slot, &(_, node_ms, c))| (*slot, node_ms as f64 + envelope[&c] - t0 as f64 / 1e3))
        .collect();
    let shred_to_notify: Vec<f64> =
        first_shred.iter().filter_map(|(slot, (at, _, _))| notify.get(slot).map(|n| n - at)).collect();

    // Slot time by position in the leader window, from the node's timestamps when there are any
    let starts: BTreeMap<u64, f64> = if first_shred.len() > any.len() / 2 { shred_local.clone() } else { any.clone() };
    let mut by_position: BTreeMap<u64, Vec<f64>> = BTreeMap::new();
    for (slot, at) in &starts {
        if let Some(previous) = starts.get(&(slot - 1)) {
            by_position.entry(slot % 4).or_default().push(at - previous);
        }
    }
    let (lo, hi) = (*any.keys().next().unwrap(), *any.keys().next_back().unwrap());
    let skipped = (lo..=hi).filter(|s| !any.contains_key(s)).count();

    // Prediction error at window starts, after a warm-up long enough for the fit
    let series_points = |map: &BTreeMap<u64, f64>| {
        let mut points: Vec<(u64, f64)> = map.iter().map(|(s, t)| (*s, *t)).collect();
        points.sort_by(|a, b| a.1.total_cmp(&b.1));
        points
    };
    let notify_points = series_points(&notify);
    let any_points = series_points(&any);
    // Node points as (slot, first shred on the local timeline, local arrival of the notification), in arrival order
    let mut node_points: Vec<(u64, f64, f64)> =
        first_shred.iter().map(|(slot, (at, _, _))| (*slot, shred_local[slot], *at)).collect();
    node_points.sort_by(|a, b| a.2.total_cmp(&b.2));
    let methods: Vec<Method> = vec![
        Method::Arrival { series: Series::SlotNotify, estimator: Estimator { fit_slots: 0, recent: 1, quantile: 0.0 } },
        Method::Arrival { series: Series::Any, estimator: Estimator { fit_slots: 150, recent: 1, quantile: 0.0 } },
        Method::Arrival { series: Series::Any, estimator: Estimator { fit_slots: 150, recent: 8, quantile: 0.25 } },
        Method::Arrival { series: Series::Any, estimator: Estimator { fit_slots: 150, recent: 8, quantile: 0.5 } },
        Method::NodeLinear { estimator: Estimator { fit_slots: 150, recent: 1, quantile: 0.0 } },
        Method::NodeLinear { estimator: Estimator { fit_slots: 150, recent: 8, quantile: 0.5 } },
        Method::NodePhased { anchors: 1 },
        Method::NodePhased { anchors: 2 },
        Method::NodePhased { anchors: 4 },
        Method::NodePhased { anchors: 8 },
    ];
    let mut targets = 0;
    let mut errors = Vec::new();
    for method in &methods {
        for &horizon in horizons {
            let mut e: Vec<f64> = Vec::new();
            match method {
                Method::Arrival { series, estimator } => {
                    let points = match series {
                        Series::SlotNotify => &notify_points,
                        Series::Any => &any_points,
                    };
                    for (&slot, &observed) in any.iter().filter(|(s, _)| *s % 4 == 0 && **s > lo + 200) {
                        let known = points.partition_point(|p| p.1 <= observed - horizon);
                        let mut past: Vec<(u64, f64)> =
                            points[known.saturating_sub(800)..known].iter().filter(|p| p.0 < slot).copied().collect();
                        past.sort_by_key(|p| p.0);
                        if let Some(predicted) = clock::predict(&past, slot, estimator, hint_ms) {
                            e.push(predicted - observed);
                        }
                    }
                }
                Method::NodePhased { .. } | Method::NodeLinear { .. } => {
                    for (&slot, &observed) in shred_local.iter().filter(|(s, _)| *s % 4 == 0 && **s > lo + 200) {
                        let moment = observed - horizon;
                        let known = node_points.partition_point(|p| p.2 <= moment);
                        let mut past: Vec<(u64, f64)> = node_points[known.saturating_sub(800)..known]
                            .iter()
                            .filter(|p| p.0 < slot && p.0 + clock::PHASE_FIT_SLOTS >= slot)
                            .map(|p| (p.0, p.1))
                            .collect();
                        past.sort_by_key(|p| p.0);
                        let predicted = match method {
                            Method::NodePhased { anchors } => clock::predict_phased(&past, slot, *anchors),
                            Method::NodeLinear { estimator } => clock::predict(&past, slot, estimator, hint_ms),
                            Method::Arrival { .. } => unreachable!(),
                        };
                        if let Some(predicted) = predicted {
                            e.push(predicted - observed);
                        }
                    }
                }
            }
            targets = targets.max(e.len());
            errors.push(ErrorStats {
                method: *method,
                label: method.label(),
                horizon_ms: horizon,
                n: e.len(),
                median_ms: quantile(&e, 0.5),
                p10_ms: quantile(&e, 0.1),
                p90_ms: quantile(&e, 0.9),
                mean_abs_ms: e.iter().map(|x| x.abs()).sum::<f64>() / e.len().max(1) as f64,
            });
        }
    }

    let by_client = leaders.map(|leaders| by_client(leaders, &shred_local, &node_points, lo, hi));

    json!({
        "by_client": by_client,
        "events": events.len(),
        "kinds": kinds,
        "slots_updates": slots_updates,
        "slots_seen": any.len(),
        "slot_range": [lo, hi],
        "skipped_slots": skipped,
        "slot_time_hint_ms": hint_ms,
        "slot_interval_by_position_ms": by_position.iter().map(|(position, gaps)| json!({
            "position_in_window": position,
            "n": gaps.len(),
            "median": quantile(gaps, 0.5),
            "p10": quantile(gaps, 0.1),
            "p90": quantile(gaps, 0.9),
        })).collect::<Vec<_>>(),
        "slot_interval_source": if first_shred.len() > any.len() / 2 { "node first-shred timestamps" } else { "local arrival" },
        "connections": by_connection.len(),
        "first_shred_to_local_ms": {
            "n": delivery.len(),
            "q02_lower_envelope": low_delivery,
            "q02_lower_envelope_by_connection": envelope.values().collect::<Vec<_>>(),
            "p10": quantile(&delivery, 0.1),
            "median": quantile(&delivery, 0.5),
            "p90": quantile(&delivery, 0.9),
        },
        "first_shred_to_slot_notify_ms": { "n": shred_to_notify.len(), "median": quantile(&shred_to_notify, 0.5), "p90": quantile(&shred_to_notify, 0.9) },
        "window_starts_evaluated": targets,
        "prediction_errors": errors,
    })
}

/// Per client of the leader: the hand-over into its windows (first shred of its first slot minus that of the slot
/// before, which the previous leader produced) and out of them, the length of its windows (first shred of the last
/// slot minus the first), its skipped windows, and the clock's error at its window starts (phased, 1 anchor, 100 ms
/// ahead), all from the node's timestamps.
fn by_client(leaders: &Leaders, shred_local: &BTreeMap<u64, f64>, node_points: &[(u64, f64, f64)], lo: u64, hi: u64) -> Value {
    #[derive(Default)]
    struct Acc {
        windows: usize,
        skipped: usize,
        hand_over_in: Vec<f64>,
        hand_over_out: Vec<f64>,
        window_ms: Vec<f64>,
        error_ms: Vec<f64>,
    }
    let node = |slot: u64| shred_local.get(&slot).copied();
    let mut acc: BTreeMap<String, Acc> = BTreeMap::new();
    let first_window = lo.div_ceil(4) * 4;
    for start in (first_window..=hi.saturating_sub(4)).step_by(4) {
        let Some(client) = leaders.client(start) else { continue };
        let a = acc.entry(client.to_owned()).or_default();
        a.windows += 1;
        if (start..start + 4).all(|s| node(s).is_none()) {
            a.skipped += 1;
            continue;
        }
        if let (Some(previous), Some(first)) = (node(start - 1), node(start)) {
            a.hand_over_in.push(first - previous);
        }
        if let (Some(first), Some(last)) = (node(start), node(start + 3)) {
            a.window_ms.push(last - first);
        }
        if let (Some(last), Some(next)) = (node(start + 3), node(start + 4)) {
            a.hand_over_out.push(next - last);
        }
        if let Some(observed) = node(start).filter(|_| start > lo + 200) {
            let moment = observed - 100.0;
            let known = node_points.partition_point(|p| p.2 <= moment);
            let mut past: Vec<(u64, f64)> = node_points[known.saturating_sub(800)..known]
                .iter()
                .filter(|p| p.0 < start && p.0 + clock::PHASE_FIT_SLOTS >= start)
                .map(|p| (p.0, p.1))
                .collect();
            past.sort_by_key(|p| p.0);
            if let Some(predicted) = clock::predict_phased(&past, start, 1) {
                a.error_ms.push(predicted - observed);
            }
        }
    }
    let stats = |v: &Vec<f64>| json!({ "n": v.len(), "median": quantile(v, 0.5), "p10": quantile(v, 0.1), "p90": quantile(v, 0.9) });
    let mut rows: Vec<Value> = acc
        .iter()
        .map(|(client, a)| {
            json!({
                "client": client,
                "windows": a.windows,
                "skipped_windows": a.skipped,
                "hand_over_in_ms": stats(&a.hand_over_in),
                "window_ms": stats(&a.window_ms),
                "hand_over_out_ms": stats(&a.hand_over_out),
                "clock_error_ms": stats(&a.error_ms),
            })
        })
        .collect();
    rows.sort_by(|a, b| b["windows"].as_u64().cmp(&a["windows"].as_u64()));
    json!({ "leaders_fetched_at": leaders.fetched_at, "clients": rows })
}

fn print(summary: &Value) {
    if let Some(clients) = summary["by_client"]["clients"].as_array() {
        println!("\nby client of the leader (node first-shred timestamps, ms; median [p10, p90]):");
        println!("{:<20} {:>7} {:>7}  {:<24} {:<24} {:<24} {:<24}", "client", "windows", "skipped", "hand-over in", "window (slot 0→3)", "hand-over out", "clock error at start");
        let cell = |v: &Value| {
            if v["n"].as_u64().unwrap_or(0) == 0 {
                return "–".to_owned();
            }
            format!(
                "{:.0} [{:.0}, {:.0}] n={}",
                v["median"].as_f64().unwrap_or(f64::NAN),
                v["p10"].as_f64().unwrap_or(f64::NAN),
                v["p90"].as_f64().unwrap_or(f64::NAN),
                v["n"]
            )
        };
        for row in clients {
            println!(
                "{:<20} {:>7} {:>7}  {:<24} {:<24} {:<24} {:<24}",
                row["client"].as_str().unwrap_or(""),
                row["windows"],
                row["skipped_windows"],
                cell(&row["hand_over_in_ms"]),
                cell(&row["window_ms"]),
                cell(&row["hand_over_out_ms"]),
                cell(&row["clock_error_ms"]),
            );
        }
    }
    println!(
        "\n{} events {}; slotsUpdates: {}; {} slots seen, {} skipped",
        summary["events"], summary["kinds"], summary["slots_updates"], summary["slots_seen"], summary["skipped_slots"]
    );
    println!("first shred → this machine: {}", summary["first_shred_to_local_ms"]);
    println!("first shred → slot notification: {}", summary["first_shred_to_slot_notify_ms"]);
    println!("slot interval by position in the window ({}):", summary["slot_interval_source"].as_str().unwrap_or(""));
    for row in summary["slot_interval_by_position_ms"].as_array().into_iter().flatten() {
        println!(
            "  slot ≡ {} mod 4: n {:>4}  median {:>6.1}  p10 {:>6.1}  p90 {:>6.1} ms",
            row["position_in_window"], row["n"], row["median"].as_f64().unwrap_or(f64::NAN),
            row["p10"].as_f64().unwrap_or(f64::NAN), row["p90"].as_f64().unwrap_or(f64::NAN)
        );
    }
    println!(
        "\nprediction error at {} window starts (predicted − observed first evidence, ms):",
        summary["window_starts_evaluated"]
    );
    println!("{:<36} {:>8} {:>7} {:>7} {:>7} {:>7}", "method", "horizon", "median", "p10", "p90", "|err|");
    for row in summary["prediction_errors"].as_array().into_iter().flatten() {
        println!(
            "{:<36} {:>8} {:>7.1} {:>7.1} {:>7.1} {:>7.1}",
            row["label"].as_str().unwrap_or(""),
            row["horizon_ms"],
            row["median_ms"].as_f64().unwrap_or(f64::NAN),
            row["p10_ms"].as_f64().unwrap_or(f64::NAN),
            row["p90_ms"].as_f64().unwrap_or(f64::NAN),
            row["mean_abs_ms"].as_f64().unwrap_or(f64::NAN),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regular_stream_is_predicted_without_error() {
        let events: Vec<RawEvent> = (1_000..1_600u64)
            .map(|slot| RawEvent { unix_us: (slot as i64 - 1_000) * 210_000, kind: "slot".into(), slot, node_ms: None })
            .collect();
        let summary = evaluate(&events, 210.0, &[300.0], false, None);
        for row in summary["prediction_errors"].as_array().unwrap() {
            assert!(row["mean_abs_ms"].as_f64().unwrap() < 1e-6, "{row}");
        }
        assert_eq!(summary["skipped_slots"], 0);
    }
}
