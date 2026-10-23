# AGENTS.md

Notes for coding agents and contributors working on this repository.

## What this is

A measurement study for the Superteam Ukraine bounty "Inside the Block: How Solana Validators Order Transactions":
probe transactions are sent into chosen leaders' windows on Solana mainnet, and their order inside the resulting
blocks shows which rule each leader follows. `README.md` explains the design; `report/REPORT.md` holds dated
measurements and what went wrong.

## Hard rules

- **Nothing is sent without `--live`.** `run`, `fund` and `sweep` are dry-runs by default; keep it that way for every
  new command that sends. A live command on mainnet spends real SOL: run one only when the repository owner asked for
  that exact run.
- **The spend cap is enforced in code** (`probe/src/ledger.rs`): reserve a burst's worst case before sending, record
  real fees from the chain, never bypass `Ledger::reserve`. Fees are recorded once per signature;
  `probe/src/reconcile.rs` adds any fee collection missed.
- **Keys live only in `./secrets`** (gitignored, mounted into the container). Never print, log, commit or move a
  keypair; never ask for one in chat. The owner's wallet never enters the tool: it funds a throwaway funder created
  by `keys --new-funder`, and `sweep --live --to <wallet>` returns what is left.
- **Measurements are mainnet only, after Alpenglow** (slot 459 653 000, October 21, 2026). Devnet, testnet and local
  validators are for pipeline tests; keep their data out of `data/` (use `--data-dir` and `--secrets-dir` elsewhere).
- **No Jito bundles or tips.** Probes are ordinary transactions.
- **Raw data is never edited by hand.** Everything in `data/` is written by `order-probe`; `report/generated/` is
  written by `order_analysis` and is regenerated, not edited.

## Layout

| Path | What |
| --- | --- |
| `probe/src/main.rs` | CLI (clap): `recon`, `calibrate`, `simulate`, `quic-ping`, `keys`, `airdrop`, `fund`, `sweep`, `run`, `reconcile` |
| `probe/src/run.rs` | Bursts: target windows, factorial conditions, fire timing, QUIC sends, records |
| `probe/src/clock.rs` | Slot clock on the RPC node's first-shred timestamps; prediction functions used by `calibrate` |
| `probe/src/collect.rs`, `reconcile.rs` | Landing positions, compact blocks, fees into the ledger |
| `probe/src/probe.rs` | The probe transaction (compute budget, 0-lamport transfer, Memo v1) |
| `probe/src/quic.rs` | QUIC client to a TPU, configured like Agave's `tpu-client-next` |
| `analyze/order_analysis/` | `load` (raw data → tables), `metrics` (pair shares, bootstrap, permutation, logistic), `report` (tables, figures, summary), `network` (TPU ingress, hand-over), `background` (whole blocks), `synth` (synthetic runs for tests) |
| `data/` | `snapshots/`, `calibration/`, `runs/<run>/{run.json, bursts.jsonl, results.jsonl, reconciled.jsonl, blocks/}`, `spend_ledger.jsonl` |

## Commands

```bash
cargo test --manifest-path probe/Cargo.toml
cargo clippy --manifest-path probe/Cargo.toml --all-targets -- -D warnings
cargo build --release --manifest-path probe/Cargo.toml        # ./probe/target/release/order-probe
docker build -t solana-order-probe/analyze analyze
docker run --rm --entrypoint python -w /app solana-order-probe/analyze -m pytest -q tests
docker compose run --rm analyze                               # regenerate report/generated from data/
docker compose up                                             # dry-run on mainnet + analysis
```

Read-only checks against mainnet (no keys needed): `recon`, `calibrate --minutes 5`, `simulate --fee-payer <any funded
address>`, `run --targets <identity>` without `--live`.

## Conventions

- Rust 2024 on toolchain 1.99.1 (`probe/rust-toolchain.toml`); `anyhow` for errors; RPC responses are read as
  `serde_json::Value`, so new transaction versions or fields never break parsing (blocks are fetched with
  `maxSupportedTransactionVersion: 1`).
- A run must never lose a sent burst: errors inside the burst loop end the loop, then the bursts already sent are
  collected and reconciled before `run` returns.
- Doc comments say why, in plain sentences; keep the existing density.
- Python: pandas, numpy, scipy, matplotlib, pinned in `analyze/requirements.txt`; every random draw is seeded so the
  analysis is reproducible. New metrics get a synthetic test with a leader that follows a known rule.
- Charts use the palette at the top of `analyze/order_analysis/report.py`.
- Record decisions and dated measurements in `report/REPORT.md`; anything that went wrong goes in its "What went wrong"
  section.
