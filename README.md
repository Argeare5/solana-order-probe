# solana-order-probe

How do Solana leaders order the transactions inside their blocks? This repository measures it on mainnet with our own
probe transactions: bursts of same-shape transactions that differ only in their compute unit price, sent straight to
the leader's TPU over QUIC in a random order, then located in the block with `getBlock`.

- `probe/` — `order-probe`, a Rust binary: cluster snapshot, slot clock, probe bursts, block collection, spend cap.
- `analyze/` — `order_analysis`, a Python package: within-block metrics, null model, figures, tables.
- `data/` — every raw record: snapshots, calibrations, runs (bursts, signatures, slots, compact blocks), spend ledger.
- `report/` — `REPORT.md` (dated measurements, results, what went wrong) and `generated/` (tables and figures).

Measurements are taken after Alpenglow's activation on October 21, 2026 (slot 459 653 000) only.

## Run it

```bash
docker compose up
```

With no keys and no spending this

- takes a **dry-run** against the current mainnet: finds the next leader windows of the targets in `.env`, builds and
  times one burst for each, opens the QUIC connection to the leader and sends nothing;
- recomputes every table and figure in `report/generated` from the raw data in `data/`.

Real sends need a funded key in `./secrets` and the explicit `--live` flag, and stop at the spend cap
(`SPEND_CAP_SOL`, 0.05 SOL by default):

```bash
cp .env.example .env                                             # RPC endpoints, spend cap, targets
docker compose run --rm probe keys --new-funder                  # a fresh funder key in ./secrets; prints its address
# send ~0.07 SOL to that address from your wallet; your wallet's key never enters the tool
docker compose run --rm probe fund --live                        # 12 payers, one per probe of a burst
docker compose run --rm probe run --targets <identity,...> --bursts-per-target 24 --live
docker compose run --rm probe sweep --live --to <your wallet>    # everything left goes back
docker compose run --rm analyze
```

| Command | What it does | Sends? |
| --- | --- | --- |
| `recon` | Snapshot of clients (`clientId`, version), stake, TPU addresses, slot time, Alpenglow state, candidate leaders | no |
| `calibrate` | Records the slot notifications and measures how well the slot clock predicts window starts | no |
| `simulate` | Compute units a probe consumes, through `simulateTransaction` | no |
| `quic-ping` | QUIC handshake with a TPU | no |
| `keys` | Funder and payers with balances; `--new-funder` creates a funder key | no |
| `airdrop` | Devnet, testnet or a local validator only | — |
| `fund`, `sweep` | Top the payers up / return everything to the funder (`--to` sends it on) | only with `--live` |
| `run` | Bursts into the targets' leader windows, then collection | only with `--live` |

Every fee paid (probes, funding, sweeping) is read back from the chain and appended to `data/spend_ledger.jsonl`.
Before each burst its worst case is reserved, and a burst that could pass the cap is refused.

## Design

**A burst** is 12 probes sent within a few milliseconds into one leader window (4 consecutive slots of one leader).
A probe is `SetComputeUnitLimit(900)`, `SetComputeUnitPrice(p)`, a 0-lamport transfer and a Memo v1 tag
`sop:<run>:<burst>:<i>`. It consumes 806 compute units on mainnet. Every probe has its own fee payer, so probes never
write-lock a common account (except in the conflict variant), and every probe has the same accounts, size and limit,
so the leader's cost model gives them all the same cost: the reward per cost unit is ordered exactly as the price.

**Factorial assignment.** In every burst the 12 prices of a ladder (1 000 … 5 000 000 µlamports/CU) are dealt to the
payers in a fresh random order, and the send order is an independent random permutation (seed in `run.json`).
Price, payer and send order are therefore independent, so a correlation of block position with one of them cannot be
an artefact of another.

**Variants**, cycled per validator:

| Variant | Prices | Send order | Tells apart |
| --- | --- | --- | --- |
| main | ladder | random | fee rule vs arrival (FIFO) vs noise |
| null | all equal | random | arrival order alone; the noise floor of the block; payer-index control |
| reverse | ladder | cheapest first, most expensive last | fee against arrival head-on |
| conflict | ladder | random; half the probes write-lock one shared account | whether conflicting transactions still follow the fee |

**Timing.** Each burst fires at a planned offset from the predicted start of the window: −300 ms (the burst waits at
the leader, which schedules it all at once) or +100 ms (probes stream in while the leader builds its block), with
0, 10 or 50 ms between sends. The prediction uses the RPC node's own first-shred timestamps (`slotsUpdatesSubscribe`)
and each slot's duration by its position in the window, re-predicted every 5 ms until the moment comes; every burst
records when it really went out against the window's observed first shred. In mainnet dry-runs on October 23
(34 bursts) the fire moment was within 50 ms of the plan for 74% of bursts and within 100 ms for 91% (median
deviation 33 ms, worst 362 ms when a hand-over was slow); see `report/REPORT.md` and `data/calibration/`.

**Path.** Probes go straight to the leader's TPU over QUIC, configured as Agave's own `tpu-client-next` configures it.
Our connection is unstaked, so stake-weighted QoS gives it a small share; probes that do not land are counted, not
re-sent. For Jito-Solana, Agave, Firedancer and Rakurai the TPU in gossip is the validator's own host. For the BAM and
Harmonic clients it never is: their nodes advertise a shared ingress of the external sequencer or block builder
(on October 23, 361 AgaveBam nodes behind 17 TPU hosts, up to 96 behind one; 59 HarmonicAgave nodes behind 7).
For those leaders the arrival order we control is the order at that ingress, and the rule we measure is the
ingress's and the leader's together (`report/generated/summary.md`). For comparison, `run --path rpc` sends the same
bursts through an RPC node's `sendTransaction` (no preflight, no retries), which forwards them itself; those bursts are
reported separately and never enter the verdicts.

**Analysis.** Only probes of one burst that landed in the same block are compared (probes in different slots are
ordered by the slot). For each pair the share that follows a rule is computed: the higher price first, the one sent
earlier first, and as a control the lower payer index first (which carries no information). 0.5 means no relation,
1.0 a strict rule. Intervals: 95% cluster bootstrap over bursts. Null model: a permutation test that shuffles the
positions within each block (every order equally likely), plus the equal-price bursts. The reading was fixed before
the data: a rule is claimed only when its permutation p-value is below 0.01 and its interval excludes 0.5, with at
least 30 pairs; otherwise the answer is "no detectable rule". The whole block serves as background: Kendall's τ of
position against reward per cost unit for every transaction.

### What the design accounts for

| Point of the task | How it is handled |
| --- | --- |
| Leader schedule and leader types | `getLeaderSchedule` per identity; client from gossip `clientId` and version at the time of each run (`run.json`, `data/snapshots/`); windows of 4 slots (8 or 12 when consecutive) |
| Path from sender to the executing node | Direct QUIC to the TPU in gossip, no RPC in between; for BAM and Harmonic that TPU is a shared sequencer/builder ingress, measured per client from the snapshots; unstaked QoS and late arrival are explicit loss sources, measured as landing rates |
| Interaction between our own transactions | One payer per probe, the same accounts and cost for all; a separate conflict variant with a shared writable account |
| Competition inside the block | Only the relative order of our probes is compared; others' transactions are background and a whole-block check |
| Send order | Random permutation independent of price; reverse variant; spacing 0 / 10 / 50 ms; fire before and inside the window |
| Noise against rule | Equal-price bursts, payer-index control, permutation null, bootstrap intervals; "no rule" is a valid outcome |

### What it cannot tell apart

- The scheduler's rule from the leader's parallel execution threads: the equal-price bursts show their combined noise,
  not the internals.
- What happens to a probe that missed its leader (there is no forwarding to follow).
- The internals of external sequencers (Jito BAM, Harmonic): only their result in the block is visible.
- The effect of the sender's stake: our connection is unstaked; staked senders may see a different picture.
- Leaders or load conditions with few measurements.

## Verify

Every probe's signature, slot and block index are in `data/runs/<run>/bursts.jsonl` and `results.jsonl`. Any of them
can be checked against the chain:

```bash
curl -s https://api.mainnet-beta.solana.com -H 'content-type: application/json' -d \
  '{"jsonrpc":"2.0","id":1,"method":"getBlock","params":[<slot>,{"encoding":"json","maxSupportedTransactionVersion":1,"transactionDetails":"signatures","rewards":false}]}'
```

The position of the signature in `result.signatures` is the block index the analysis uses.
`data/runs/<run>/blocks/<slot>.json.gz` keeps each block in compact form (signature, fee, compute units, cost units,
compute budget per transaction).

## Results

See [report/REPORT.md](report/REPORT.md) and [report/generated/summary.md](report/generated/summary.md).

## Development

```bash
cargo build --release --manifest-path probe/Cargo.toml && cargo test --manifest-path probe/Cargo.toml
docker build -t solana-order-probe/analyze analyze && docker run --rm --entrypoint python -w /app solana-order-probe/analyze -m pytest -q tests
```

The analysis tests generate synthetic runs in the probe's file format with leaders that follow known rules (strict
fee, fee then arrival, FIFO, random, by account) and check that each is recovered.

## License

MIT
