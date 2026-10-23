//! Leader windows of chosen validators from `getLeaderSchedule` (filtered by identity). A window is a run of
//! consecutive leader slots: 4 slots, or 8 and 12 when the schedule gives one leader consecutive windows.

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::rpc::Rpc;

/// Longest window kept as one. Mainnet leaders hold at most 12 consecutive slots (three windows in a row); a leader
/// that holds more, as the only node of a test cluster does, is split into windows of this length.
const MAX_WINDOW_SLOTS: u64 = 48;

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Window {
    pub start: u64,
    pub slots: u64,
}

impl Window {
    pub fn end(&self) -> u64 {
        self.start + self.slots - 1
    }
}

pub struct EpochBounds {
    pub epoch: u64,
    pub first_slot: u64,
    pub slots: u64,
}

pub async fn epoch_bounds(rpc: &Rpc) -> Result<EpochBounds> {
    let info = rpc.call("getEpochInfo", json!([{ "commitment": "confirmed" }])).await?;
    let current = info["absoluteSlot"].as_u64().context("absoluteSlot")?;
    Ok(EpochBounds {
        epoch: info["epoch"].as_u64().context("epoch")?,
        first_slot: current - info["slotIndex"].as_u64().context("slotIndex")?,
        slots: info["slotsInEpoch"].as_u64().context("slotsInEpoch")?,
    })
}

/// Slot time in ms over the node's last ten performance samples (60 s each). A node younger than a minute has none
/// yet: then the protocol's default of 400 ms is used (the slot clock fits the real slot time as it runs).
pub async fn slot_ms(rpc: &Rpc) -> Result<f64> {
    let samples = rpc.call("getRecentPerformanceSamples", json!([10])).await?;
    let (slots, secs) = samples.as_array().context("performance samples")?.iter().fold((0u64, 0u64), |acc, s| {
        (acc.0 + s["numSlots"].as_u64().unwrap_or(0), acc.1 + s["samplePeriodSecs"].as_u64().unwrap_or(0))
    });
    if slots == 0 {
        eprintln!("no performance samples yet: assuming 400 ms slots");
        return Ok(400.0);
    }
    Ok(secs as f64 * 1000.0 / slots as f64)
}

/// Windows of `identity` in the epoch that contains `slot_in_epoch` (whose first slot is `first_slot`).
pub async fn windows(rpc: &Rpc, identity: &str, slot_in_epoch: u64, first_slot: u64) -> Result<Vec<Window>> {
    let result = rpc.call("getLeaderSchedule", json!([slot_in_epoch, { "identity": identity, "commitment": "confirmed" }])).await?;
    let mut slots: Vec<u64> = result
        .get(identity)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .map(|index| first_slot + index)
        .collect();
    slots.sort_unstable();
    let mut out: Vec<Window> = Vec::new();
    for slot in slots {
        match out.last_mut() {
            Some(window) if window.end() + 1 == slot && window.slots < MAX_WINDOW_SLOTS => window.slots += 1,
            _ => out.push(Window { start: slot, slots: 1 }),
        }
    }
    Ok(out)
}
