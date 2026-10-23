"""Within-block order metrics.

Only probes of one burst that landed in the same block are compared: probes in different slots are ordered by the
slot, which says nothing about the rule inside a block. For every such pair the probe placed earlier is compared
with the rule each hypothesis predicts:

- fee: the higher compute unit price first (all probes have the same shape and cost, so price orders the reward per
  cost unit the same way);
- arrival: the probe sent earlier first;
- payer: the probe with the lower payer index first. Prices are dealt to payers at random every burst, so this label
  carries no information; it is the null-model control for "the leader orders by account" and for chance.

A share of agreeing pairs of 0.5 is what an order unrelated to the label gives; 1.0 is a strict rule. Pairs from one
burst are not independent, so confidence intervals come from a cluster bootstrap over bursts, and the p-values from
permuting the block positions within each block (the H-noise null).
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np
import pandas as pd
from scipy import stats

GROUP = ["run_id", "burst", "landed_slot"]


def landed_groups(probes: pd.DataFrame) -> pd.DataFrame:
    """Probes that landed with a known block position, in blocks that hold at least two probes of their burst."""
    landed = probes[probes["block_index"].notna()].copy()
    landed["block_index"] = landed["block_index"].astype(int)
    size = landed.groupby(GROUP)["index"].transform("size")
    return landed[size >= 2]


def pairs(probes: pd.DataFrame) -> pd.DataFrame:
    """Every unordered pair of probes within one block, oriented so that `first` is the one placed earlier."""
    rows = []
    for key, group in landed_groups(probes).groupby(GROUP):
        g = group.sort_values("block_index").to_dict("records")
        for i in range(len(g)):
            for j in range(i + 1, len(g)):
                a, b = g[i], g[j]
                rows.append({
                    "run_id": key[0], "burst": key[1], "slot": key[2],
                    "identity": a["identity"], "client": a["client"], "variant": a["variant"],
                    "spacing_ms": a["spacing_ms"], "offset_ms": a["offset_ms"],
                    "price_first": a["cu_price"], "price_second": b["cu_price"],
                    "sent_first_ms": a["sent_ms"], "sent_second_ms": b["sent_ms"],
                    "index_first": a["index"], "index_second": b["index"],
                    "conflict": bool(a["shared_target"] and b["shared_target"]),
                    "gap": b["block_index"] - a["block_index"],
                })
    df = pd.DataFrame(rows)
    if df.empty:
        return df
    df["fee_agree"] = np.where(df.price_first == df.price_second, np.nan, (df.price_first > df.price_second).astype(float))
    df["arrival_agree"] = (df.sent_first_ms < df.sent_second_ms).astype(float)
    df["payer_agree"] = (df.index_first < df.index_second).astype(float)
    return df


def kendall(probes: pd.DataFrame) -> pd.DataFrame:
    """Kendall's tau-b per block: position against price (positive = higher price earlier) and against send order
    (positive = sent earlier, placed earlier)."""
    rows = []
    for key, g in landed_groups(probes).groupby(GROUP):
        fee = stats.kendalltau(g["block_index"], -g["cu_price"]).statistic if g["cu_price"].nunique() > 1 else np.nan
        arrival = stats.kendalltau(g["block_index"], g["send_order"]).statistic
        rows.append({
            "run_id": key[0], "burst": key[1], "slot": key[2], "identity": g["identity"].iloc[0],
            "client": g["client"].iloc[0], "variant": g["variant"].iloc[0], "probes": len(g),
            "tau_fee": fee, "tau_arrival": arrival,
        })
    return pd.DataFrame(rows)


@dataclass
class Share:
    share: float
    low: float
    high: float
    p_value: float
    pairs: int
    bursts: int


# The label of each rule, oriented so that "higher label placed earlier" agrees with the rule
LABELS = {
    "fee": lambda g: g["cu_price"].astype(float),
    "arrival": lambda g: -g["sent_ms"].astype(float),
    "payer": lambda g: -g["index"].astype(float),
}


def ordered_labels(probes: pd.DataFrame, rule: str) -> tuple[list[np.ndarray], np.ndarray]:
    """For every block with two or more probes of a burst: the rule's labels in block order, and the burst id."""
    groups, bursts = [], []
    for key, g in landed_groups(probes).groupby(GROUP):
        labels = LABELS[rule](g.sort_values("block_index")).to_numpy()
        if np.isnan(labels).any():
            continue
        groups.append(labels)
        bursts.append(f"{key[0]}:{key[1]}")
    return groups, pd.factorize(pd.Series(bursts, dtype=str))[0]


def _agreement(labels: np.ndarray) -> tuple[np.ndarray, int]:
    """Agreeing pairs (earlier probe has the higher label) for each row of `labels` (orders × probes), and the number
    of pairs with different labels."""
    n = labels.shape[-1]
    upper = np.triu(np.ones((n, n), dtype=bool), 1)
    diff = labels[..., :, None] - labels[..., None, :]
    return ((diff > 0) & upper).sum(axis=(-2, -1)), int(((diff != 0) & upper).sum(axis=(-2, -1)).flat[0])


def share(probes: pd.DataFrame, rule: str, rng: np.random.Generator, n_boot: int = 2000, n_perm: int = 2000) -> Share:
    """Share of within-block pairs that agree with `rule`, its 95% cluster-bootstrap CI over bursts, and the two-sided
    permutation p-value against H-noise (every order of a block's probes equally likely)."""
    groups, bursts = ordered_labels(probes, rule)
    agree, counts = np.zeros(len(groups)), np.zeros(len(groups))
    for k, labels in enumerate(groups):
        a, n = _agreement(labels[None, :])
        agree[k], counts[k] = a[0], n
    total = counts.sum()
    if total == 0:
        return Share(np.nan, np.nan, np.nan, np.nan, 0, 0)
    observed = agree.sum() / total

    n_bursts = bursts.max() + 1
    burst_agree = np.bincount(bursts, weights=agree, minlength=n_bursts)
    burst_counts = np.bincount(bursts, weights=counts, minlength=n_bursts)
    draws = rng.integers(0, n_bursts, size=(n_boot, n_bursts))
    boot = burst_agree[draws].sum(axis=1) / np.maximum(burst_counts[draws].sum(axis=1), 1)
    low, high = np.quantile(boot, [0.025, 0.975])

    permuted = np.zeros(n_perm)
    for labels, n in zip(groups, counts):
        if n == 0:
            continue
        orders = np.argsort(rng.random((n_perm, len(labels))), axis=1)
        permuted += _agreement(labels[orders])[0]
    permuted /= total
    p = (np.sum(np.abs(permuted - 0.5) >= abs(observed - 0.5) - 1e-12) + 1) / (n_perm + 1)
    return Share(float(observed), float(low), float(high), float(p), int(total), int(n_bursts))


def logistic(df: pd.DataFrame, rng: np.random.Generator, n_boot: int = 500) -> dict:
    """Logistic regression of "probe A placed before probe B" on the price difference (log10, decades) and the send
    time difference (per 10 ms, positive = A sent later), without intercept (the orientation of each pair is random).
    Coefficients with 95% cluster-bootstrap intervals over bursts."""
    d = df[df["price_first"].notna() & df["sent_first_ms"].notna()]
    if len(d) < 10:
        return {}
    flip = rng.random(len(d)) < 0.5
    p1, p2 = np.log10(d.price_first.to_numpy()), np.log10(d.price_second.to_numpy())
    s1, s2 = d.sent_first_ms.to_numpy(), d.sent_second_ms.to_numpy()
    x = np.column_stack([np.where(flip, p2 - p1, p1 - p2), np.where(flip, s2 - s1, s1 - s2) / 10.0])
    y = np.where(flip, 0.0, 1.0)
    clusters = pd.factorize(d["run_id"].astype(str) + ":" + d["burst"].astype(str))[0]

    def fit(xm: np.ndarray, ym: np.ndarray) -> np.ndarray:
        beta = np.zeros(xm.shape[1])
        for _ in range(50):
            p = 1 / (1 + np.exp(-np.clip(xm @ beta, -30, 30)))
            gradient = xm.T @ (ym - p)
            hessian = xm.T @ (xm * (p * (1 - p))[:, None]) + 1e-6 * np.eye(xm.shape[1])
            step = np.linalg.solve(hessian, gradient)
            beta += step
            if np.abs(step).max() < 1e-8:
                break
        return beta

    beta = fit(x, y)
    n_clusters = clusters.max() + 1
    members = [np.flatnonzero(clusters == c) for c in range(n_clusters)]
    boot = []
    for _ in range(n_boot):
        rows = np.concatenate([members[c] for c in rng.integers(0, n_clusters, n_clusters)])
        boot.append(fit(x[rows], y[rows]))
    boot = np.array(boot)
    low, high = np.quantile(boot, [0.025, 0.975], axis=0)
    return {
        "beta_price_per_decade": float(beta[0]), "beta_price_low": float(low[0]), "beta_price_high": float(high[0]),
        "beta_send_per_10ms": float(beta[1]), "beta_send_low": float(low[1]), "beta_send_high": float(high[1]),
        "pairs": int(len(d)),
    }


def verdict(fee: Share, arrival: Share, payer: Share, min_pairs: int, min_bursts: int = 5) -> str:
    """The pre-registered reading of the shares (README): a rule is claimed only when its permutation p-value is below
    0.01 and its bootstrap interval excludes 0.5, from at least `min_pairs` pairs in at least `min_bursts` bursts (an
    interval resampled from fewer bursts says little). `fee` comes from bursts where price is independent of the send
    order (main, conflict); `arrival` and `payer` from equal-price bursts."""
    if fee.pairs < min_pairs or fee.bursts < min_bursts:
        return "insufficient data"

    def holds(s: Share) -> bool:
        return s.pairs >= min_pairs and s.bursts >= min_bursts and s.p_value < 0.01 and s.low > 0.5

    if holds(fee) and holds(arrival):
        return "priority fee, then arrival"
    if holds(fee):
        return "strict priority fee" if fee.low >= 0.95 else "priority fee (not strict)"
    if holds(arrival):
        return "arrival order (FIFO)"
    if holds(payer):
        return "by account (payer), not fee or arrival"
    if fee.p_value < 0.01 and fee.high < 0.5:
        return "anti-fee (cheaper first)"
    return "no detectable rule"
