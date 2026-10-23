"""`python -m order_analysis`: recomputes every table and figure from the raw data."""

from __future__ import annotations

import argparse
from pathlib import Path

from . import load, report


def main() -> None:
    parser = argparse.ArgumentParser(prog="order_analysis", description=__doc__)
    parser.add_argument("--data", type=Path, default=Path("data"), help="raw data directory (data/runs/...)")
    parser.add_argument("--out", type=Path, default=Path("report/generated"), help="output directory")
    parser.add_argument("--cluster", default="mainnet-beta", help="only runs on this cluster ('any' for all)")
    parser.add_argument("--seed", type=int, default=20261023, help="seed of the bootstrap and permutations")
    parser.add_argument("--bootstrap", type=int, default=2000)
    parser.add_argument("--permutations", type=int, default=2000)
    parser.add_argument("--min-pairs", type=int, default=30, help="pairs a validator needs before a verdict")
    args = parser.parse_args()

    data = load.load(args.data, cluster=None if args.cluster == "any" else args.cluster)
    summary = report.build(data, args.out, args.seed, args.bootstrap, args.permutations, args.min_pairs)
    print(f"{len(summary['runs'])} live runs, {len(summary.get('validators', []))} validators → {args.out}")
    for v in summary.get("validators", []):
        print(f"  {v['client']:<20} {v['identity']}  {v['verdict']}  fee share {v['fee_share']}")


if __name__ == "__main__":
    main()
