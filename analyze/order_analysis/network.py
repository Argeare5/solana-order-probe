"""The network around the measurements, from the read-only records in data/: where each client's leaders take
transactions in (snapshots) and how leader windows start and hand over by client (calibrations).

Gossip advertises a TPU address per node. When it is not the node's own address and many nodes share it, the
transactions sent "to the leader" first reach a shared ingress (an external sequencer or block builder), and the
arrival order we control is the order at that ingress.
"""

from __future__ import annotations

import gzip
import json
from pathlib import Path

import pandas as pd


def _host(address: str | None) -> str | None:
    return address.rsplit(":", 1)[0] if address else None


def tpu_ingress(data_dir: Path) -> pd.DataFrame:
    """Per snapshot and client: staked nodes, their stake share, distinct TPU hosts, nodes whose TPU host is their
    own gossip host, and the most nodes behind one TPU host."""
    rows = []
    for snapshot in sorted((data_dir / "snapshots").glob("*/cluster_nodes.json.gz")):
        directory = snapshot.parent
        with gzip.open(snapshot, "rt") as f:
            nodes = json.load(f)
        with gzip.open(directory / "vote_accounts.json.gz", "rt") as f:
            votes = json.load(f)
        stake = {v["nodePubkey"]: v["activatedStake"] for v in votes["current"]}
        total = sum(stake.values())
        frame = pd.DataFrame([
            {
                "client": n.get("clientId") or "none",
                "identity": n["pubkey"],
                "stake": stake[n["pubkey"]],
                "tpu_host": _host(n.get("tpuQuic")),
                "own_host": _host(n.get("tpuQuic")) == _host(n.get("gossip")),
            }
            for n in nodes
            if n["pubkey"] in stake and n.get("tpuQuic")
        ])
        for client, g in frame.groupby("client"):
            rows.append({
                "snapshot": directory.name,
                "client": client,
                "staked_nodes": len(g),
                "stake_share": g["stake"].sum() / total,
                "tpu_hosts": g["tpu_host"].nunique(),
                "tpu_on_own_host": int(g["own_host"].sum()),
                "most_nodes_per_tpu_host": int(g["tpu_host"].value_counts().iloc[0]),
            })
    return pd.DataFrame(rows)


def hand_over(data_dir: Path) -> pd.DataFrame:
    """Per calibration and client of the leader: windows, skipped windows, hand-over into its windows, window
    length, and the slot clock's error at its window starts (ms, from the node's first-shred timestamps)."""
    rows = []
    for path in sorted((data_dir / "calibration").glob("*/summary.json")):
        summary = json.loads(path.read_text())
        clients = ((summary.get("summary") or {}).get("by_client") or {}).get("clients") or []
        for c in clients:
            rows.append({
                "calibration": path.parent.name,
                "minutes": summary.get("minutes"),
                "client": c["client"],
                "windows": c["windows"],
                "skipped_windows": c["skipped_windows"],
                **{f"hand_over_in_{k}": c["hand_over_in_ms"][k] for k in ("median", "p10", "p90")},
                **{f"window_{k}": c["window_ms"][k] for k in ("median", "p10", "p90")},
                **{f"clock_error_{k}": c["clock_error_ms"][k] for k in ("median", "p10", "p90")},
            })
    return pd.DataFrame(rows)


def slot_positions(data_dir: Path) -> pd.DataFrame:
    """Per calibration: slot duration by position in the leader window."""
    rows = []
    for path in sorted((data_dir / "calibration").glob("*/summary.json")):
        summary = json.loads(path.read_text()).get("summary") or {}
        for r in summary.get("slot_interval_by_position_ms", []):
            rows.append({"calibration": path.parent.name, **r})
    return pd.DataFrame(rows)
