//! Slot clock. The RPC node reports the slots it sees over its WebSocket: `slotSubscribe` when it creates a slot's
//! bank and, where the node offers it, `slotsUpdatesSubscribe` from the first shred received on, with the node's own
//! timestamp. The public mainnet WebSocket delivers these in batches (first shred → this machine: median ~180 ms,
//! p90 ~900 ms on 23.10.2026), so local arrival times are a poor clock, while the node's timestamps are regular.
//!
//! The clock therefore predicts a slot's first shred at the node from the node's timestamps: each slot's duration
//! depends on its position in the 4-slot leader window (the window's first slot carries the hand-over from the
//! previous leader), so the median duration per position over the last ~150 slots is added up from the latest few
//! known slots, and the median of those projections taken. Node time maps to local time by the lower envelope of
//! the delivery delay (the node's clock and ours differ by tens of ms). A public endpoint may put a reconnection on
//! another node with another clock, so the offset and the node timestamps are only used within one connection.
//! Without slot updates the clock falls back to the local arrival of each slot's first notification, projected at
//! the fitted slot time.
//!
//! Under Alpenglow a leader starts its window when the previous one is done, not on a global timer, so the error of
//! a prediction grows with how far ahead it looks. `run` re-predicts in a tight loop until the fire moment and
//! records the observed first shred of the window with every burst; `calibrate` measures the prediction error.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

/// Silence after which the WebSocket is taken as stalled and reconnected.
const STALL: Duration = Duration::from_secs(3);
/// Slots of first evidence kept in memory.
const KEPT_SLOTS: u64 = 4_000;
/// Slots in a leader window (`NUM_CONSECUTIVE_LEADER_SLOTS`); windows start at slots divisible by it.
pub const WINDOW_SLOTS: u64 = 4;
/// Slots over which the per-position slot durations are taken.
pub const PHASE_FIT_SLOTS: u64 = 150;
/// Latest known slots projected forward, whose median is the prediction (chosen with `calibrate`).
pub const ANCHORS: usize = 1;
/// Delivery delays kept for the node-to-local offset (~10 min of slots), and the quantile taken as its lower
/// envelope. On the public mainnet endpoint the envelope drifts by ~1 ms a minute, while in some minutes every
/// notification is late (the envelope rises by 100+ ms); a long window with a low quantile rides those out.
const DELIVERIES_KEPT: usize = 3_000;
const DELIVERY_QUANTILE: f64 = 0.01;

/// How a slot ahead is predicted from local arrival times (the fallback without slot updates).
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Estimator {
    /// Slots over which the slot time is fitted (least squares); 0 keeps the hint from the performance samples.
    pub fit_slots: u64,
    /// Most recent slots whose projections to the slot ahead are combined.
    pub recent: usize,
    /// Quantile of those projections that is taken (0 = the earliest).
    pub quantile: f64,
}

/// The arrival-based estimator, chosen with `calibrate` on mainnet: the latest slot at the fitted slot time.
pub const ESTIMATOR: Estimator = Estimator { fit_slots: 150, recent: 1, quantile: 0.0 };

/// What the clock predicts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// The node's first-shred timestamps, mapped to local time.
    NodeFirstShred,
    /// Local arrival of each slot's first notification.
    Arrival,
}

/// First evidence of a slot.
#[derive(Clone, Copy, Debug)]
pub struct Observed {
    /// Local arrival of the slot's first notification.
    pub unix_us: i64,
    /// The node's own timestamp of the slot's first shred, in ms since the Unix epoch (`slotsUpdatesSubscribe`).
    pub first_shred_node_ms: Option<u64>,
    /// The WebSocket connection it came over (counted from 1).
    pub connection: u64,
}

/// A notification as it arrived, for the calibration log.
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub struct RawEvent {
    pub unix_us: i64,
    /// `slot` for `slotSubscribe`, `connected` when the WebSocket (re)connects, otherwise the `type` of the slots
    /// update.
    pub kind: String,
    pub slot: u64,
    pub node_ms: Option<u64>,
}

#[derive(Default)]
struct Seen {
    slots: BTreeMap<u64, Observed>,
    /// Local arrival minus node timestamp of recent first-shred notifications on the current connection, in ms.
    deliveries: VecDeque<f64>,
    slots_updates: bool,
    connection: u64,
}

pub struct SlotClock {
    seen: Arc<Mutex<Seen>>,
    hint_ms: f64,
}

pub fn unix_us() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_micros() as i64)
}

/// The instant at Unix time `unix_ms`.
pub fn instant_at(unix_ms: f64) -> Instant {
    shift(Instant::now(), unix_ms - unix_us() as f64 / 1e3)
}

/// `at` moved by `ms` (negative = earlier).
pub fn shift(at: Instant, ms: f64) -> Instant {
    let by = Duration::from_secs_f64(ms.abs() / 1e3);
    if ms >= 0.0 { at + by } else { at - by }
}

impl SlotClock {
    /// Starts the WebSocket stream and waits for the first slot. `hint_ms` is the slot time from the performance
    /// samples, used until enough slots are seen. Every notification is also sent to `log`, if given.
    pub async fn start(ws_url: String, hint_ms: f64, log: Option<mpsc::UnboundedSender<RawEvent>>) -> Result<Self> {
        let seen = Arc::new(Mutex::new(Seen::default()));
        let writer = seen.clone();
        tokio::spawn(async move {
            loop {
                if let Err(error) = stream(&ws_url, &writer, log.as_ref()).await {
                    eprintln!("slot stream: {error:#}; reconnecting");
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });
        let started = Instant::now();
        while seen.lock().unwrap().slots.is_empty() {
            if started.elapsed() > Duration::from_secs(15) {
                bail!("no slot notification within 15 s");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok(Self { seen, hint_ms })
    }

    /// Waits until the clock predicts from node timestamps, or `timeout` passes (then it stays on arrival times).
    pub async fn settle(&self, timeout: Duration) -> Basis {
        let until = Instant::now() + timeout;
        while self.basis() != Basis::NodeFirstShred && Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        self.basis()
    }

    /// Whether the node delivers `slotsUpdatesSubscribe` (first shreds).
    pub fn has_slots_updates(&self) -> bool {
        self.seen.lock().unwrap().slots_updates
    }

    pub fn latest_slot(&self) -> u64 {
        *self.seen.lock().unwrap().slots.keys().next_back().expect("the clock starts with a slot")
    }

    pub fn observed(&self, slot: u64) -> Option<Observed> {
        self.seen.lock().unwrap().slots.get(&slot).copied()
    }

    /// Node clock minus local clock, as the lower envelope of the delivery delay (negated), in ms.
    pub fn node_offset_ms(&self) -> Option<f64> {
        let seen = self.seen.lock().unwrap();
        if seen.deliveries.len() < 30 {
            return None;
        }
        let deliveries: Vec<f64> = seen.deliveries.iter().copied().collect();
        Some(-quantile(&deliveries, DELIVERY_QUANTILE))
    }

    /// The node's first-shred timestamps of recent slots as (slot, ms), ascending by slot.
    fn node_points(&self) -> Vec<(u64, f64)> {
        let seen = self.seen.lock().unwrap();
        let last = seen.slots.keys().next_back().copied().unwrap_or(0);
        seen.slots
            .range(last.saturating_sub(PHASE_FIT_SLOTS)..)
            .filter(|(_, o)| o.connection == seen.connection)
            .filter_map(|(slot, o)| Some((*slot, o.first_shred_node_ms? as f64)))
            .collect()
    }

    /// Local arrival of recent slots as (slot, Unix ms), ascending by slot.
    fn arrival_points(&self) -> Vec<(u64, f64)> {
        let seen = self.seen.lock().unwrap();
        let last = seen.slots.keys().next_back().copied().unwrap_or(0);
        let from = last.saturating_sub(ESTIMATOR.fit_slots.max(ESTIMATOR.recent as u64));
        seen.slots.range(from..).map(|(slot, o)| (*slot, o.unix_us as f64 / 1e3)).collect()
    }

    pub fn basis(&self) -> Basis {
        if self.node_offset_ms().is_some() && self.node_points().len() >= 32 { Basis::NodeFirstShred } else { Basis::Arrival }
    }

    /// Mean slot time in ms on the current basis.
    pub fn slot_ms(&self) -> f64 {
        match self.basis() {
            Basis::NodeFirstShred => phase_gaps(&self.node_points()).map_or(self.hint_ms, |g| g.iter().sum::<f64>() / 4.0),
            Basis::Arrival => slot_time(&self.arrival_points(), &ESTIMATOR, self.hint_ms),
        }
    }

    /// The predicted first evidence of `slot`, in local Unix ms.
    pub fn predict_unix_ms(&self, slot: u64) -> f64 {
        if let (Basis::NodeFirstShred, Some(offset)) = (self.basis(), self.node_offset_ms())
            && let Some(node_ms) = predict_phased(&self.node_points(), slot, ANCHORS)
        {
            return node_ms - offset;
        }
        predict(&self.arrival_points(), slot, &ESTIMATOR, self.hint_ms).expect("the clock starts with a slot")
    }

    /// When the first evidence of `slot` is expected, by the current prediction.
    pub fn predict(&self, slot: u64) -> Instant {
        instant_at(self.predict_unix_ms(slot))
    }

    /// The observed first evidence of `slot` in local Unix ms, on the current basis.
    pub fn observed_unix_ms(&self, slot: u64) -> Option<f64> {
        let observed = self.observed(slot)?;
        let current = observed.connection == self.seen.lock().unwrap().connection;
        match (self.basis(), observed.first_shred_node_ms.filter(|_| current), self.node_offset_ms()) {
            (Basis::NodeFirstShred, Some(node_ms), Some(offset)) => Some(node_ms as f64 - offset),
            _ => Some(observed.unix_us as f64 / 1e3),
        }
    }

    /// Waits until the first evidence of `slot` arrives, or `deadline` passes, or the clock is well past the slot
    /// (a skipped slot is never seen).
    pub async fn wait_observed(&self, slot: u64, deadline: Instant) -> Option<Observed> {
        loop {
            if let Some(observed) = self.observed(slot).filter(|o| o.first_shred_node_ms.is_some() || !self.has_slots_updates()) {
                return Some(observed);
            }
            if Instant::now() > deadline || self.latest_slot() > slot + 8 {
                return self.observed(slot);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// The slot this clock estimates for now, with its fraction.
    pub fn now_slot(&self) -> f64 {
        let latest = self.latest_slot();
        let at = self.predict_unix_ms(latest);
        latest as f64 + (unix_us() as f64 / 1e3 - at) / self.slot_ms()
    }
}

pub fn quantile(values: &[f64], q: f64) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let position = (sorted.len() - 1) as f64 * q;
    let (low, high) = (position.floor() as usize, position.ceil() as usize);
    sorted[low] + (sorted[high] - sorted[low]) * (position - low as f64)
}

/// Median duration of a slot by its position in the leader window (`slot % 4`), from the consecutive pairs among
/// `points` (slot, ms). A position with fewer than 5 pairs takes the median over all positions.
pub fn phase_gaps(points: &[(u64, f64)]) -> Option<[f64; 4]> {
    let mut by_position: [Vec<f64>; 4] = Default::default();
    for pair in points.windows(2) {
        let ((a, ta), (b, tb)) = (pair[0], pair[1]);
        if b == a + 1 {
            by_position[(b % WINDOW_SLOTS) as usize].push(tb - ta);
        }
    }
    let all: Vec<f64> = by_position.iter().flatten().copied().collect();
    if all.is_empty() {
        return None;
    }
    let overall = quantile(&all, 0.5);
    Some(std::array::from_fn(|p| if by_position[p].len() >= 5 { quantile(&by_position[p], 0.5) } else { overall }))
}

/// Predicted time of `slot` (on the points' timeline): the last `anchors` known slots before it, each moved forward
/// by the per-position durations, and the median of those projections. A slot already known is returned as is.
pub fn predict_phased(points: &[(u64, f64)], slot: u64, anchors: usize) -> Option<f64> {
    if let Some(&(_, ms)) = points.iter().find(|p| p.0 == slot) {
        return Some(ms);
    }
    let gaps = phase_gaps(points)?;
    let projections: Vec<f64> = points
        .iter()
        .rev()
        .filter(|p| p.0 < slot)
        .take(anchors.max(1))
        .map(|&(anchor, ms)| ms + (anchor + 1..=slot).map(|s| gaps[(s % WINDOW_SLOTS) as usize]).sum::<f64>())
        .collect();
    (!projections.is_empty()).then(|| quantile(&projections, 0.5))
}

/// Slot time in ms: least squares of arrival on slot over the last `fit_slots` slots, or `hint_ms` when there are
/// too few points or the fit is implausible. `points` are (slot, ms) ascending by slot.
pub fn slot_time(points: &[(u64, f64)], estimator: &Estimator, hint_ms: f64) -> f64 {
    let Some(&(last, _)) = points.last() else { return hint_ms };
    if estimator.fit_slots == 0 {
        return hint_ms;
    }
    let window: Vec<(f64, f64)> = points
        .iter()
        .filter(|(slot, _)| slot + estimator.fit_slots >= last)
        .map(|&(slot, ms)| (slot as f64 - last as f64, ms))
        .collect();
    if window.len() < 16 {
        return hint_ms;
    }
    let n = window.len() as f64;
    let mean_x = window.iter().map(|p| p.0).sum::<f64>() / n;
    let mean_y = window.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = window.iter().map(|p| (p.0 - mean_x).powi(2)).sum();
    let sxy: f64 = window.iter().map(|p| (p.0 - mean_x) * (p.1 - mean_y)).sum();
    let slope = sxy / sxx;
    if slope.is_finite() && slope > hint_ms * 0.5 && slope < hint_ms * 2.0 { slope } else { hint_ms }
}

/// Predicted first evidence of `slot` (ms on the points' timeline): each of the last `recent` points projected to
/// `slot` at the fitted slot time, and the `quantile` of those projections taken.
pub fn predict(points: &[(u64, f64)], slot: u64, estimator: &Estimator, hint_ms: f64) -> Option<f64> {
    let slot_ms = slot_time(points, estimator, hint_ms);
    let mut projections: Vec<f64> =
        points.iter().rev().take(estimator.recent.max(1)).map(|&(s, ms)| ms + (slot as f64 - s as f64) * slot_ms).collect();
    if projections.is_empty() {
        return None;
    }
    projections.sort_by(f64::total_cmp);
    let index = ((projections.len() - 1) as f64 * estimator.quantile).round() as usize;
    Some(projections[index])
}

async fn stream(url: &str, seen: &Mutex<Seen>, log: Option<&mpsc::UnboundedSender<RawEvent>>) -> Result<()> {
    let (mut ws, _) = connect_async(url).await.context("connecting the WebSocket")?;
    {
        // Possibly another node behind the endpoint, with another clock: learn its offset afresh
        let mut seen = seen.lock().unwrap();
        seen.connection += 1;
        seen.deliveries.clear();
    }
    if let Some(log) = log {
        let _ = log.send(RawEvent { unix_us: unix_us(), kind: "connected".to_owned(), slot: 0, node_ms: None });
    }
    ws.send(Message::Text(json!({ "jsonrpc": "2.0", "id": 1, "method": "slotSubscribe" }).to_string().into())).await?;
    ws.send(Message::Text(json!({ "jsonrpc": "2.0", "id": 2, "method": "slotsUpdatesSubscribe" }).to_string().into()))
        .await?;
    // Slots come every ~200 ms; a stream that stays silent much longer has stalled without closing
    loop {
        let message = match tokio::time::timeout(STALL, ws.next()).await {
            Ok(Some(message)) => message,
            Ok(None) => break,
            Err(_) => bail!("no notification for {} s", STALL.as_secs()),
        };
        match message? {
            Message::Text(text) => {
                let value: Value = serde_json::from_str(&text)?;
                if value["id"] == 2 {
                    let ok = value["result"].is_u64();
                    seen.lock().unwrap().slots_updates = ok;
                    if !ok {
                        eprintln!("slotsUpdatesSubscribe is not available here: {}", value["error"]);
                    }
                    continue;
                }
                let result = &value["params"]["result"];
                let Some(slot) = result["slot"].as_u64() else { continue };
                let (kind, node_ms) = match value["method"].as_str() {
                    Some("slotNotification") => ("slot".to_owned(), None),
                    Some("slotsUpdatesNotification") => {
                        (result["type"].as_str().unwrap_or("unknown").to_owned(), result["timestamp"].as_u64())
                    }
                    _ => continue,
                };
                let event = RawEvent { unix_us: unix_us(), kind, slot, node_ms };
                record(seen, &event);
                if let Some(log) = log {
                    let _ = log.send(event);
                }
            }
            Message::Ping(payload) => ws.send(Message::Pong(payload)).await?,
            Message::Close(_) => break,
            _ => {}
        }
    }
    bail!("the WebSocket closed")
}

fn record(seen: &Mutex<Seen>, event: &RawEvent) {
    // Root, confirmation and dead-slot updates come long after the slot started: they are no evidence of its start
    if matches!(event.kind.as_str(), "root" | "optimisticConfirmation" | "dead" | "connected") {
        return;
    }
    let first_shred = (event.kind == "firstShredReceived").then_some(event.node_ms).flatten();
    let mut seen = seen.lock().unwrap();
    if let Some(node_ms) = first_shred {
        seen.deliveries.push_back(event.unix_us as f64 / 1e3 - node_ms as f64);
        if seen.deliveries.len() > DELIVERIES_KEPT {
            seen.deliveries.pop_front();
        }
    }
    let connection = seen.connection;
    let entry =
        seen.slots.entry(event.slot).or_insert(Observed { unix_us: event.unix_us, first_shred_node_ms: None, connection });
    if entry.first_shred_node_ms.is_none() {
        entry.first_shred_node_ms = first_shred;
    }
    let last = *seen.slots.keys().next_back().unwrap();
    while seen.slots.first_key_value().is_some_and(|(slot, _)| slot + KEPT_SLOTS < last) {
        seen.slots.pop_first();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Slots of a mainnet-like window: 260 ms for the hand-over slot, then 150, 200, 200 ms.
    fn phased(slots: std::ops::Range<u64>) -> Vec<(u64, f64)> {
        const GAPS: [f64; 4] = [260.0, 150.0, 200.0, 200.0];
        let mut t = 0.0;
        slots.map(|s| { t += GAPS[(s % 4) as usize]; (s, t) }).collect()
    }

    #[test]
    fn phase_gaps_are_recovered() {
        assert_eq!(phase_gaps(&phased(100..200)), Some([260.0, 150.0, 200.0, 200.0]));
    }

    #[test]
    fn window_start_is_predicted_with_the_hand_over() {
        let points = phased(100..203);
        let truth = phased(100..205);
        assert_eq!(predict_phased(&points, 204, 2), Some(truth.last().unwrap().1));
        assert_eq!(predict_phased(&points, 150, 2), Some(points[50].1));
    }

    #[test]
    fn regular_slots_are_predicted_exactly() {
        let points: Vec<(u64, f64)> = (100..200).map(|s| (s, (s - 100) as f64 * 210.0)).collect();
        assert!((slot_time(&points, &ESTIMATOR, 400.0) - 210.0).abs() < 1e-9);
        assert!((predict(&points, 210, &ESTIMATOR, 400.0).unwrap() - 110.0 * 210.0).abs() < 1e-6);
    }

    #[test]
    fn a_late_notification_is_rejected_by_a_low_quantile() {
        let mut points: Vec<(u64, f64)> = (100..200).map(|s| (s, (s - 100) as f64 * 210.0)).collect();
        points.last_mut().unwrap().1 += 150.0;
        let estimator = Estimator { fit_slots: 150, recent: 8, quantile: 0.25 };
        let predicted = predict(&points, 204, &estimator, 210.0).unwrap();
        assert!((predicted - 104.0 * 210.0).abs() < 5.0, "{predicted}");
    }

    #[test]
    fn few_points_fall_back_to_the_hint() {
        let points = vec![(10, 0.0), (11, 300.0)];
        let estimator = Estimator { fit_slots: 150, recent: 1, quantile: 0.0 };
        assert_eq!(slot_time(&points, &estimator, 210.0), 210.0);
        assert_eq!(predict(&points, 13, &estimator, 210.0), Some(720.0));
    }

    #[test]
    fn quantiles_interpolate() {
        let v = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(quantile(&v, 0.5), 2.5);
        assert_eq!(quantile(&v, 0.0), 1.0);
        assert!(quantile(&[], 0.5).is_nan());
    }
}
