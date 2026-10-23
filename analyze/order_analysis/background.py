"""The whole block as background: how closely all transactions in the target leader's blocks (not only the probes)
follow the reward per cost unit. The leader's reward of a transaction is its priority fee plus half of its base fee
(5 000 lamports per signature, half burned); its cost is `costUnits` from the transaction's metadata.

This is a secondary check: other senders' transactions differ in accounts, size and path (bundles, external block
builders), so a weak relation here does not contradict a strict rule among same-shape probes.
"""

from __future__ import annotations

import numpy as np
import pandas as pd
from scipy import stats

from .load import Dataset

BASE_FEE_PER_SIGNATURE = 5_000


def block_rows(data: Dataset) -> pd.DataFrame:
    """Kendall's tau of position against reward per cost unit (positive = richer first) for every collected block of
    a target's window."""
    rows = []
    seen = set()
    for b in data.bursts.itertuples():
        for slot in range(b.window_start, b.window_start + b.window_slots):
            if (b.run_id, slot) in seen:
                continue
            block = data.block(b.run_id, slot)
            if block is None:
                continue
            seen.add((b.run_id, slot))
            txs = pd.DataFrame(block["transactions"])
            if txs.empty:
                continue
            txs["position"] = np.arange(len(txs))
            known = txs[~txs["err"] & txs["priority_fee"].notna() & txs["cost_units"].notna() & (txs["cost_units"] > 0)]
            reward = known["priority_fee"] + BASE_FEE_PER_SIGNATURE / 2 * known["signatures"].fillna(1)
            per_cost = reward / known["cost_units"]
            tau = stats.kendalltau(known["position"], -per_cost).statistic if len(known) > 2 else np.nan
            rows.append({
                "run_id": b.run_id, "slot": slot, "identity": b.identity, "client": b.client,
                "transactions": len(txs), "with_priority_info": len(known), "tau_reward_per_cost": tau,
                "position_in_window": slot - b.window_start,
            })
    return pd.DataFrame(rows)
