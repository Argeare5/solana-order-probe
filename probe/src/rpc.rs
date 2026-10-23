//! Minimal JSON-RPC client. Requests are built as plain JSON so that new transaction versions or response fields
//! never break parsing: only the fields the probe needs are read.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct Rpc {
    url: String,
    http: reqwest::Client,
}

impl Rpc {
    pub fn new(url: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .context("building the HTTP client")?;
        Ok(Self { url: url.to_owned(), http })
    }

    /// Calls `method` and returns its `result`. Retries rate limits and transient HTTP errors with backoff.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
        let mut delay = Duration::from_millis(400);
        for attempt in 1..=6 {
            let response = match self.http.post(&self.url).json(&body).send().await {
                Ok(response) => response,
                Err(error) if attempt < 6 => {
                    eprintln!("{method}: {error}, retrying");
                    tokio::time::sleep(delay).await;
                    delay *= 2;
                    continue;
                }
                Err(error) => return Err(error).with_context(|| format!("{method}: request failed")),
            };
            let status = response.status();
            if status.as_u16() == 429 || status.is_server_error() {
                tokio::time::sleep(delay).await;
                delay *= 2;
                continue;
            }
            let value: Value = response.json().await.with_context(|| format!("{method}: invalid JSON"))?;
            if let Some(error) = value.get("error") {
                bail!("{method}: {error}");
            }
            return Ok(value["result"].clone());
        }
        bail!("{method}: still rate limited after retries")
    }
}
