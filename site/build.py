"""Builds the landing page (docs/index.html) from the analysis output, so its numbers are the report's.

    python3 site/build.py

Reads report/generated/{validators,pairs,by_session_validator}.csv, report/generated/summary.json, the spend ledger
and one burst of session 2, and fills site/template.html. Charts and tables are written as static HTML and SVG: the
page reads fully without JavaScript, which only animates the replay and shows tooltips. Standard library only.
"""

from __future__ import annotations

import csv
import html
import json
import math
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GENERATED = ROOT / "report" / "generated"
SESSIONS = {"20261023T102008Z": "s1", "20261024T023015Z": "s2"}
EXAMPLE = ("20261024T023015Z", 12)  # a reverse burst into the FireBAM leader: sent cheapest first
CLIENT_ORDER_KEY = "before"
INDEPENDENT = {"main", "conflict"}


def rows(name: str) -> list[dict]:
    with open(GENERATED / name, newline="") as f:
        return list(csv.DictReader(f))


def fmt(v: float) -> str:
    return f"{v:.2f}"


def thousands(n: int) -> str:
    return f"{n:,}".replace(",", " ")


def short(identity: str) -> str:
    return f"{identity[:4]}…{identity[-4:]}"


def esc(text: str) -> str:
    return html.escape(text, quote=True)


# ---------------------------------------------------------------- data

def client_shares() -> list[dict]:
    """Higher-fee-first share per client: whole burst at the leader before its window (fired 300 ms early, at most
    10 ms apart) and probes arriving while it builds (fired 100 ms into the window)."""
    acc = defaultdict(lambda: {"before": [0, 0], "building": [0, 0]})
    for r in rows("pairs.csv"):
        if r["variant"] not in INDEPENDENT or r["fee_agree"] == "":
            continue
        offset, spacing = float(r["offset_ms"]), float(r["spacing_ms"])
        key = "before" if offset < 0 and spacing <= 10 else "building" if offset > 0 else None
        if key:
            acc[r["client"]][key][0] += float(r["fee_agree"])
            acc[r["client"]][key][1] += 1
    out = [{"client": c, "before": v["before"][0] / v["before"][1], "before_n": v["before"][1],
            "building": v["building"][0] / v["building"][1], "building_n": v["building"][1]} for c, v in acc.items()]
    return sorted(out, key=lambda r: -r[CLIENT_ORDER_KEY])


def validators() -> list[dict]:
    sessions = defaultdict(dict)
    for r in rows("by_session_validator.csv"):
        if r["run_id"] in SESSIONS:
            sessions[r["identity"]][SESSIONS[r["run_id"]]] = (float(r["fee_share"]), int(r["fee_bursts"]), r["verdict"])
    out = []
    for r in rows("validators.csv"):
        out.append({
            "client": r["client"], "id": r["identity"], "verdict": r["verdict"],
            "fee": float(r["fee_share"]), "fee_lo": float(r["fee_low"]), "fee_hi": float(r["fee_high"]),
            "eq": float(r["arrival_null_share"]), "eq_lo": float(r["arrival_null_low"]), "eq_hi": float(r["arrival_null_high"]),
            "bursts": int(r["fee_bursts"]), "sessions": sessions.get(r["identity"], {}),
        })
    return out


def example_burst() -> dict:
    run, number = EXAMPLE
    run_dir = ROOT / "data" / "runs" / run
    burst = next(b for b in map(json.loads, open(run_dir / "bursts.jsonl")) if b["burst"] == number)
    result = next(r for r in map(json.loads, open(run_dir / "results.jsonl")) if r["burst"] == number)
    probes = {p["index"]: p for p in burst["probes"]}
    landed = sorted((r for r in result["results"] if r["block_index"] is not None), key=lambda r: r["block_index"])
    items = [{"pos": r["block_index"], "price": probes[r["index"]]["cu_price"], "sent": probes[r["index"]]["send_order"] + 1,
              "sig": r["signature"]} for r in landed]
    prices = [i["price"] for i in items]
    in_order = sum(1 for a in range(len(prices)) for b in range(a + 1, len(prices)) if prices[a] > prices[b])
    return {"slot": landed[0]["landed_slot"], "txs": landed[0]["block_transactions"], "items": items,
            "pairs": len(prices) * (len(prices) - 1) // 2, "in_order": in_order, "client": burst["client"],
            "identity": burst["identity"], "variant": burst["variant"], "fire_at": burst["fire_at"]}


def totals() -> dict:
    summary = json.loads((GENERATED / "summary.json").read_text())
    ledger = [json.loads(line) for line in open(ROOT / "data" / "spend_ledger.jsonl")]
    by_kind = defaultdict(int)
    for entry in ledger:
        by_kind[entry["kind"]] += entry["fee_lamports"]
    delivery = rows("delivery.csv")
    vals = validators()
    per_client = defaultdict(int)
    for v in vals:
        per_client[v["client"]] += 1
    # Stake share of the measured clients in the latest snapshot
    ingress = rows("tpu_ingress.csv")
    latest = max(r["snapshot"] for r in ingress)
    stake = sum(float(r["stake_share"]) for r in ingress if r["snapshot"] == latest and r["client"] in per_client)
    windows = {}
    for run in SESSIONS:
        fired = [json.loads(line)["fire_at"] for line in open(ROOT / "data" / "runs" / run / "bursts.jsonl")]
        windows[run] = (min(fired), max(fired))
    return {"probes": summary["probes_sent"], "landed": summary["probes_landed"],
            "in_window": sum(int(r["in_window"]) for r in delivery), "bursts": sum(int(r["bursts"]) for r in delivery),
            "validators": len(vals), "per_client": dict(sorted(per_client.items())), "clients": len(per_client),
            "stake": stake, "sessions": len(SESSIONS), "windows": windows,
            "sol": sum(by_kind.values()) / 1e9, "by_kind": dict(by_kind)}


def tile_tips(tot: dict) -> dict:
    def when(run: str) -> str:
        start, end = tot["windows"][run]
        return f"{start[5:7].lstrip('0')}/{start[8:10]} {start[11:16]}–{end[11:16]} UTC".replace("10/", "Oct ")
    k = tot["by_kind"]
    return {
        "TIP_PROBES": esc(f"<b>{tot['probes']:,} probes</b> in {tot['bursts']} bursts of 12<br>{tot['landed']:,} landed, "
                          f"{tot['in_window']:,} in the target leader's own window"),
        "TIP_VALIDATORS": esc("<b>Leaders measured</b><br>" + "<br>".join(f"{n} × {c}" for c, n in tot["per_client"].items())),
        "TIP_CLIENTS": esc(f"<b>Clients</b> as gossip reports them<br>{', '.join(tot['per_client'])}<br>"
                           f"together {tot['stake'] * 100:.1f}% of the stake"),
        "TIP_SESSIONS": esc("<b>Two campaign sessions</b><br>" + "<br>".join(
            f"{label}: {when(run)}" for run, label in zip(SESSIONS, ("midday", "night"))) + "<br>plus a 24-burst pilot"),
        "TIP_SOL": esc(f"<b>{sum(k.values()):,} lamports</b> in all<br>probes {k.get('probe', 0):,} · funding {k.get('fund', 0):,}"
                       f" · sweep {k.get('sweep', 0):,}<br>cap 0.05 SOL, enforced in code<br>the ledger matches the chain to the lamport"),
    }


# ---------------------------------------------------------------- rendering

def price_color(t: float) -> str:
    """One hue, dark to bright with the price: a sequential ramp for the replay."""
    return f"hsl(188 {62 + 30 * t:.0f}% {30 + 42 * t:.0f}%)"


def render_replay(ex: dict) -> str:
    """The replay drawn in its final state: probes in send order (top), the leader's queue (middle) and the block
    (bottom), each probe's path running through its place in the queue. The script animates it."""
    lo, hi = math.log10(1000), math.log10(5_000_000)
    width, n = 560, len(ex["items"])
    step, bw, top_base, bottom_top, max_h = width / n, 22, 86, 230, 72
    hub_y, hub_x0, hub_x1 = 158, 96, 464
    q_step = (hub_x1 - hub_x0 - 24) / (n - 1)
    links, bars, queue = [], [], []
    for rank, item in enumerate(ex["items"]):
        t = (math.log10(item["price"]) - lo) / (hi - lo)
        h = 12 + (max_h - 12) * t
        c = price_color(t)
        xs, xb, xq = step * (item["sent"] - 1) + step / 2, step * rank + step / 2, hub_x0 + 12 + q_step * rank
        y0, y1 = top_base + 20, bottom_top - 6
        links.append(f'<path class="link" data-sent="{item["sent"]}" stroke="{c}" '
                     f'd="M{xs:.1f} {y0} C{xs:.1f} {y0 + 26} {xq:.1f} {hub_y - 26} {xq:.1f} {hub_y} '
                     f'C{xq:.1f} {hub_y + 30} {xb:.1f} {y1 - 30} {xb:.1f} {y1}" '
                     f'data-split="M{xs:.1f} {y0} C{xs:.1f} {y0 + 26} {xq:.1f} {hub_y - 26} {xq:.1f} {hub_y}"/>')
        queue.append(f'<circle class="qdot" cx="{xq:.1f}" cy="{hub_y}" r="3" fill="{c}"/>')
        bars.append(
            f'<g class="pr" data-sent="{item["sent"]}" data-rank="{rank}">'
            f'<rect class="slot" x="{xs - bw / 2:.1f}" y="{top_base - max_h}" width="{bw}" height="{max_h}" rx="4"/>'
            f'<rect class="rtop" x="{xs - bw / 2:.1f}" y="{top_base - h:.1f}" width="{bw}" height="{h:.1f}" rx="4" fill="{c}"/>'
            f'<text class="lab" x="{xs:.1f}" y="{top_base + 14}">{item["sent"]}</text>'
            f'<rect class="slot" x="{xb - bw / 2:.1f}" y="{bottom_top}" width="{bw}" height="{max_h}" rx="4"/>'
            f'<rect class="rbot" x="{xb - bw / 2:.1f}" y="{bottom_top + max_h - h:.1f}" width="{bw}" height="{h:.1f}" rx="4" fill="{c}"/>'
            f'<text class="lab blab" x="{xb:.1f}" y="{bottom_top + max_h + 14}">#{item["pos"]}</text>'
            f'<circle class="fly" r="5" fill="{c}" cx="{xs:.1f}" cy="{y0}" opacity="0"/></g>')
    hub = (f'<rect class="hub" x="{hub_x0}" y="{hub_y - 15}" width="{hub_x1 - hub_x0}" height="30" rx="15"/>'
           f'<text class="hublab" x="{hub_x0 - 12}" y="{hub_y + 4}" text-anchor="end">LEADER</text>'
           f'<text class="hublab" x="{hub_x1 + 12}" y="{hub_y + 4}">by fee →</text>')
    return f'{hub}<g class="links">{"".join(links)}</g><g class="queue">{"".join(queue)}</g>{"".join(bars)}'


def render_burst_cells(ex: dict) -> str:
    lo, hi = math.log10(1000), math.log10(5_000_000)
    cells = []
    for item in ex["items"]:
        h = 10 + 70 * (math.log10(item["price"]) - lo) / (hi - lo)
        tip = f'<b>Position {item["pos"]}</b> of {ex["txs"]}<br>{thousands(item["price"])} µlamports/CU<br>sent {item["sent"]} of {len(ex["items"])}'
        cells.append(
            f'<a class="cell" href="https://explorer.solana.com/tx/{item["sig"]}" data-tip="{esc(tip)}" '
            f'aria-label="Position {item["pos"]}, price {item["price"]}, sent {item["sent"]} of {len(ex["items"])}">'
            f'<div class="cellwrap"><span class="price" style="--h:{h:.0f}px"></span></div>'
            f'<div class="pos">#{item["pos"]}</div><div class="sent">{item["sent"]}<i>/{len(ex["items"])}</i></div></a>')
    return "\n".join(cells)


def render_clients(clients: list[dict]) -> str:
    out = []
    for c in clients:
        name = esc(c["client"])
        t1 = f'<b>{name}</b> · burst in before the window<br>{fmt(c["before"])} of {thousands(c["before_n"])} pairs higher fee first'
        t2 = f'<b>{name}</b> · arriving while it builds<br>{fmt(c["building"])} of {thousands(c["building_n"])} pairs higher fee first'
        out.append(
            f'<div class="crow"><div class="clab">{name}</div><div class="ctrack"><div class="cbars">'
            f'<div class="cbar b1" style="width:{c["before"] * 100:.1f}%" data-tip="{esc(t1)}"><span class="v">{fmt(c["before"])}</span></div>'
            f'<div class="cbar b2" style="width:{c["building"] * 100:.1f}%" data-tip="{esc(t2)}"><span class="v">{fmt(c["building"])}</span></div>'
            f'</div></div></div>')
    table = "".join(f'<tr><td>{esc(c["client"])}</td><td>{fmt(c["before"])}</td><td>{fmt(c["building"])}</td></tr>' for c in clients)
    out.append(f'<div class="sr"><table><tr><th>Client</th><th>Before the window</th><th>While building</th></tr>{table}</table></div>')
    return "\n".join(out)


def interval(v: float, lo: float, hi: float, alt: bool = False) -> str:
    return (f'<div class="ci"><span class="num">{fmt(v)}</span><span class="citrack{" alt" if alt else ""}" '
            f'data-tip="{fmt(v)} [{fmt(lo)}, {fmt(hi)}]"><span class="half"></span>'
            f'<span class="rng" style="left:{lo * 100:.1f}%;width:{(hi - lo) * 100:.1f}%"></span>'
            f'<span class="pt" style="left:{v * 100:.1f}%"></span></span></div>')


def render_verdicts(vals: list[dict]) -> str:
    out = []
    for v in vals:
        pill = "pill" if "then" in v["verdict"] else "pill green"
        out.append(
            f'<tr><td class="c-client">{esc(v["client"])}</td>'
            f'<td class="id"><a href="https://www.validators.app/validators/{v["id"]}?locale=en&amp;network=mainnet" title="{v["id"]}">{short(v["id"])}</a></td>'
            f'<td class="c-verdict"><span class="{pill}">{esc(v["verdict"])}</span></td>'
            f'<td class="c-ci" data-label="Higher price first">{interval(v["fee"], v["fee_lo"], v["fee_hi"])}</td>'
            f'<td class="c-ci" data-label="Sent earlier first">{interval(v["eq"], v["eq_lo"], v["eq_hi"], alt=True)}</td>'
            f'<td class="mono-num c-bursts" data-label="Bursts">{v["bursts"]}</td></tr>')
    return "\n".join(out)


def render_sessions(vals: list[dict]) -> tuple[str, str]:
    lo, hi = 0.5, 1.0
    x = lambda v: (v - lo) / (hi - lo) * 100  # noqa: E731
    grid = "".join(f'<div class="dgrid" style="left:{x(t):.1f}%"></div>' for t in (0.5, 0.6, 0.7, 0.8, 0.9, 1.0))
    out = []
    for v in vals:
        s1, s2 = v["sessions"].get("s1"), v["sessions"].get("s2")
        name = f'<b>{esc(v["client"])}</b> {short(v["id"])}'
        parts = [grid]
        usable1 = s1 is not None and s1[2] != "insufficient data"
        if usable1 and s2:
            parts.append(f'<div class="dline" style="left:{x(min(s1[0], s2[0])):.1f}%;width:{abs(x(s2[0]) - x(s1[0])):.1f}%"></div>')
        if usable1:
            parts.append(f'<div class="sdot day" style="left:{x(s1[0]):.1f}%" data-tip="{esc(name + "<br>midday: " + fmt(s1[0]) + f" · {s1[1]} bursts")}"></div>')
        if s2:
            note = "" if usable1 else "<br>midday: too little data"
            parts.append(f'<div class="sdot night" style="left:{x(s2[0]):.1f}%" data-tip="{esc(name + "<br>night: " + fmt(s2[0]) + f" · {s2[1]} bursts" + note)}"></div>')
        out.append(f'<div class="drow"><div class="dlab">{esc(v["client"])}<small>{short(v["id"])}</small></div><div class="dtrack">{"".join(parts)}</div></div>')
    ticks = "".join(f'<span style="left:{x(t):.1f}%">{t:.1f}</span>' for t in (0.5, 0.6, 0.7, 0.8, 0.9, 1.0))
    return "\n".join(out), ticks


def main() -> None:
    ex, clients, vals, tot = example_burst(), client_shares(), validators(), totals()
    sessions, ticks = render_sessions(vals)
    fills = {
        "PROBES": f'{tot["probes"]:,}', "VALIDATORS": str(tot["validators"]), "CLIENTS": str(tot["clients"]),
        "SESSIONS": str(tot["sessions"]), "SOL": f'{tot["sol"]:.4f}', **tile_tips(tot),
        "EX_SLOT": str(ex["slot"]), "EX_SLOT_SPACED": thousands(ex["slot"]), "EX_TXS": str(ex["txs"]),
        "EX_IN_ORDER": str(ex["in_order"]), "EX_PAIRS": str(ex["pairs"]), "EX_CLIENT": esc(ex["client"]),
        "EX_ID": short(ex["identity"]),
        "REPLAY": render_replay(ex), "BURST_CELLS": render_burst_cells(ex), "CLIENT_ROWS": render_clients(clients),
        "VERDICT_ROWS": render_verdicts(vals), "SESSION_ROWS": sessions, "SESSION_TICKS": ticks,
    }
    page = (ROOT / "site" / "template.html").read_text()
    for key, value in fills.items():
        page = page.replace("{{" + key + "}}", value)
    if "{{" in page:
        raise SystemExit("unfilled placeholder: " + page[page.index("{{"):page.index("{{") + 40])
    (ROOT / "docs" / "index.html").write_text(page)
    print(f"docs/index.html: {len(clients)} clients, {len(vals)} validators, example burst at slot {ex['slot']}")


if __name__ == "__main__":
    main()
