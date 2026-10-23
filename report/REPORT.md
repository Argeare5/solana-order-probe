# Report

Status: the measurement campaign on mainnet has not run yet. This report records the preparatory measurements and
what went wrong so far; results per validator and client will be added from `generated/` after the live runs.

## Measurements

All times UTC. Every measurement is after Alpenglow's activation (slot 459 653 000, October 21, 2026).

| Date | What | Data |
| --- | --- | --- |
| 2026-10-23 08:01 | Cluster snapshot: clients, stake, versions, TPU addresses; Alpenglow check (no vote transactions in sampled blocks) | `data/snapshots/20261023T080130Z` |
| 2026-10-23 08:20 | Slot clock calibration, 5 min of `slotSubscribe` + `slotsUpdatesSubscribe` from the public mainnet WebSocket | `data/calibration/20261023T082010Z` |
| 2026-10-23 08:30 | Dry-run, 32 bursts into 4 leaders on 4 clients (nothing sent) | `data/runs/20261023T083007Z` |
| 2026-10-23 08:41 | Dry-run from `docker compose up`, 3 bursts on 3 clients (nothing sent) | `data/runs/20261023T084133Z` |
| 2026-10-23 09:00 | Cluster snapshot for the campaign; QUIC handshake with the 12 chosen leaders (all accept an unstaked connection) | `data/snapshots/20261023T090046Z` |
| 2026-10-23 09:00–09:31 | Slot clock calibration, 30 min, with the leader of every slot (`getSlotLeaders`) | `data/calibration/20261023T090046Z` |

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
- **No devnet SOL.** The devnet and testnet faucets answered every `requestAirdrop` with HTTP 429 or an internal
  error. The end-to-end live test ran on a local validator in Docker (`solanalabs/solana:v1.18.26`): 8 bursts,
  96 of 96 probes landed and were collected; funding, the spend ledger and the sweep matched the chain's fees.
