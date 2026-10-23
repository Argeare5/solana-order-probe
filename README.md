# solana-order-probe

How do Solana leaders order the transactions inside their blocks? This repository measures it on mainnet with its own
probe transactions: bursts of same-shape transactions from separate payers, whose compute unit prices are set by the
experiment, sent over QUIC to the TPU address each leader advertises, in a random order, and then located in the
block with `getBlock`.

- `probe/` — `order-probe`, a Rust binary: cluster snapshot, slot clock, probe bursts, block collection, spend cap,
  reconciliation with the chain.
- `analyze/` — `order_analysis`, a Python package: within-block metrics, null model, figures, tables.
- `data/` — every raw record: snapshots, calibrations, runs (bursts, signatures, slots, compact blocks), spend ledger.
- `report/` — `REPORT.md` (dated measurements, results, what went wrong) and `generated/` (tables and figures).

All measurements are taken after Alpenglow's activation on October 21, 2026 (slot 459 653 000): mainnet blocks just
before it carry ~650 vote transactions each, blocks after it none (`report/REPORT.md`).

**Status (October 23, 2026):** a pilot and the first campaign session have run on mainnet: 12 validators on 8 clients,
3 744 probes, 0.0187 SOL of fees, with the spend ledger matching the chain to the lamport.

## Run it

Requires Docker with Compose v2.

```bash
docker compose up
```

Without funds and without sending anything, this

- takes a **dry-run** against the current mainnet: finds the next leader windows of the targets (`TARGETS` in `.env`,
  or three default leaders), builds, signs and times one burst for each, opens the QUIC connection to the leader and
  sends nothing (the 12 payer keypairs it signs with are created, unfunded, in `./secrets` on first use);
- recomputes every table and figure in `report/generated` from the raw data in `data/`.

Real sends need a funded key in `./secrets`, the explicit `--live` flag, and stay under the spend cap (`SPEND_CAP_SOL`,
0.05 SOL by default):

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
| `calibrate` | Records the slot notifications and measures how well the slot clock predicts window starts, by client | no |
| `simulate` | Compute units a probe consumes, through `simulateTransaction` | no |
| `quic-ping` | QUIC handshake with a TPU | no |
| `keys` | Funder and payers with balances; `--new-funder` creates a funder key | no |
| `airdrop` | Asks the faucet for SOL (devnet, testnet or a local validator; refused on mainnet) | faucet request |
| `fund`, `sweep` | Top the payers up / return everything to the funder (`--to` sends it on) | only with `--live` |
| `run` | Bursts into the targets' leader windows, then collection and reconciliation | only with `--live` |
| `reconcile` | Checks every probe of the live runs against the chain and records any fee the ledger is missing | no |

**Spend cap.** Every fee paid (probes, funding, sweeping) is read back from the chain and appended to
`data/spend_ledger.jsonl`, once per signature. Before each burst its worst case is reserved, and a burst that could
pass the cap is refused. A burst whose collection fails, or a probe that lands after its burst was collected, is
picked up by the reconciliation that `run --live` performs before it sends anything and after its last probe can no
longer land, so the ledger holds every fee actually paid. On a local validator the payers' balances fell by exactly the
sum of the probe fees in the ledger (556 135 lamports over 101 landed probes, one run killed mid-way and reconciled).

## Design

**A burst** is 12 probes sent into one leader window (4 consecutive slots of one leader), back to back or 10 or 50 ms
apart. A probe is `SetComputeUnitLimit(900)`, `SetComputeUnitPrice(p)`, a 0-lamport transfer to a fresh address and a
Memo v1 tag `sop:<run>:<burst>:<i>`; it consumes 806 compute units on mainnet. Every probe has its own fee payer, so
probes never write-lock a common account (except in the conflict variant), and all have the same instructions,
number of accounts, size and limit, so the leader's cost model gives them the same cost and the reward per cost unit
is ordered exactly as the price.

**Factorial assignment.** In every burst the 12 prices of a ladder (1 000 … 5 000 000 µlamports/CU) are dealt to the
payers in a fresh random order, and the send order is an independent random permutation (seed in `run.json`).
Price, payer and send order are therefore independent, so a correlation of block position with one of them cannot be
an artefact of another.

**Variants**, cycled per validator together with the timing conditions below:

| Variant | Prices | Send order | Tells apart |
| --- | --- | --- | --- |
| main | ladder | random | fee rule vs arrival (FIFO) vs noise |
| null | all equal | random | arrival order alone; the noise floor of the block; payer-index control |
| reverse | ladder | cheapest first, most expensive last | fee against arrival head-on |
| conflict | ladder | random; half the probes write-lock one shared account | whether conflicting transactions still follow the fee |

**Timing.** Each burst fires at a planned offset from the predicted start of the window: −300 ms (the burst is at the
leader before its window starts) or +100 ms (probes arrive while the leader builds its block). The prediction uses
the RPC node's own first-shred timestamps (`slotsUpdatesSubscribe`) and each slot's duration by its position in the
window, re-predicted every 5 ms until the moment comes; every burst records when it really went out against the
window's observed first shred. In mainnet dry-runs on October 23 (34 bursts) the fire moment was within 50 ms of the
plan for 74% of bursts and within 100 ms for 91% (median deviation 33 ms, worst 362 ms when a hand-over was slow).

**Path.** Probes go over QUIC, configured as Agave's own `tpu-client-next` configures it, to the TPU address the
leader advertises in gossip; for every probe the run records whether the TPU acknowledged all of its bytes. Our
connection is unstaked, so stake-weighted QoS gives it a small share; probes that do not land are counted, not re-sent.
For Jito-Solana, Agave, Firedancer and Rakurai that TPU is the validator's own host. For the BAM and Harmonic clients
it never is: their nodes advertise a shared ingress of the external sequencer or block builder (on October 23, 361
AgaveBam nodes behind 17 TPU hosts, up to 96 behind one; 59 HarmonicAgave nodes behind 7). For those leaders the
arrival order we control is the order at that ingress, and the rule we measure is the ingress's and the leader's
together. For comparison, `run --path rpc` sends the same bursts through an RPC node's `sendTransaction` (no
preflight, no retries), which forwards them itself; those bursts are reported separately and never enter the
verdicts.

**Analysis.** Only probes of one burst that landed in the same block are compared (probes in different slots are
ordered by the slot). For each pair the share that follows a rule is computed: the higher price first, the one sent
earlier first, and as a control the lower payer index first (which carries no information). 0.5 means no relation,
1.0 a strict rule. Intervals: 95% cluster bootstrap over bursts. Null model: a permutation test that shuffles the
positions within each block (every order equally likely), plus the equal-price bursts. The reading was fixed before
the data: a rule is claimed only when its permutation p-value is below 0.01 and its interval excludes 0.5, with at
least 30 pairs from at least 5 bursts; otherwise the answer is "no detectable rule" (or "insufficient data"). The
5-burst minimum was added after the pilot and before the campaign: the pilot showed an interval resampled from a
single burst, which has zero width and means nothing. The fee share comes from the main and conflict bursts,
where price is independent of the send order; the reverse bursts are reported on their own. The whole block serves as
background: Kendall's τ of position against reward per cost unit for every transaction.

### What the design accounts for

| Point of the task | How it is handled |
| --- | --- |
| Leader schedule and leader types | `getLeaderSchedule` per identity; client from gossip `clientId` and version at the time of each run (`run.json`, `data/snapshots/`); windows of 4 slots (8 or 12 when consecutive); window hand-over measured per client (`calibrate`) |
| Path from sender to the executing node | QUIC to the TPU in gossip, no RPC in between; for BAM and Harmonic that TPU is a shared sequencer/builder ingress, measured per client from the snapshots; TPU acknowledgements recorded; unstaked QoS and late arrival are explicit loss sources, measured as landing rates; an RPC path for comparison |
| Interaction between our own transactions | One payer per probe, the same instructions and cost for all; a separate conflict variant with a shared writable account |
| Competition inside the block | Only the relative order of our probes is compared; others' transactions are background and a whole-block check |
| Send order | Random permutation independent of price; reverse variant; spacing 0 / 10 / 50 ms; fire before and inside the window |
| Noise against rule | Equal-price bursts, payer-index control, permutation null, bootstrap intervals; "no rule" is a valid outcome |

### What it cannot tell apart

- The scheduler's rule from the leader's parallel execution threads: the equal-price bursts show their combined noise,
  not the internals.
- Why a probe was dropped: a TPU acknowledges a QUIC stream before the leader decides what to do with it (on a local
  validator, the 11th and 12th of 12 streams sent back to back over an unstaked connection were acknowledged and never
  executed), so a missing probe is counted, not explained.
- What happens to a probe after its window: late landings are found by the reconciliation and counted, not traced.
- The internals of external sequencers (Jito BAM, Harmonic): only their result in the block is visible.
- The effect of the sender's stake: our connection is unstaked; staked senders may see a different picture.
- Leaders or load conditions with few measurements.

## Verify

Every probe's signature, slot and block index are in `data/runs/<run>/bursts.jsonl` and `results.jsonl` (and in
`reconciled.jsonl` for probes found after collection). Any of them can be checked against the chain:

```bash
curl -s https://api.mainnet-beta.solana.com -H 'content-type: application/json' -d \
  '{"jsonrpc":"2.0","id":1,"method":"getBlock","params":[<slot>,{"encoding":"json","maxSupportedTransactionVersion":1,"transactionDetails":"signatures","rewards":false}]}'
```

The position of the signature in `result.signatures` is the block index the analysis uses.
`data/runs/<run>/blocks/<slot>.json.gz` keeps each block in compact form (signature, fee, compute units, cost units,
compute budget per transaction).

## Results

From session 1 (October 23, 10:20–12:51 UTC; details, intervals and caveats in [report/REPORT.md](report/REPORT.md),
every table recomputed in [report/generated/summary.md](report/generated/summary.md)):

- Every client with enough data orders by priority fee more often than chance, and none strictly under all
  conditions. With the whole burst at the leader before its window, the share of probe pairs in fee order is
  0.96–0.97 for Firedancer, 0.89–0.94 for AgaveBam, 0.81–0.83 for Agave and 0.61–0.71 for JitoLabs, HarmonicAgave
  and Rakurai.
- When probes arrive while the leader builds, arrival competes with the fee (shares 0.40–0.65 for most clients); at
  equal price the Agave-based leaders tend to put the earlier probe first, the Firedancer ones the later.
- Probes that write-lock one account are ordered by fee more strictly than independent ones, for every validator
  with data.
- Paths differ by client: BAM and Harmonic leaders take transactions through shared ingresses; the FireBAM ingress
  dropped our unstaked connection in 21 of 24 bursts, and Harmonic's handed some probes to neighbouring leaders.

## Development

```bash
cargo test --manifest-path probe/Cargo.toml                                   # Rust 1.99.1, pinned in probe/rust-toolchain.toml
cargo clippy --manifest-path probe/Cargo.toml --all-targets -- -D warnings
docker build -t solana-order-probe/analyze analyze
docker run --rm --entrypoint python -w /app solana-order-probe/analyze -m pytest -q tests
```

The analysis tests generate synthetic runs in the probe's file format with leaders that follow known rules (strict
fee, fee then arrival, FIFO, random, by account) and check that each is recovered. CI runs all of the above and
`docker compose build` on every push (`.github/workflows/ci.yml`). Notes for coding agents and contributors are in
[AGENTS.md](AGENTS.md).

## License

[MIT](LICENSE)
