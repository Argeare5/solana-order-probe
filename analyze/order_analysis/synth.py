"""Synthetic runs in the probe's exact file format, with leaders that follow known rules. The tests check that the
analysis recovers each rule; nothing here touches data/.

`python -m order_analysis.synth <dir>` writes a synthetic data directory to try the analysis on.
"""

from __future__ import annotations

import gzip
import json
import sys
from pathlib import Path

import numpy as np

LADDER = [1000, 2000, 5000, 10000, 20000, 50000, 100000, 200000, 500000, 1000000, 2000000, 5000000]
VARIANTS = ["main", "null", "reverse", "conflict"]

# Rule name → key that sorts a block's probes (smallest first in the block)
RULES = {
    "SynthFee": lambda p, rng: (-p["cu_price"], rng.random()),
    "SynthFeeFifo": lambda p, rng: (-p["cu_price"], p["sent_offset_us"]),
    "SynthFifo": lambda p, rng: (p["sent_offset_us"],),
    "SynthRandom": lambda p, rng: (rng.random(),),
    "SynthPayer": lambda p, rng: (p["index"],),
}


def write(out: Path, bursts_per_validator: int = 24, seed: int = 7, drop: float = 0.15) -> None:
    rng = np.random.default_rng(seed)
    run_id = "20990101T000000Z"
    run_dir = out / "runs" / run_id
    (run_dir / "blocks").mkdir(parents=True, exist_ok=True)
    (run_dir / "run.json").write_text(json.dumps({"run_id": run_id, "cluster": "synthetic", "live": True}))
    slot = 1_000_000
    burst = 0
    with open(run_dir / "bursts.jsonl", "w") as bursts_file, open(run_dir / "results.jsonl", "w") as results_file:
        for client, direct_rule in RULES.items():
            identity = f"{client}Validator1111111111111111111111"
            # The fee-ordered leader also gets bursts through an RPC node, which here scrambles them
            plan = [("quic", bursts_per_validator, direct_rule), ("rpc", 8 if client == "SynthFee" else 0, RULES["SynthRandom"])]
            for path, done, rule in ((path, done, rule) for path, count, rule in plan for done in range(count)):
                variant = VARIANTS[done % 4]
                spacing = [0, 10, 50][(done // 8) % 3]
                price_rank = rng.permutation(12)
                order = np.argsort(price_rank) if variant == "reverse" else rng.permutation(12)
                probes = []
                for i in range(12):
                    send_order = int(np.flatnonzero(order == i)[0])
                    probes.append({
                        "index": i, "tag": f"sop:{run_id}:{burst}:{i}", "signature": f"sig{burst}x{i}",
                        "payer": f"payer{i}", "cu_limit": 900,
                        "cu_price": LADDER[6] if variant == "null" else LADDER[price_rank[i]],
                        "max_fee_lamports": 0, "shared_target": variant == "conflict" and i % 2 == 1,
                        "send_order": send_order,
                        # Sends are sequential, so the offset grows with the send order even at spacing 0
                        "sent_offset_us": send_order * (spacing * 1000 + 30) + int(rng.integers(0, 10)),
                        "send_error": None,
                    })
                record = {
                    "run_id": run_id, "burst": burst, "live": True, "identity": identity, "client": client,
                    "version": "synthetic", "tpu_quic": "127.0.0.1:1", "stake_sol": 1.0,
                    "window": {"start": slot, "slots": 4}, "path": path, "variant": variant, "spacing_ms": spacing,
                    "offset_ms": -300.0 if done % 8 < 4 else 100.0, "fire_at": "2099-01-01T00:00:00Z",
                    "fire_vs_observed_ms": float(rng.normal(-300 if done % 8 < 4 else 100, 40)),
                    "clock_basis": "node_first_shred", "quic_handshake_ms": 50.0, "quic_rtt_ms": 40.0,
                    "probes": probes,
                }
                bursts_file.write(json.dumps(record) + "\n")

                # Landing: some probes are dropped; the rest land in the window's first slot in the rule's order,
                # spread among background transactions
                landed = [p for p in probes if rng.random() > drop]
                landed.sort(key=lambda p: rule(p, rng))
                background = 300
                positions = np.sort(rng.choice(background + len(landed), size=len(landed), replace=False))
                txs = [{"signature": f"bg{slot}x{k}", "fee": 5000 + int(rng.integers(0, 50000)), "cu": 1000,
                        "cost_units": 2000, "err": False, "version": 0, "signatures": 1, "cu_limit": None,
                        "cu_price": None, "priority_fee": int(rng.integers(0, 50000))} for k in range(background + len(landed))]
                results = []
                position_of = {}
                for p, position in zip(landed, positions):
                    txs[position] = {"signature": p["signature"], "fee": 5000, "cu": 806, "cost_units": 2000,
                                     "err": False, "version": "legacy", "signatures": 1, "cu_limit": 900,
                                     "cu_price": p["cu_price"], "priority_fee": -(-900 * p["cu_price"] // 1_000_000)}
                    position_of[p["index"]] = int(position)
                with gzip.open(run_dir / "blocks" / f"{slot}.json.gz", "wt") as f:
                    json.dump({"slot": slot, "transactions": txs}, f)
                for p in probes:
                    hit = p["index"] in position_of
                    results.append({
                        "index": p["index"], "signature": p["signature"], "landed_slot": slot if hit else None,
                        "in_window": hit, "block_index": position_of.get(p["index"]),
                        "block_transactions": len(txs) if hit else None, "fee_lamports": 5000 if hit else None,
                        "err": None,
                    })
                results_file.write(json.dumps({
                    "run_id": run_id, "burst": burst, "identity": identity, "client": client, "window_start": slot,
                    "window_slots": 4, "landed": len(landed), "landed_in_window": len(landed), "results": results,
                }) + "\n")
                burst += 1
                slot += 40


if __name__ == "__main__":
    write(Path(sys.argv[1]))
    print(f"synthetic data written to {sys.argv[1]}")
