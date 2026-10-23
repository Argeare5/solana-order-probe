# Results (generated)

Runs: 20261023T100857Z. Probes sent 288, landed 276.
Intervals: 95% cluster bootstrap over bursts (2000 draws); p: permutation test of block positions (2000 permutations), seed 20261023. A rule is claimed when p < 0.01 and the interval excludes 0.5, from at least 30 pairs in 5 bursts.

## Verdict per validator

| Client | Validator | Verdict | Higher price first (main, conflict) | Sent earlier first (equal price) | Payer-index control | Higher price first (reverse) |
| --- | --- | --- | --- | --- | --- | --- |
| Firedancer | `EvnRmnMrd69kFdbLMxWkTn1icZ7DCceRhvmb2SJXqDo4` | insufficient data | 1.00 [0.99, 1.00], p=0.0005, 264 pairs / 4 bursts | 0.08 [0.08, 0.09], p=0.0005, 132 pairs / 2 bursts | 0.51 [0.35, 0.67], p=0.965, 132 pairs / 2 bursts | 1.00 |
| HarmonicAgave | `Fd7btgySsrjuo25CJCj7oE7VPMyezDhnx7pZkj2v69Nk` | insufficient data | 0.78 [0.65, 0.86], p=0.0005, 264 pairs / 4 bursts | 0.60 [0.52, 0.69], p=0.234, 130 pairs / 2 bursts | 0.51 [0.50, 0.52], p=0.957, 132 pairs / 2 bursts | 0.53 |
| JitoLabs | `HEL1USMZKAL2odpNBj2oCjffnFGaYwmbGmyewGv1e2TU` | insufficient data | 0.80 [0.71, 0.89], p=0.0005, 264 pairs / 4 bursts | 0.40 [0.40, 0.40], p=0.458, 62 pairs / 1 bursts | 0.56 [0.56, 0.56], p=0.653, 66 pairs / 1 bursts | 0.78 |

## Per client (pooled)

| Client | Validators | Verdict | Higher price first | Sent earlier first (equal price) | Fee share: fired before / in window |
| --- | --- | --- | --- | --- | --- |
| Firedancer | 1 | insufficient data | 1.00 [0.99, 1.00], p=0.0005, 264 pairs / 4 bursts | 0.08 [0.08, 0.09], p=0.0005, 132 pairs / 2 bursts | 1.00 / – |
| HarmonicAgave | 1 | insufficient data | 0.78 [0.65, 0.86], p=0.0005, 264 pairs / 4 bursts | 0.60 [0.52, 0.69], p=0.232, 130 pairs / 2 bursts | 0.75 / – |
| JitoLabs | 1 | insufficient data | 0.80 [0.71, 0.89], p=0.0005, 264 pairs / 4 bursts | 0.40 [0.40, 0.40], p=0.449, 62 pairs / 1 bursts | 0.80 / – |

## Delivery

|                                                                   |   bursts |   sent |   send_errors |   tpu_acked |   landed |   in_window |   landed_after_collection |   failed |   comparable |
|:------------------------------------------------------------------|---------:|-------:|--------------:|------------:|---------:|------------:|--------------------------:|---------:|-------------:|
| ('Firedancer', 'EvnRmnMrd69kFdbLMxWkTn1icZ7DCceRhvmb2SJXqDo4')    |        8 |     96 |             0 |          96 |       96 |          96 |                         0 |        0 |           96 |
| ('HarmonicAgave', 'Fd7btgySsrjuo25CJCj7oE7VPMyezDhnx7pZkj2v69Nk') |        8 |     96 |             0 |          96 |       96 |          96 |                         0 |        0 |           96 |
| ('JitoLabs', 'HEL1USMZKAL2odpNBj2oCjffnFGaYwmbGmyewGv1e2TU')      |        8 |     96 |             8 |          84 |       84 |          84 |                         0 |        0 |           84 |

## Fire timing (first probe sent − window's first shred, ms)

|   offset_ms |   count |   mean |   std |   min |   10% |   50% |   90% |   max |
|------------:|--------:|-------:|------:|------:|------:|------:|------:|------:|
|        -300 |      20 |   -329 |    77 |  -556 |  -444 |  -295 |  -261 |  -252 |

## Whole blocks: Kendall τ of position against reward per cost unit

|                                                                   |   count |   50% |
|:------------------------------------------------------------------|--------:|------:|
| ('Firedancer', 'EvnRmnMrd69kFdbLMxWkTn1icZ7DCceRhvmb2SJXqDo4')    |    8.00 |  0.28 |
| ('HarmonicAgave', 'Fd7btgySsrjuo25CJCj7oE7VPMyezDhnx7pZkj2v69Nk') |    8.00 |  0.09 |
| ('JitoLabs', 'HEL1USMZKAL2odpNBj2oCjffnFGaYwmbGmyewGv1e2TU')      |    7.00 |  0.14 |

Figures: `figures/shares_by_validator.png`, `figures/position_vs_price.png`, `figures/position_vs_send_order.png`, `figures/fire_timing.png`.

## Where leaders take transactions in (snapshot 20261023T094303Z)

A TPU host shared by many nodes and different from their own address is the ingress of an external sequencer or block builder: probes sent to such a leader reach that ingress first.

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
| Raiku | 1 | 0.1% | 1 | 1 | 1 |
| Unknown(45933) | 1 | 0.1% | 1 | 1 | 1 |

## Slot duration by position in the leader window (calibration 20261023T090046Z)

| Position | Slots | Median ms | p10 | p90 |
| --- | --- | --- | --- | --- |
| 0 | 2117 | 267 | 237 | 386 |
| 1 | 2118 | 149 | 115 | 179 |
| 2 | 2117 | 200 | 197 | 212 |
| 3 | 2118 | 200 | 197 | 212 |

## Window start by client of the leader (calibration 20261023T090046Z, 30 min)

From the RPC node's first-shred timestamps: hand-over = first shred of the leader's first slot minus that of the slot before; window = first shred of its last slot minus its first; clock error = the slot clock's prediction of the window start 100 ms ahead minus the observed start (negative = the window started later than predicted). Median [p10, p90], ms.

| Client | Windows | Skipped | Hand-over in | Window | Clock error |
| --- | --- | --- | --- | --- | --- |
| AgaveBam | 771 | 1 | 289 [242, 401] | 546 [513, 549] | -34 [-160, 31] |
| HarmonicAgave | 511 | 3 | 261 [239, 327] | 566 [553, 594] | 4 [-132, 38] |
| JitoLabs | 488 | 1 | 266 [238, 370] | 547 [497, 554] | -8 [-137, 37] |
| Firedancer | 144 | 1 | 259 [224, 314] | 601 [547, 762] | -8 [-102, 46] |
| HarmonicFiredancer | 128 | 1 | 247 [227, 290] | 551 [549, 699] | 19 [-119, 67] |
| FireBAM | 38 | 0 | 244 [224, 312] | 549 [549, 551] | 17 [-163, 57] |
| Rakurai | 26 | 0 | 252 [224, 296] | 549 [516, 552] | 10 [-72, 38] |
| Agave | 15 | 0 | 283 [262, 336] | 554 [544, 570] | -12 [-49, 16] |
| Raiku | 2 | 0 | 244 [240, 248] | 556 [556, 557] | 18 [11, 26] |
| Unknown(17134) | 2 | 0 | 390 [382, 397] | 552 [545, 559] | -120 [-120, -120] |
