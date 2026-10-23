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


def test_agreement_counts_pairs():
    agree, n = metrics._agreement(np.array([[3.0, 2.0, 1.0], [1.0, 2.0, 3.0]]))
    assert list(agree) == [3, 0] and n == 3
    agree, n = metrics._agreement(np.array([[2.0, 2.0, 1.0]]))
    assert list(agree) == [2] and n == 2
