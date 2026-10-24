"""The analysis recovers the rule of every synthetic leader, and finds none where there is none."""

from pathlib import Path

import numpy as np
import pytest

from order_analysis import load, metrics, report, synth


@pytest.fixture(scope="module")
def summary(tmp_path_factory):
    root = tmp_path_factory.mktemp("synthetic")
    synth.write(root / "data")
    data = load.load(root / "data", cluster="synthetic")
    return report.build(data, root / "out", seed=1, n_boot=400, n_perm=400, min_pairs=30, min_bursts=5)


def verdicts(summary):
    return {v["client"]: v["verdict"] for v in summary["validators"]}


def test_rules_are_recovered(summary):
    assert verdicts(summary) == {
        "SynthFee": "strict priority fee",
        "SynthFeeFifo": "priority fee, then arrival",
        "SynthFifo": "arrival order (FIFO)",
        "SynthRandom": "no detectable rule",
        "SynthPayer": "by account (payer), not fee or arrival",
    }


def test_fifo_leader_inverts_the_reverse_bursts(summary):
    fifo = next(v for v in summary["validators"] if v["client"] == "SynthFifo")
    assert fifo["reverse_fee_share"] == 0.0
    assert 0.35 < fifo["fee_share"] < 0.65


def test_random_leader_stays_near_one_half(summary):
    random = next(v for v in summary["validators"] if v["client"] == "SynthRandom")
    # A 95% interval misses 0.5 one time in twenty by chance; the verdict rests on the permutation p-value
    assert 0.4 < random["fee_share"] < 0.6
    assert random["fee_p_value"] > 0.01


def test_outputs_are_written(summary, tmp_path_factory):
    out = Path(tmp_path_factory.getbasetemp()) / "synthetic0" / "out"
    for name in ("summary.md", "validators.csv", "pairs.csv", "rpc_path.csv", "figures/shares_by_validator.png",
                 "figures/position_vs_price.png", "figures/fire_timing.png"):
        assert (out / name).exists(), name


def test_a_single_session_gives_the_pooled_verdicts(summary):
    assert {v["client"]: v["verdict"] for v in summary["session_verdicts"]} == verdicts(summary)


def test_session_verdicts_get_a_column_per_session():
    import pandas as pd

    rows = [{"run_id": run_id, "client": "C", "identity": "A" * 44, "verdict": "v", "fee_share": 0.9, "fee_bursts": 6,
             "arrival_null_share": float("nan"), "arrival_null_bursts": 0} for run_id in ("r1", "r2")]
    table = report._session_verdicts_markdown(pd.DataFrame(rows))
    assert "| Client | Validator | r1 | r2 |" in table
    assert "| C | `AAAA…AAAA` | v; 0.90 (6) / – (0) | v; 0.90 (6) / – (0) |" in table
    assert report._session_verdicts_markdown(pd.DataFrame(rows[:1])) == ""


def test_probes_outside_the_window_are_not_compared():
    import pandas as pd

    # Two probes in the target's block, two handed to another leader's block by a shared ingress
    probes = pd.DataFrame({"run_id": "r", "burst": 0, "landed_slot": [10, 10, 9, 9], "index": [0, 1, 2, 3],
                           "block_index": [0.0, 1.0, 0.0, 1.0], "in_window": [True, True, False, False]})
    assert list(metrics.landed_groups(probes)["index"]) == [0, 1]


def test_agreement_counts_pairs():
    agree, n = metrics._agreement(np.array([[3.0, 2.0, 1.0], [1.0, 2.0, 3.0]]))
    assert list(agree) == [3, 0] and n == 3
    agree, n = metrics._agreement(np.array([[2.0, 2.0, 1.0]]))
    assert list(agree) == [2] and n == 2


def test_missing_fire_timing_comes_from_the_recorded_stream(tmp_path):
    import gzip
    import json

    synth.write(tmp_path / "data", bursts_per_validator=4)
    path = tmp_path / "data" / "runs" / "20990101T000000Z" / "bursts.jsonl"
    bursts = [json.loads(line) for line in path.read_text().splitlines()]
    # The run's clock missed the first burst's window start; a slot stream recorded alongside saw it
    bursts[0]["fire_vs_observed_ms"] = None
    bursts[0]["fire_unix_us"] = 5_000_000_000_000
    path.write_text("".join(json.dumps(b) + "\n" for b in bursts))
    window = bursts[0]["window"]["start"]
    # 40 first shreds delivered 50 ms after the node's timestamp; the window's first shred 250 ms after the fire
    # moment by the node's clock, so 300 ms after it on this machine's clock
    events = [{"unix_us": 0, "kind": "connected", "slot": 0, "node_ms": None}]
    for i in range(40):
        node_ms = 5_000_000_250 - (39 - i) * 200
        events.append({"unix_us": (node_ms + 50) * 1000, "kind": "firstShredReceived", "slot": window - 39 + i, "node_ms": node_ms})
    (tmp_path / "data" / "calibration" / "x").mkdir(parents=True)
    with gzip.open(tmp_path / "data" / "calibration" / "x" / "events.json.gz", "wt") as f:
        json.dump(events, f)

    data = load.load(tmp_path / "data", cluster="synthetic")
    first = data.bursts.iloc[0]
    assert first["fire_timing_source"] == "slot stream recorded alongside"
    assert abs(first["fire_vs_observed_ms"] - (-300.0)) < 1e-6
    assert (data.bursts["fire_timing_source"].iloc[1:] == "run clock").all()
    assert abs(data.probes[data.probes["burst"] == 0]["fire_vs_observed_ms"].iloc[0] - (-300.0)) < 1e-6
