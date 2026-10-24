# Report

Status (October 24, 2026): the pilot and two campaign sessions have run on mainnet, one at midday and one at night
(12 validators on 8 clients, 7 200 probes, 0.0377 SOL of fees in all). Results below; every number is recomputed from
`data/` by `order_analysis` (`generated/summary.md`, `generated/*.csv`).

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
| 2026-10-23 13:20 | Live run started by mistake during a code check (4 bursts, 48 probes, 258 160 lamports); records lost, fees and signatures recovered from the chain; not analysed (see What went wrong) | `data/runs/20261023T132013Z/reconstructed.jsonl` |
| 2026-10-24 02:30 | Funding top-up of the 12 payers (2 transactions, 10 000 lamports of fees) | `data/spend_ledger.jsonl` |
| 2026-10-24 02:30 | Cluster snapshot before session 2 | `data/snapshots/20261024T023013Z` |
| 2026-10-24 02:30–05:12 | **Live campaign, session 2**: the same 12 leaders and design as session 1, at night (05:30–08:12 in Kyiv) | `data/runs/20261024T023015Z` |
| 2026-10-24 02:30–05:14 | Slot stream recorded alongside session 2 with the leader of every slot: calibration, and the window start of bursts whose run clock missed it | `data/calibration/20261024T023015Z` |
| 2026-10-24 05:12–05:14 | Reconciliation of session 2 (66 probes not landed at collection, none landed later); spend ledger checked against the 13 addresses' balances | `data/runs/20261024T023015Z/reconciled.jsonl`, `data/spend_ledger.jsonl` |

## Where leaders take transactions in

Gossip advertises one TPU address per node. For Jito-Solana, Agave, Firedancer and Rakurai it is the node's own host.
For the clients with an external sequencer or block builder it almost never is (snapshot `20261024T023013Z`, before
session 2):

| Client | Staked nodes | Stake | TPU hosts | TPU on the node's own host | Most nodes behind one TPU host |
| --- | --- | --- | --- | --- | --- |
| AgaveBam | 360 | 35.7% | 17 | 0 | 98 |
| JitoLabs | 147 | 22.9% | 147 | 136 | 1 |
| HarmonicAgave | 49 | 21.6% | 7 | 1 | 16 |
| HarmonicFiredancer | 37 | 8.7% | 6 | 0 | 20 |
| Firedancer | 24 | 7.0% | 24 | 23 | 1 |
| FireBAM | 14 | 1.6% | 8 | 0 | 4 |
| Rakurai | 9 | 1.2% | 9 | 9 | 1 |
| Agave | 20 | 1.1% | 20 | 20 | 1 |

Some hosts serve both families of one sequencer: `198.13.141.173` takes transactions for 98 AgaveBam and 4 FireBAM
nodes, `198.13.140.105` for 16 HarmonicAgave and 20 HarmonicFiredancer nodes. The snapshot of October 23 09:00, before
session 1, has the same structure (361 AgaveBam nodes behind 17 hosts, up to 96 behind one; no HarmonicAgave node on
its own host); overnight 12 nodes switched from HarmonicAgave to HarmonicFiredancer. The table is recomputed by
`order_analysis` from the saved gossip (`report/generated/tpu_ingress.csv`).

A probe sent "to the leader" of these clients first reaches that shared ingress, so the arrival order the design
controls is the order at the ingress, and the measured rule is the ingress's and the leader's together. Session 2
shows more of what that means for a sender. 46 of its probes sent to the FireBAM and HarmonicFiredancer leaders
landed in blocks of other leaders behind the same ingress, and none in a block of any other leader:

- A burst sent on time to the HarmonicFiredancer leader (`198.13.140.105:12349`), 250 ms before its window, landed
  whole in the block of the HarmonicAgave leader building just before it, on the same host. In two bursts sent 50 ms
  apart before the window, to the HarmonicFiredancer and the FireBAM leader, 2 probes each did the same.
- In 3 bursts sent 50 ms apart from 100 ms after the window's start, 2 probes each landed with the ingress's next
  leaders, 4 and 12 slots later.
- Two bursts went out seconds late because of a slow RPC answer (What went wrong). The one to the FireBAM leader, 9.4 s
  after its window started, landed whole 52 slots later with an AgaveBam leader that advertises the very same address
  (`198.13.141.173:5010`). The one to the HarmonicFiredancer leader, 4.0 s late, landed whole 48 slots later with the
  HarmonicAgave leader this study also measures (`Fd7b…69Nk`). Two other late bursts, sent to the Rakurai and an
  Agave leader on their own hosts, did not land at all.

The ingress hands a transaction to whichever of its leaders is building, or keeps it for the next one; the two
validators' own TPUs dropped it once the window had passed. The 16 probes of session 1 outside their window, all sent
to the HarmonicAgave leader, also landed with other leaders behind `198.13.140.105`. The verdicts count only the
target's own blocks.

## Slot timing under Alpenglow

From the RPC node's first-shred timestamps over 30 minutes on October 23 (8 476 slots, 28 skipped; calibration
`20261023T090046Z`) and over the 2 h 44 min of session 2 at night (45 284 slots, 1 075 skipped; calibration
`20261024T023015Z`):

| Slot position in the leader window | Median, Oct 23 | p10–p90 | Median, Oct 24 | p10–p90 |
| --- | --- | --- | --- | --- |
| 0 (hand-over from the previous leader) | 267 ms | 237–386 ms | 265 ms | 239–382 ms |
| 1 | 149 ms | 115–179 ms | 149 ms | 142–177 ms |
| 2 | 200 ms | 197–212 ms | 200 ms | 197–213 ms |
| 3 | 200 ms | 197–212 ms | 200 ms | 198–213 ms |

A window lasts about 815 ms. The public WebSocket delivers notifications in batches: from the node's first shred to
this machine took a median 219 ms and 2.3 s at p90 on October 23, 284 ms and 3.9 s at night, so arrival times are a
poor clock, while the node's timestamps are regular. Predicting a window's first shred 100–300 ms ahead from the node's
timestamps (latest known slot plus the median duration of each position) erred by 61–66 ms on average on October 23
(median −7 to −10 ms, p10 −143 to −155 ms, p90 +39 to +41 ms; 2 069 window starts) and by 76–81 ms at night (median
−14 to −18 ms, p10 −180 to −194 ms, p90 +32 ms; 11 261 window starts). From arrival times the best method erred by
236–276 ms and 158–195 ms; the first version of the clock used it. The comparison of ten methods is in
`data/calibration/*/summary.json` and is recomputed with `order-probe calibrate --from data/calibration/<id>`; the
5-minute calibration of 08:20 gives the same picture (49–53 ms on average). The night was rougher in other ways too:
2.3% of slots were skipped (0.3% on October 23), and the endpoint closed or stalled the WebSocket 33 times in
2 h 44 min.

The node's clock runs ahead of ours: by 53 ms in the 08:20 recording and 115 ms in the 09:00 one, probably another
node behind the same endpoint. Within one connection the offset drifts by about 1 ms a minute, and in some
minutes every notification is late, so the clock takes the offset as the lowest 1% of the delivery delays over the
last ten minutes and learns it afresh after every reconnection.

### Window start by client of the leader

The 2 h 44 min recorded alongside session 2, with the leader of every slot (median [p10, p90], ms; hand-over = first
shred of the leader's first slot minus that of the slot before; window = first shred of its last slot minus its
first; clock error = the prediction of the window start 100 ms ahead minus the observed start):

| Client | Windows | Skipped | Hand-over in | Window | Clock error |
| --- | --- | --- | --- | --- | --- |
| AgaveBam | 4 114 | 83 | 279 [244, 396] | 546 [542, 549] | −48 [−193, 28] |
| JitoLabs | 2 671 | 63 | 266 [238, 373] | 547 [502, 628] | −17 [−185, 31] |
| HarmonicAgave | 2 531 | 66 | 261 [242, 326] | 564 [552, 592] | −1 [−163, 31] |
| HarmonicFiredancer | 986 | 18 | 252 [232, 315] | 550 [548, 555] | 6 [−147, 40] |
| Firedancer | 838 | 22 | 257 [235, 319] | 601 [547, 739] | −2 [−188, 39] |
| FireBAM | 191 | 4 | 250 [230, 316] | 549 [549, 551] | 8 [−153, 38] |
| Rakurai | 126 | 0 | 258 [230, 292] | 548 [544, 550] | 4 [−127, 34] |
| Agave | 112 | 1 | 282 [264, 391] | 553 [544, 569] | −26 [−186, 19] |

AgaveBam leaders take the longest to start their window after the previous leader (median 279 ms, p90 396 ms), so the
clock fires early for them by 48 ms in the median. Firedancer leaders produce longer windows (601 ms, p90 739 ms
against ~547 ms for most clients), and HarmonicAgave ones slightly longer (564 ms). The 30 minutes of October 23 give
the same picture (AgaveBam hand-over 289 ms and clock error −34 ms, Firedancer windows 601 ms, HarmonicAgave 566 ms).
The first shred reaches the RPC node after the leader started, so these are differences between clients as seen from
one node, not absolute times.

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

7 200 probes in 600 bursts; 6 779 landed, 6 714 of them in the target leader's window:

| Run | Bursts | Probes | Not taken by the TPU | Landed | In the window | Probe fees, lamports |
| --- | --- | --- | --- | --- | --- | --- |
| Pilot, Oct 23 10:09–10:20 | 24 | 288 | 12 | 276 | 276 | 1 529 400 |
| Session 1, Oct 23 10:20–12:51 | 288 | 3 456 | 312 | 3 113 | 3 096 | 17 164 716 |
| Session 2, Oct 24 02:30–05:12 | 288 | 3 456 | 3 | 3 390 | 3 342 | 18 707 160 |

Fees paid in all: 37 679 436 lamports (0.0377 SOL): the probes above, 258 160 for the unplanned run (What went wrong)
and 20 000 for two fundings. The spend ledger equals the chain: the 13 addresses (funder and 12 payers) received
0.1 SOL from the owner's wallet and 7 206 lamports from strangers, and hold 62 327 770 lamports now, a difference of
exactly the ledger's 37 679 436. The strangers' lamports came in three transfers to two payers during session 2 (1 842,
2 038 and 3 326 lamports at 02:44, 03:01 and 05:12, each from a different address that never had anything to do with
this project), most likely address-poisoning spam aimed at busy accounts.

- **Landing slot.** Of the probes fired 300 ms before the window that landed in it, 90% landed in the window's first
  slot (all 276 in the pilot, 89% in each session) and 8% in the second. Fired 100 ms after the start, they spread
  over the window: 21%, 57%, 14% and 9% in its four slots.
- **Fire timing.** Planned −300 ms: median −309 ms (p10 −472, p90 −270) in session 1, −331 ms (p10 −541, p90 −264) in
  session 2; planned +100 ms: median +98 ms (p10 −7, p90 +131) and +101 ms (p10 −33, p90 +146). At night the clock was
  less precise (above): the fire moment was within 100 ms of the plan for 69% and 82% of bursts against 81% and 88%.
  45 bursts of session 2 whose run clock missed the window's start (a stream reconnection, a skipped first slot) take
  it from the slot stream recorded alongside; 5 have none. Four bursts of session 2 went out 2.1–9.4 s after their
  window started, held up by a slow RPC answer (What went wrong); they are the outliers in
  `generated/figures/fire_timing.png`.
- **Connections.** In session 1, 312 probes were never accepted: the TPU closed our unstaked QUIC connection right
  after the handshake (`closed by peer: dropped`), mostly at the FireBAM leader, whose TPU is a BAM ingress shared
  with ~100 nodes (21 of its 24 bursts), and in 2 bursts of an AgaveBam leader and 3 of an Agave one. For session 2
  the connection was opened 700 ms before the fire moment instead of 2–3 s (What went wrong): no connection was
  refused, and the TPUs acknowledged 3 453 of 3 456 probes. The FireBAM ingress dropped 8 connections in the middle
  of a burst; they were reopened and 3 probes were lost. The FireBAM leader now has 271 landings in 24 bursts and a
  verdict.
- **Outside the window.** 17 probes in session 1 and 48 in session 2 landed outside their target's window: 62 sent to
  BAM and Harmonic leaders went to other leaders behind the same ingress (above), and 3 landed in a later window of
  the same leader, one of them 92 slots late and found by the reconciliation. None of them enters the verdicts.
- **A burst that vanished.** One burst of session 2 (Agave `5Us1…5nBS`, equal prices, fired 100 ms after the start,
  50 ms apart) was acknowledged by the leader's TPU probe by probe, and the leader produced all 4 blocks of its window
  (252–285 transactions each), yet none of its 12 probes landed and none was charged. What the leader did with them
  is not visible from outside (README, What it cannot tell apart).
- **Lost positions.** 21 landed probes (13 in session 1, 8 in session 2) have no position: the public RPC refused
  their 6 blocks after retries (rate limit). Their fees came from `getTransaction`; they drop out of the comparisons.

### Verdict per validator

Pooled over the pilot and both sessions. Shares of within-block probe pairs that follow each rule, with 95%
cluster-bootstrap intervals; a rule is claimed when p < 0.01 and the interval excludes 0.5, from at least 30 pairs in
5 bursts. "Then arrival" means that, besides the fee order in the price-ladder bursts, the equal-price bursts put the
probe sent earlier first.

| Client | Validator | Verdict | Higher price first | Sent earlier first (equal price) |
| --- | --- | --- | --- | --- |
| Agave | `5Us1…5nBS` | priority fee, then arrival | 0.66 [0.57, 0.74] | 0.72 [0.57, 0.86] |
| Agave | `7QQG…wLpE` | priority fee, then arrival | 0.69 [0.60, 0.77] | 0.71 [0.57, 0.85] |
| AgaveBam | `5Cch…q9Ux` | priority fee (not strict) | 0.82 [0.75, 0.88] | 0.63 [0.49, 0.78] |
| AgaveBam | `DRpb…21hy` | priority fee (not strict) | 0.85 [0.78, 0.90] | 0.63 [0.50, 0.79] |
| FireBAM | `9jxg…nGFP` | priority fee (not strict) | 0.87 [0.77, 0.94] | 0.61 [0.51, 0.78] |
| Firedancer | `9eGr…8FoY` | priority fee (not strict) | 0.80 [0.72, 0.87] | 0.34 [0.19, 0.54] |
| Firedancer | `EvnR…qDo4` | priority fee (not strict) | 0.89 [0.82, 0.96] | 0.33 [0.15, 0.51] |
| HarmonicAgave | `Fd7b…69Nk` | priority fee, then arrival | 0.63 [0.58, 0.68] | 0.63 [0.54, 0.74] |
| HarmonicFiredancer | `JUPi…1h4b` | priority fee, then arrival | 0.61 [0.52, 0.68] | 0.63 [0.51, 0.78] |
| JitoLabs | `E1r4…dxHL` | priority fee, then arrival | 0.64 [0.59, 0.69] | 0.69 [0.58, 0.83] |
| JitoLabs | `HEL1…e2TU` | priority fee (not strict) | 0.63 [0.57, 0.69] | 0.56 [0.46, 0.69] |
| Rakurai | `ana2…xcfY` | priority fee, then arrival | 0.65 [0.57, 0.71] | 0.64 [0.51, 0.78] |

Pooled per client, the Firedancer leaders' equal-price share is 0.33 [0.21, 0.47] (p = 0.0005, 26 bursts), so at the
client level the rule is claimed as "priority fee, then last arrival first" (`generated/summary.md`, per client).

The payer-index control stays near chance (0.49–0.58). For one validator, the Firedancer leader `EvnR…qDo4`, it
reaches 0.58 [0.52, 0.65] at p = 0.0085, the one control of twelve that would pass the rule; with twelve controls a
result like that turns up by chance about once in nine studies. No fixed property of the probes orders the
equal-price pairs of any leader either: ordering them by the payer's key or by the signature, compared as bytes,
matches the block order in 0.43–0.59 of pairs. So no leader ordered the probes by their accounts. Full tables with
p-values and pair counts: `generated/summary.md`.

### The two sessions agree on the numbers more than on the labels

The same rule applied to each session alone (higher price first / sent earlier first at equal price;
`generated/by_session_validator.csv`):

| Client | Validator | Session 1, midday | Session 2, night |
| --- | --- | --- | --- |
| Agave | `5Us1…5nBS` | priority fee, then arrival; 0.67 / 0.73 | priority fee, then arrival; 0.64 / 0.70 |
| Agave | `7QQG…wLpE` | arrival order (FIFO); 0.63 / 0.67 | priority fee (not strict); 0.75 / 0.74 |
| AgaveBam | `5Cch…q9Ux` | priority fee (not strict); 0.83 / 0.60 | priority fee (not strict); 0.81 / 0.65 |
| AgaveBam | `DRpb…21hy` | priority fee (not strict); 0.84 / 0.66 | priority fee (not strict); 0.85 / 0.60 |
| FireBAM | `9jxg…nGFP` | insufficient data | priority fee (not strict); 0.90 / 0.59 |
| Firedancer | `9eGr…8FoY` | priority fee (not strict); 0.77 / 0.35 | priority fee (not strict); 0.84 / 0.33 |
| Firedancer | `EvnR…qDo4` | priority fee (not strict); 0.88 / 0.39 | priority fee (not strict); 0.86 / 0.36 |
| HarmonicAgave | `Fd7b…69Nk` | no detectable rule; 0.55 / 0.61 | priority fee, then arrival; 0.63 / 0.65 |
| HarmonicFiredancer | `JUPi…1h4b` | priority fee, then arrival; 0.65 / 0.66 | no detectable rule; 0.56 / 0.60 |
| JitoLabs | `E1r4…dxHL` | priority fee, then arrival; 0.61 / 0.72 | priority fee (not strict); 0.67 / 0.68 |
| JitoLabs | `HEL1…e2TU` | no detectable rule; 0.58 / 0.63 | priority fee (not strict); 0.62 / 0.52 |
| Rakurai | `ana2…xcfY` | priority fee (not strict); 0.64 / 0.65 | priority fee (not strict); 0.65 / 0.63 |

Of the 11 validators measured in both sessions, 10 kept their fee share within 0.09 and their equal-price share within
0.07. The label changed for 5 of them anyway: one session gives each validator 12 ladder bursts and 6 equal-price
bursts, so an interval spans ±0.1–0.15 and a share of 0.6–0.7 falls on either side of the line from one session to the
next. So close to the line, a label can even change with the bootstrap's random draws: when the session 2 bursts that
landed with other leaders were taken out (What went wrong), the data of the AgaveBam leader `DRpb…21hy` in session 1
did not change, yet its label went from "then arrival" to "not strict". The pooled verdicts rest on both sessions.
What every session shows without exception:

- The fee order is strictest for Firedancer, FireBAM and AgaveBam (0.77–0.90) and looser for Agave, JitoLabs,
  HarmonicAgave, Rakurai and HarmonicFiredancer (0.55–0.75).
- At equal price the plain Firedancer leaders put the later probe first (0.33–0.39 in all four readings), and the
  Agave-based ones the earlier (0.60–0.74, except one JitoLabs leader at night, 0.52).

The largest changes: an Agave leader from 0.63 to 0.75, the HarmonicAgave one from 0.55 to 0.63, and the
HarmonicFiredancer one from 0.65 to 0.56, while its blocks grew from a median 321 to 480 transactions. Block loads by
session are in `generated/by_session.csv`.

### The rule depends on when the transactions arrive

Higher price first, by firing condition (main and conflict bursts, pooled per client; pairs in brackets):

| Client | −300 ms, 0 ms apart | −300 ms, 10 ms apart | −300 ms, 50 ms apart | +100 ms, 0 ms apart | +100 ms, 10 ms apart | +100 ms, 50 ms apart |
| --- | --- | --- | --- | --- | --- | --- |
| Agave | 0.83 (528) | 0.80 (528) | 0.54 (226) | 0.60 (462) | 0.52 (459) | 0.53 (106) |
| AgaveBam | 0.93 (528) | 0.95 (528) | 0.75 (273) | 0.88 (528) | 0.64 (357) | 0.56 (140) |
| FireBAM | 0.96 (132) | 0.90 (132) | 0.58 (12) | 0.92 (132) | 0.77 (166) | 0.74 (31) |
| Firedancer | 0.95 (660) | 0.98 (660) | 0.73 (351) | 0.71 (528) | 0.86 (405) | 0.59 (138) |
| HarmonicAgave | 0.68 (396) | 0.69 (396) | 0.54 (112) | 0.67 (198) | 0.48 (233) | 0.54 (57) |
| HarmonicFiredancer | 0.87 (198) | 0.58 (264) | 0.62 (113) | 0.48 (264) | 0.62 (253) | 0.38 (72) |
| JitoLabs | 0.71 (660) | 0.72 (660) | 0.56 (250) | 0.57 (528) | 0.54 (490) | 0.62 (110) |
| Rakurai | 0.70 (264) | 0.74 (264) | 0.68 (168) | 0.54 (264) | 0.62 (253) | 0.49 (63) |

- **When the whole burst is at the leader before its window** (−300 ms, sent back to back or 10 ms apart), the fee
  decides most of the order: Firedancer 0.95–0.98, AgaveBam 0.93–0.95, FireBAM 0.90–0.96, Agave 0.80–0.83, JitoLabs
  0.71–0.72, Rakurai 0.70–0.74, HarmonicAgave 0.68–0.69, HarmonicFiredancer 0.58–0.87. The Firedancer, AgaveBam and
  FireBAM leaders come close to a strict fee order.
- **When the probes arrive while the leader builds** (+100 ms) or spread over 550 ms (50 ms apart), arrival competes
  with the fee. The share falls to 0.38–0.68 for the Agave, JitoLabs, Rakurai and Harmonic leaders, and to 0.56–0.92
  for the AgaveBam, FireBAM and Firedancer ones. In the reverse bursts (cheapest sent first) arrival won more often
  than the fee for the Agave (fee share 0.44, 0.46), HarmonicAgave (0.40), JitoLabs (0.42, 0.49) and Rakurai (0.42)
  leaders. It was close to even for the HarmonicFiredancer one (0.54). The fee won for the AgaveBam (0.66–0.70),
  FireBAM (0.68) and Firedancer (0.72–0.81) leaders.
- **At equal price** the leaders built on Agave (Agave, AgaveBam, HarmonicAgave, JitoLabs, Rakurai) put the probe sent
  earlier first more often than not (0.56–0.72); the rule holds for 5 of these 8. The plain Firedancer leaders put
  the later one first (0.34 and 0.33; claimed for the client, above). The two Firedancer leaders behind an external
  sequencer side with the Agave ones: HarmonicFiredancer 0.63 (claimed) and FireBAM 0.61 (p = 0.024, not claimed). For
  them, the equal-price order seems to be set by the sequencer in front, not by the leader's own scheduler.

### Conflicting transactions follow the fee more strictly

In the conflict bursts, pairs of probes that write-lock the same account were ordered by price more often than the
independent pairs of the same bursts, for every validator: Agave 0.87 / 0.83 against 0.68 / 0.70, AgaveBam
0.89 / 0.86 against 0.77 / 0.79, FireBAM 0.97 against 0.96, Firedancer 0.91 / 0.92 against 0.69 / 0.89, HarmonicAgave
0.73 against 0.61, HarmonicFiredancer 0.76 against 0.65, JitoLabs 0.76 / 0.79 against 0.63 / 0.62, Rakurai 0.75
against 0.64 (`generated/validators.csv`). Transactions that must run one after the other are queued by priority;
independent ones go to parallel execution threads, whose interleaving in the block blurs the fee order.

### The whole block

Kendall's τ of position against reward per cost unit over all transactions in the target leaders' blocks is weak:
median 0.02–0.10 per validator, 0.28 for one Firedancer leader (0.26–0.28 in each of its three runs). Each validator's
median moved by at most 0.03 between the sessions. Other senders' transactions differ in accounts, size, bundles and
builders, so the block as a whole shows little of the rule that same-shape probes reveal; the probes are the evidence,
the block is background.

### What these results do not show

Two sessions of 2.5 and 2.7 hours, at midday (October 23, 10:20–12:51 UTC) and at night (October 24, 02:30–05:12 UTC),
from one unstaked machine. Other days, staked senders and other leaders of the same clients may differ. Each client is
represented by one or two leaders, and the FireBAM one has usable data from session 2 only. The sessions differ in
detail (above), and so did the pilot: in the cleanest condition it gave a fee share of 0.80 for the JitoLabs leader,
0.78 for HarmonicAgave and 1.00 for Firedancer, against 0.63, 0.59 and 0.95 in session 1 and 0.76, 0.69 and 0.97 in
session 2 (`generated/by_session.csv`). The verdicts describe the result in the block. For BAM and Harmonic leaders
that result is the external sequencer's and the leader's together, and a probe sent to one of them can end up in
another leader's block.

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
- **A live run was started by mistake.** At 13:20 UTC, while checking the new guard that payers must afford a whole
  run, the check itself was run with `--live` and a scratch data directory, expecting the guard to refuse it. The
  payers could afford it, so 4 bursts (48 probes, all landed, 258 160 lamports) went to two leaders without being
  planned, and their records were deleted with the scratch directory. The fees went into that directory's own spend
  ledger, out of sight of the cap. Recovered: the 48 signatures and fees from the payers' on-chain history (memo tags
  `sop:20261023T132013Z:*`), now in the spend ledger and in `data/runs/20261023T132013Z/reconstructed.jsonl`; the
  ledger again equals the chain (18 962 276 lamports spent of the 0.1 SOL deposited). Fixed: the ledger now belongs to
  the keys (`secrets/spend_ledger.jsonl`, merged with the published copy), so another data directory cannot reset the
  count; a live run prints its plan and waits 10 s; checks are never run with `--live`. These bursts are not analysed.
- **Unstaked connections were evicted.** In session 1 the TPU closed 26 connections right after the handshake
  (`closed by peer: dropped`), losing 312 probes, mostly at the FireBAM leader whose TPU is a BAM ingress shared with
  ~100 nodes. The connection was opened 2–3 s before the fire moment and sat idle. For session 2 it was opened 700 ms
  before and reopened if dropped: no connection was refused, 8 dropped mid-burst at the FireBAM ingress were reopened,
  and 3 probes were lost instead of 312.
- **Four bursts went out seconds late.** In session 2 four bursts went out 1.8–9.7 s after the moment the clock had
  set for them, 2.1–9.4 s after their window started. The blockhash for a burst is fetched 2.5 s before the window,
  and the public RPC answers a rate-limited request only after retries with a doubling pause; on these bursts no QUIC
  connection dropped and every handshake took 21–59 ms, so that request is the step that held them up, and once late,
  the burst went out at once. Two of them, sent to validators' own TPUs, did not land; the other two landed with
  another leader behind the same ingress (above). Fixed after the session: a burst that would go out more than
  250 ms after its planned moment is skipped and its window left (checked in a mainnet dry-run, where on-time bursts
  go out as before).
- **Probes in another leader's block counted for the target.** The analysis compared every pair of probes of one
  burst in one block, without checking that the block was the target leader's. Found while writing this report,
  after session 2: 4 whole bursts and 7 pairs of probes in other leaders' blocks had been counted for the FireBAM,
  HarmonicAgave and HarmonicFiredancer leaders. Now only blocks of the target's window count (with a test). The
  HarmonicFiredancer leader's fee share went from 0.58 to 0.61 (0.52 to 0.56 in session 2) and its verdict from
  "arrival order (FIFO)" to "priority fee, then arrival"; FireBAM's from 0.88 to 0.87, HarmonicAgave's by less than
  0.01. In the table by firing condition two cells made up mostly of such bursts changed: FireBAM, −300 ms and 50 ms
  apart, from 0.94 to 0.58 (12 pairs left), and HarmonicFiredancer, −300 ms and back to back, from 0.70 to 0.87.
- **The night stream was rougher.** In session 2 the public WebSocket was closed or went silent 31 times on the run's
  own connection, and its delivery lagged more (p90 3.9 s). 50 bursts fired without the run clock seeing their
  window's first slot; the slot stream recorded alongside, added for this session, supplies the start for 45 of them.
  The fire moment was a little less precise than at midday (Delivery and cost).
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
