//! Leader windows of chosen validators from `getLeaderSchedule` (filtered by identity). A window is a run of
//! consecutive leader slots: 4 slots, or 8 and 12 when the schedule gives one leader consecutive windows.

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::rpc::Rpc;

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
            Some(window) if window.end() + 1 == slot => window.slots += 1,
            _ => out.push(Window { start: slot, slots: 1 }),
        }
    }
    Ok(out)
}
