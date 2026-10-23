# Results (generated)

No live mainnet runs in the data yet.

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
