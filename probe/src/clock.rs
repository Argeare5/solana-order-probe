//! Slot clock: the RPC node's `slotSubscribe` stream gives the latest slot and when it arrived; slots ahead are
//! extrapolated at the measured slot time. The offset between this clock and the leader's real slot start is part
//! of the send lead, which the mainnet dry-run calibrates.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

#[derive(Clone, Copy, Debug)]
pub struct SlotTick {
    pub slot: u64,
    pub at: Instant,
}

pub struct SlotClock {
    rx: watch::Receiver<Option<SlotTick>>,
    slot_ms: f64,
}

impl SlotClock {
    pub async fn start(ws_url: String, slot_ms: f64) -> Result<Self> {
        let (tx, rx) = watch::channel(None);
        tokio::spawn(async move {
            loop {
                if let Err(error) = stream(&ws_url, &tx).await {
                    eprintln!("slot stream: {error:#}; reconnecting");
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });
        let mut first = rx.clone();
        tokio::time::timeout(Duration::from_secs(15), first.wait_for(Option::is_some))
            .await
            .context("no slot notification within 15 s")??;
        Ok(Self { rx, slot_ms })
    }

    pub fn latest(&self) -> SlotTick {
        self.rx.borrow().expect("the clock starts with a tick")
    }

    pub fn slot_ms(&self) -> f64 {
        self.slot_ms
    }

    /// When `slot` is expected to start by this clock.
    pub fn eta(&self, slot: u64) -> Instant {
        let tick = self.latest();
        let ms = (slot as f64 - tick.slot as f64) * self.slot_ms;
        if ms >= 0.0 { tick.at + Duration::from_secs_f64(ms / 1e3) } else { tick.at - Duration::from_secs_f64(-ms / 1e3) }
    }

    /// The slot this clock estimates for now, with its fraction.
    pub fn now_slot(&self) -> f64 {
        let tick = self.latest();
        tick.slot as f64 + tick.at.elapsed().as_secs_f64() * 1e3 / self.slot_ms
    }
}

async fn stream(url: &str, tx: &watch::Sender<Option<SlotTick>>) -> Result<()> {
    let (mut ws, _) = connect_async(url).await.context("connecting the WebSocket")?;
    ws.send(Message::Text(json!({ "jsonrpc": "2.0", "id": 1, "method": "slotSubscribe" }).to_string().into())).await?;
    while let Some(message) = ws.next().await {
        match message? {
            Message::Text(text) => {
                let value: Value = serde_json::from_str(&text)?;
                if let Some(slot) = value["params"]["result"]["slot"].as_u64() {
                    // Notifications from forks can arrive out of order: the clock only moves forward
                    let newer = tx.borrow().is_none_or(|tick| slot > tick.slot);
                    if newer {
                        tx.send_replace(Some(SlotTick { slot, at: Instant::now() }));
                    }
                }
            }
            Message::Ping(payload) => ws.send(Message::Pong(payload)).await?,
            Message::Close(_) => break,
            _ => {}
        }
    }
    bail!("the WebSocket closed")
}
