"""Loading the raw data the probe writes: data/runs/<run>/{run.json, bursts.jsonl, results.jsonl,
reconciled.jsonl, blocks/*.json.gz}.

One row per probe joins what was planned and sent (bursts.jsonl) with where it landed (results.jsonl, and
reconciled.jsonl for probes found to have landed after their burst was collected). Only live runs on the requested
cluster are kept; dry-runs carry no landings.
"""

from __future__ import annotations

import gzip
import json
from dataclasses import dataclass
from pathlib import Path

import pandas as pd


@dataclass
class Dataset:
    data_dir: Path
    runs: pd.DataFrame
    bursts: pd.DataFrame
    probes: pd.DataFrame
    run_dirs: dict[str, Path]

    def block(self, run_id: str, slot: int) -> dict | None:
        path = self.run_dirs[run_id] / "blocks" / f"{slot}.json.gz"
        if not path.exists():
            return None
        with gzip.open(path, "rt") as f:
            return json.load(f)


def _jsonl(path: Path) -> list[dict]:
    if not path.exists():
        return []
    with open(path) as f:
        return [json.loads(line) for line in f if line.strip()]


BURST_COLUMNS = [
    "run_id", "burst", "identity", "client", "version", "stake_sol", "path", "variant", "spacing_ms", "offset_ms",
    "fire_at", "fire_vs_observed_ms", "clock_basis", "quic_handshake_ms", "quic_rtt_ms",
]


def load(data_dir: Path, cluster: str | None = "mainnet-beta", include_dry: bool = False) -> Dataset:
    runs, bursts, probes, run_dirs = [], [], [], {}
    for run_json in sorted((data_dir / "runs").glob("*/run.json")):
        run_dir = run_json.parent
        run = json.loads(run_json.read_text())
        if cluster is not None and run.get("cluster") != cluster:
            continue
        if not run.get("live") and not include_dry:
            continue
        run_dirs[run["run_id"]] = run_dir
        runs.append(run)
        results = {(r["run_id"], r["burst"]): r for r in _jsonl(run_dir / "results.jsonl")}
        late = {c["signature"]: c for c in _jsonl(run_dir / "reconciled.jsonl") if c.get("landed_slot") is not None}
        for b in _jsonl(run_dir / "bursts.jsonl"):
            row = {k: b.get(k) for k in BURST_COLUMNS}
            row["path"] = row["path"] or "quic"
            row["window_start"] = b["window"]["start"]
            row["window_slots"] = b["window"]["slots"]
            result = results.get((b["run_id"], b["burst"]))
            row["collected"] = result is not None
            bursts.append(row)
            landed = {r["index"]: r for r in (result or {}).get("results", [])}
            for p in b["probes"]:
                r = landed.get(p["index"], {})
                if r.get("landed_slot") is None and p["signature"] in late:
                    # Landed after collection: its slot and fee are known, its place in the block is not
                    c = late[p["signature"]]
                    r = {"landed_slot": c["landed_slot"], "in_window": c["in_window"], "fee_lamports": c["fee_lamports"],
                         "err": c.get("err"), "late": True}
                probes.append({
                    **{k: row[k] for k in ("run_id", "burst", "identity", "client", "version", "path", "variant",
                                           "spacing_ms", "offset_ms", "fire_vs_observed_ms", "window_start",
                                           "window_slots")},
                    "index": p["index"],
                    "signature": p["signature"],
                    "payer": p["payer"],
                    "cu_limit": p["cu_limit"],
                    "cu_price": p["cu_price"],
                    "shared_target": p["shared_target"],
                    "send_order": p["send_order"],
                    "sent_ms": None if p.get("sent_offset_us") is None else p["sent_offset_us"] / 1e3,
                    "send_error": p.get("send_error"),
                    "tpu_ack": p.get("tpu_ack"),
                    "landed_late": bool(r.get("late")),
                    "landed_slot": r.get("landed_slot"),
                    "in_window": r.get("in_window", False),
                    "block_index": r.get("block_index"),
                    "block_transactions": r.get("block_transactions"),
                    "fee_lamports": r.get("fee_lamports"),
                    "failed": r.get("err") is not None,
                })
    probes_df = pd.DataFrame(probes)
    if not probes_df.empty:
        for column in ("landed_slot", "block_index", "block_transactions", "fee_lamports"):
            probes_df[column] = probes_df[column].astype("Int64")
    return Dataset(data_dir, pd.DataFrame(runs), pd.DataFrame(bursts), probes_df, run_dirs)
