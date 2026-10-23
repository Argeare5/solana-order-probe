# Report

Status (October 23, 2026): the pilot and the first campaign session have run on mainnet (12 validators on 8 clients,
3 744 probes, 0.0187 SOL of fees). Results below; every number is recomputed from `data/` by `order_analysis`
(`generated/summary.md`, `generated/*.csv`).

## Measurements

All times UTC. Every measurement is after Alpenglow's activation at slot 459 653 000 on October 21, 2026, around
13:54 (slot 459 652 980 has block time 13:53:55, slot 459 653 040 13:54:10; slot 459 653 000 itself was skipped).
Checked on the chain: blocks 459 652 900 and 459 652 950 carry 664 of 953 and 649 of 915 vote transactions, blocks
459 653 100 and 459 653 200 carry 0 of 560 and 0 of 353. The first measurement here is at slot 460 36x xxx.

| Date | What | Data |
| --- | --- | --- |
| 2026-10-23 08:01 | Cluster snapshot: clients, stake, versions, TPU addresses; Alpenglow check (no vote transactions in sampled blocks) | `data/snapshots/20261023T080130Z` |
| 2026-10-23 08:20 | Slot clock calibration, 5 min of `slotSubscribe` + `slotsUpdatesSubscribe` from the public mainnet WebSocket | `data/calibration/20261023T082010Z` |
| 2026-10-23 08:30 | Dry-run, 32 bursts into 4 leaders on 4 clients (nothing sent) | `data/runs/20261023T083007Z` |
| 2026-10-23 08:41 | Dry-run from `docker compose up`, 3 bursts on 3 clients (nothing sent) | `data/runs/20261023T084133Z` |
| 2026-10-23 09:00 | Cluster snapshot for the campaign; QUIC handshake with the 12 chosen leaders (all accept an unstaked connection) | `data/snapshots/20261023T090046Z` |
| 2026-10-23 09:00–09:31 | Slot clock calibration, 30 min, with the leader of every slot (`getSlotLeaders`) | `data/calibration/20261023T090046Z` |
| 2026-10-23 09:43 | Cluster snapshot before the first live run | `data/snapshots/20261023T094303Z` |
| 2026-10-23 10:05 | Funding of the 12 payers (2 transactions, 10 000 lamports of fees) | `data/spend_ledger.jsonl` |
| 2026-10-23 10:09–10:20 | **Live pilot**: 3 leaders on 3 clients (JitoLabs, HarmonicAgave, Firedancer) × 8 bursts, fired 300 ms before the window, 0 and 10 ms spacing | `data/runs/20261023T100857Z` |
| 2026-10-23 10:20–12:51 | **Live campaign, session 1**: 12 leaders on 8 clients × 24 bursts, every combination of 4 variants, fire 300 ms before or 100 ms after the window start, and 0, 10 or 50 ms between sends | `data/runs/20261023T102008Z` |
| 2026-10-23 13:00–13:08 | Reconciliation of all live probes with the chain (1 late landing, 5 180 lamports added); spend ledger checked against the 13 addresses' balances | `data/runs/*/reconciled.jsonl`, `data/spend_ledger.jsonl` |

## Where leaders take transactions in

Gossip advertises one TPU address per node. For Jito-Solana, Agave, Firedancer and Rakurai it is the node's own host.
For the clients with an external sequencer or block builder it never is (snapshot `20261023T090046Z`):

| Client | Staked nodes | Stake | TPU hosts | TPU on the node's own host | Most nodes behind one TPU host |
| --- | --- | --- | --- | --- | --- |
| AgaveBam | 361 | 36.3% | 17 | 0 | 96 |
| HarmonicAgave | 59 | 23.3% | 7 | 0 | 21 |
| JitoLabs | 147 | 23.2% | 147 | 136 | 1 |
| Firedancer | 19 | 6.6% | 19 | 18 | 1 |
| HarmonicFiredancer | 26 | 6.5% | 6 | 0 | 15 |
| FireBAM | 13 | 1.6% | 7 | 0 | 4 |
| Rakurai | 9 | 1.2% | 9 | 9 | 1 |
| Agave | 20 | 1.1% | 20 | 20 | 1 |

Some hosts serve both families of one sequencer: `198.13.141.173` takes transactions for 96 AgaveBam and 3 FireBAM
nodes, `198.13.140.105` for 21 HarmonicAgave and 15 HarmonicFiredancer nodes. A probe sent "to the leader" of these
clients first reaches that shared ingress, so the arrival order the design controls is the order at the ingress, and
the measured rule is the ingress's and the leader's together. The table is recomputed by `order_analysis` from the
saved gossip (`report/generated/tpu_ingress.csv`).

## Slot timing under Alpenglow

From the RPC node's first-shred timestamps over 30 minutes (8 476 slots, 28 skipped; calibration `20261023T090046Z`):

| Slot position in the leader window | Median duration | p10–p90 |
| --- | --- | --- |
| 0 (hand-over from the previous leader) | 267 ms | 237–386 ms |
| 1 | 149 ms | 115–179 ms |
| 2 | 200 ms | 197–212 ms |
| 3 | 200 ms | 197–212 ms |

A window lasts about 815 ms. The public WebSocket delivers notifications in batches: from the node's first shred to
this machine took a median 219 ms and 2.3 s at p90, so arrival times are a poor clock, while the node's timestamps
are regular. Predicting a window's first shred 100–300 ms ahead from the node's timestamps (latest known slot plus the
median duration of each position) errs by 61–66 ms on average (median −7 to −10 ms, p10 −143 to −155 ms, p90 +39 to
+41 ms; 2 069 window starts). From arrival times the best method errs by 236–276 ms on average, the method the first
version used. The comparison of ten methods is in `data/calibration/*/summary.json` and is recomputed with
`order-probe calibrate --from data/calibration/<id>`; the 5-minute calibration of 08:20 gives the same picture
(49–53 ms on average).

The node's clock runs ahead of ours: by 53 ms in the 08:20 recording and 115 ms in the 09:00 one, probably another
node behind the same endpoint. Within one connection the offset drifts by about 1 ms a minute, and in some
minutes every notification is late, so the clock takes the offset as the lowest 1% of the delivery delays over the
last ten minutes and learns it afresh after every reconnection.

### Window start by client of the leader

The same 30 minutes, with the leader of every slot (median [p10, p90], ms; hand-over = first shred of the leader's
first slot minus that of the slot before; window = first shred of its last slot minus its first; clock error = the
prediction of the window start 100 ms ahead minus the observed start):

| Client | Windows | Skipped | Hand-over in | Window | Clock error |
| --- | --- | --- | --- | --- | --- |
| AgaveBam | 771 | 1 | 289 [242, 401] | 546 [513, 549] | −34 [−160, 31] |
| HarmonicAgave | 511 | 3 | 261 [239, 327] | 566 [553, 594] | 4 [−132, 38] |
| JitoLabs | 488 | 1 | 266 [238, 370] | 547 [497, 554] | −8 [−137, 37] |
| Firedancer | 144 | 1 | 259 [224, 314] | 601 [547, 762] | −8 [−102, 46] |
| HarmonicFiredancer | 128 | 1 | 247 [227, 290] | 551 [549, 699] | 19 [−119, 67] |
| FireBAM | 38 | 0 | 244 [224, 312] | 549 [549, 551] | 17 [−163, 57] |
| Rakurai | 26 | 0 | 252 [224, 296] | 549 [516, 552] | 10 [−72, 38] |
| Agave | 15 | 0 | 283 [262, 336] | 554 [544, 570] | −12 [−49, 16] |

AgaveBam leaders take the longest to start their window after the previous leader (median 289 ms, p90 401 ms), so the
clock fires early for them by 34 ms in the median. Firedancer leaders produce longer windows (601 ms, p90 762 ms
against ~547 ms for most clients), and HarmonicAgave ones slightly longer (566 ms). The first shred reaches the RPC
node after the leader started, so these are differences between clients as seen from one node, not absolute times.

Fire timing in the dry-runs (34 bursts with an observed window start): the first probe went out within 50 ms of the
planned offset for 74% of bursts and within 100 ms for 91%. By client the deviation followed the table above: the
window started later than predicted for AgaveBam leaders (median deviation of the fire moment −73 ms in that small
sample) and earlier for Firedancer (+32 ms).

## Pipeline checks before mainnet

On a local validator in Docker (`solanalabs/solana:v1.18.26`, 2026-10-23 09:52–10:03), with the same binary and image:

- A live run of 8 bursts (96 probes): 89 landed, all in their window; every probe's QUIC stream was acknowledged by
  the TPU. The 7 that did not land were the last one or two sent in the bursts with no spacing (11th and 12th of 12
  sent back to back); with 10 ms between sends all 12 landed. An unstaked connection's streams beyond the TPU's rate
  are acknowledged and then dropped. The mainnet runs record the send order of every probe, so the same effect shows
  there as a landing rate by send order.
- A run killed right after its first burst, before collection: `reconcile` collected the burst afterwards (12 of 12
  landed, positions and fees recorded); a second `reconcile` changed nothing.
- Money: the 12 payers' balances fell by exactly the probe fees in the spend ledger, 556 135 lamports over 101 landed
  probes; `sweep --live --to` then left every payer and the funder at 0.

## Pilot (live, mainnet)

288 probes in 24 bursts: 276 landed, every one in its leader's window and every one in the window's first slot. One
burst (JitoLabs, equal prices, 10 ms spacing) did not land at all: the leader's TPU dropped the QUIC connection in the
middle of it (`tpu_ack`: "connection lost"). Unlike the local validator, mainnet leaders took all 12 probes sent back
to back: every send position landed 12 times out of 12. Fees: 1 529 400 lamports for 276 probes; the reconciliation
found nothing missing. The pilot was a check of the method; its bursts are part of the data, and its first readings
(fee order strict for the Firedancer leader, not strict for the JitoLabs and HarmonicAgave ones) wait for the
campaign's numbers.

## Results

### Delivery and cost

3 744 probes in 312 bursts (24 in the pilot, 288 in session 1); 3 389 landed, 3 372 of them in the target leader's
window. Fees
paid: 18 704 116 lamports (0.0187 SOL) in all, funding and pilot included. The spend ledger equals the chain: the 13
addresses (funder and 12 payers) received 0.1 SOL and hold 81 295 884 lamports now, a difference of exactly the
ledger's 18 704 116.

- A burst fired 300 ms before the window landed whole in the window's first slot; the planned fire moment was kept
  closely (planned −300 ms: median −308 ms, p10 −454, p90 −269; planned +100 ms: median +98 ms, p10 −7, p90 +131).
- In session 1, 312 probes were never accepted: the TPU closed our unstaked QUIC connection right after the handshake
  (`closed by peer: dropped`). This hit the FireBAM leader in 21 of its 24 bursts (its TPU is a BAM ingress shared
  with AgaveBam nodes), an AgaveBam leader in 2 and an Agave leader in 3. The FireBAM leader therefore has too little
  data for a verdict.
- 16 HarmonicAgave probes landed outside the target's window: one whole burst two slots before it, in the previous
  leader's block, and 4 probes in the next leader's block. Harmonic's shared ingress hands transactions to the leader
  that is building, not to the one they were sent for.

### Verdict per validator

Shares of within-block probe pairs that follow each rule, with 95% cluster-bootstrap intervals; a rule is claimed when
p < 0.01 and the interval excludes 0.5, from at least 30 pairs in 5 bursts. "Then arrival" means that, besides the fee
order in the price-ladder bursts, the equal-price bursts put the probe sent earlier first.

| Client | Validator | Verdict | Higher price first | Sent earlier first (equal price) |
| --- | --- | --- | --- | --- |
| Agave | `5Us18…nBS` | priority fee, then arrival | 0.67 [0.53, 0.78] | 0.73 [0.52, 0.95] |
| Agave | `7QQG…wLpE` | arrival order (FIFO) | 0.63 [0.48, 0.77] | 0.67 [0.51, 0.85] |
| AgaveBam | `5Cch…q9Ux` | priority fee (not strict) | 0.83 [0.75, 0.89] | 0.60 [0.44, 0.85] |
| AgaveBam | `DRpb…21hy` | priority fee, then arrival | 0.84 [0.73, 0.93] | 0.66 [0.50, 0.87] |
| FireBAM | `9jxg…nGFP` | insufficient data | 1 burst landed with ≥ 2 probes | |
| Firedancer | `9eGr…8FoY` | priority fee (not strict) | 0.77 [0.63, 0.87] | 0.35 [0.13, 0.65] |
| Firedancer | `EvnR…qDo4` | priority fee (not strict) | 0.92 [0.82, 0.98] | 0.29 [0.11, 0.55] |
| HarmonicAgave | `Fd7b…69Nk` | priority fee (not strict) | 0.63 [0.56, 0.71] | 0.60 [0.51, 0.75] |
| HarmonicFiredancer | `JUPi…1h4b` | priority fee, then arrival | 0.65 [0.55, 0.74] | 0.66 [0.50, 0.85] |
| JitoLabs | `E1r4…dxHL` | priority fee, then arrival | 0.61 [0.56, 0.65] | 0.72 [0.60, 0.88] |
| JitoLabs | `HEL1…e2TU` | priority fee (not strict) | 0.64 [0.56, 0.73] | 0.59 [0.47, 0.77] |
| Rakurai | `ana2…xcfY` | priority fee (not strict) | 0.64 [0.53, 0.74] | 0.65 [0.48, 0.88] |

The payer-index control stays near chance for every validator (0.48–0.61; none passes the rule), so no leader ordered
the probes by their accounts. Full tables with p-values and pair counts: `generated/summary.md`.

### The rule depends on when the transactions arrive

Higher price first, by firing condition (main and conflict bursts, pooled per client; pairs in brackets):

| Client | −300 ms, 0 ms apart | −300 ms, 10 ms apart | −300 ms, 50 ms apart | +100 ms, 0 ms apart | +100 ms, 10 ms apart | +100 ms, 50 ms apart |
| --- | --- | --- | --- | --- | --- | --- |
| Agave | 0.83 (264) | 0.81 (264) | 0.44 (73) | 0.61 (264) | 0.40 (206) | 0.43 (46) |
| AgaveBam | 0.89 (264) | 0.94 (264) | 0.78 (122) | 0.92 (264) | 0.64 (171) | 0.54 (81) |
| Firedancer | 0.96 (396) | 0.97 (396) | 0.64 (160) | 0.72 (264) | 0.86 (200) | 0.56 (68) |
| HarmonicAgave | 0.66 (264) | 0.71 (264) | 0.45 (58) | 0.66 (132) | 0.42 (101) | 0.65 (26) |
| HarmonicFiredancer | 0.90 (132) | 0.62 (132) | 0.61 (70) | 0.58 (132) | 0.59 (132) | 0.32 (38) |
| JitoLabs | 0.68 (396) | 0.69 (396) | 0.57 (125) | 0.51 (264) | 0.59 (237) | 0.72 (61) |
| Rakurai | 0.71 (132) | 0.61 (132) | 0.67 (100) | 0.64 (132) | 0.61 (132) | 0.48 (27) |

- **When the whole burst is at the leader before its window** (−300 ms, sent back to back or 10 ms apart), the fee
  decides most of the order: Firedancer 0.96–0.97, AgaveBam 0.89–0.94, Agave 0.81–0.83, HarmonicFiredancer 0.90 and
  0.62, Rakurai 0.61–0.71, HarmonicAgave 0.66–0.71, JitoLabs 0.68–0.69. Only the Firedancer leaders come close to a
  strict fee order.
- **When the probes arrive while the leader builds** (+100 ms) or spread over 550 ms (50 ms apart), arrival competes
  with the fee and the share falls to 0.40–0.65 for most clients. In the reverse bursts (cheapest sent first) the
  arrival order won more often than the fee for the Agave, HarmonicAgave, HarmonicFiredancer and Rakurai leaders and
  one JitoLabs leader (fee share 0.37–0.48); the fee won for the other JitoLabs leader (0.60), the AgaveBam ones
  (0.63–0.66) and the Firedancer ones (0.71–0.81).
- **At equal price** the leaders built on Agave (Agave, AgaveBam, HarmonicAgave, JitoLabs, Rakurai) put the probe sent
  earlier first more often than not (0.59–0.73); the rule holds for 4 of these 8, and for the HarmonicFiredancer
  leader (0.66). The Firedancer leaders lean the other way (0.29–0.35, the last probe sent first), but their
  intervals reach 0.5, so that is not claimed.

### Conflicting transactions follow the fee more strictly

In the conflict bursts, pairs of probes that write-lock the same account were ordered by price more often than the
independent pairs of the same bursts, for every validator with data: Agave 0.86 / 0.82 against 0.65 / 0.71, AgaveBam
0.91 / 0.88 against 0.81 / 0.79, Firedancer 0.91 / 0.91 against 0.63 / 0.87, HarmonicAgave 0.81 against 0.58,
HarmonicFiredancer 0.82 against 0.68, JitoLabs 0.79 / 0.81 against 0.57 / 0.66, Rakurai 0.72 against 0.67
(`generated/validators.csv`). Transactions that must run one after the other are queued by priority; independent
ones go to parallel execution threads, whose interleaving in the block blurs the fee order.

### The whole block

Kendall's τ of position against reward per cost unit over all transactions in the target leaders' blocks is weak:
median 0.02–0.10 per validator, 0.28 for one Firedancer leader. Other senders' transactions differ in accounts, size,
bundles and builders, so the block as a whole shows little of the rule that same-shape probes reveal; the probes are
the evidence, the block is background.

### What these results do not show

One session of 2.5 hours on October 23 (10:20–12:51 UTC), from one unstaked machine: other times of day, staked senders and leaders with
more data may differ. The verdicts describe the result in the block; for BAM and Harmonic leaders that result is the
external sequencer's and the leader's together.

## What went wrong

- **The first slot clock fired 1.8–3.4 slots early.** It extrapolated from the arrival of the latest `slotSubscribe`
  notification once, 2.5 s ahead. The public WebSocket batches notifications, and under Alpenglow a window starts when
  the previous one ends rather than on a global timer. The clock was rebuilt on the node's first-shred timestamps and
  re-predicts until the fire moment (above).
- **Memo v2 would have failed every probe.** It logs the memo and checks signers for ~11 100 compute units, while the
  probes were limited to 1 400. `simulate` caught it before any live run; probes now use Memo v1 (356 CU, 806 CU for
  the whole probe, limit 900).
- **Price was tied to the payer.** Probe `i` always had payer `i` and price `ladder[i]`, so a leader ordering by
  account would have looked like one ordering by price. Prices are now dealt to payers at random every burst, and
  the payer index is a control in the analysis.
- **The public endpoint can switch nodes.** The 08:20 and 09:00 recordings saw node clocks 53 ms and 115 ms ahead of
  ours. A clock that kept one offset across a reconnection would fire up to ~60 ms off for a while; the offset is now
  tied to the connection.
- **A failed collection would have hidden fees.** In the first version a collection error (an RPC failure on
  `getBlock`, a probe landing after its burst was collected) left the probe's fee out of the spend ledger, and an
  error in the burst loop aborted the run before its last bursts were collected. Found in the code review before the
  first mainnet run; now fees come from `getTransaction` when a block cannot be fetched, the run always collects what
  it sent, and the reconciliation records any fee still missing (tested above).
- **A merely processed status was counted as landed.** A slot that is processed but later dropped with its fork
  would have recorded a landing that never happened; a probe now counts as landed only when its block is confirmed.
- **The final reconciliation of session 1 timed out.** It asked the public RPC for 256 signature statuses with history
  search in one request; for unknown signatures (mostly the FireBAM probes that never reached a leader) the endpoint
  timed out, and the run ended with an error after all 288 bursts had been sent and collected. Requests now carry 32
  signatures and a failed request leaves its probes for the next reconciliation; the rerun took 7.6 minutes and
  found one late landing.
- **The slot stream can stall without closing.** In four pilot bursts in a row the public WebSocket went silent, so
  the clock fired from stale points and the window's start was never observed (their fire timing is unknown; the
  probes still landed in the window). The stream is now reconnected after 3 s of silence.
- **The verdict rule needed a minimum number of bursts.** In the pilot one leader's equal-price share rested on a
  single burst, and the bootstrap over bursts gave it a zero-width interval. A verdict now needs at least 5 bursts
  besides 30 pairs; the rule was changed after the pilot and before the campaign.
- **No devnet SOL.** The devnet and testnet faucets answered every `requestAirdrop` with HTTP 429 or an internal
  error. The end-to-end live test ran on a local validator in Docker (`solanalabs/solana:v1.18.26`): 8 bursts,
  96 of 96 probes landed and were collected; funding, the spend ledger and the sweep matched the chain's fees.
