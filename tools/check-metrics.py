#!/usr/bin/env python3
"""Golden-metrics regression check for Marq.

Several bugs in this repo's history were regressions of an earlier fix: a table
header printing as "Manifes/t", then "Statu/s" once the first was fixed, then
header alignment. Each was found by a human looking at a screenshot, days later.
Each is a number this script compares.

    tools/check-metrics.py            # compare against tests/baselines/
    tools/check-metrics.py --bless    # record current output as the baseline

Baselines record only what should be stable: how many tables, how wide they sit
relative to the measure, what font scale print settled on, which columns break
words, and how many pages the export runs to. Not exact pixel heights, which
move with any typographic change and would make the check noise.
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MARQ = ROOT / ".build" / "debug" / "marq"
PDFTOOL = ROOT / ".build" / "debug" / "pdftool"
BASELINES = ROOT / "tests" / "baselines"
WORK = ROOT / ".harness" / "check"

FIXTURES = ["examples/test.md", "examples/anchor-test.md"]

# Screen metrics are only meaningful at a stated width — the whole table
# allocation is a function of the available measure.
WIDTH = 960

# How far a number may drift before it counts as a change. Percentages of the
# measure are stable to well under a point; font scale is solved iteratively and
# can settle a whisker differently.
TOLERANCE = {"fillPct": 0.5, "fontScale": 0.02, "maxTextWidthPt": 2.0}


def run(args, capture_json=False):
    result = subprocess.run(
        args, cwd=ROOT, capture_output=True, text=True, timeout=120)
    if result.returncode != 0:
        raise SystemExit(
            f"command failed ({result.returncode}): {' '.join(str(a) for a in args)}\n"
            f"{result.stderr[-2000:]}")
    return json.loads(result.stdout) if capture_json else result


def summarise_tables(metrics):
    """The stable shape of a metrics dump."""
    return {
        "tableCount": metrics["tableCount"],
        "headings": metrics["headings"],
        "problems": metrics["problems"],
        "tables": [
            {
                "index": t["index"],
                "columnCount": t["columnCount"],
                "rowCount": t["rowCount"],
                "fillPct": t["fillPct"],
                "fontScale": t["fontScale"],
                "overflows": t["overflows"],
                "brokenColumns": t["brokenColumns"],
                "headers": [c["header"] for c in t["columns"]],
            }
            for t in metrics["tables"]
        ],
    }


def measure(fixture):
    """Everything one fixture contributes: screen layout, print layout, export."""
    name = Path(fixture).stem
    WORK.mkdir(parents=True, exist_ok=True)
    screen = WORK / f"{name}.screen.json"
    printed = WORK / f"{name}.print.json"
    pdf = WORK / f"{name}.pdf"

    run([MARQ, fixture, "--width", str(WIDTH), "--dump-metrics", screen])
    run([MARQ, fixture, "--width", str(WIDTH), "--dump-metrics", printed, "--print"])
    run([MARQ, fixture, "--export-pdf", pdf])
    info = run([PDFTOOL, "info", pdf], capture_json=True)

    widths = [p.get("textWidthPt", 0) for p in info["pages"]]
    return {
        "screen": summarise_tables(json.loads(screen.read_text())),
        "print": summarise_tables(json.loads(printed.read_text())),
        "pdf": {
            "pageCount": info["pageCount"],
            "maxTextWidthPt": max(widths) if widths else 0,
        },
    }


def compare(path, expected, actual, out):
    """Walk two summaries in step, reporting only what moved."""
    if isinstance(expected, dict) and isinstance(actual, dict):
        for key in sorted(set(expected) | set(actual)):
            if key not in expected:
                out.append(f"{path}.{key}: added ({actual[key]!r})")
            elif key not in actual:
                out.append(f"{path}.{key}: removed (was {expected[key]!r})")
            else:
                compare(f"{path}.{key}", expected[key], actual[key], out)
    elif isinstance(expected, list) and isinstance(actual, list):
        if len(expected) != len(actual):
            out.append(f"{path}: {len(expected)} entries -> {len(actual)}")
        for i, (e, a) in enumerate(zip(expected, actual)):
            compare(f"{path}[{i}]", e, a, out)
    elif isinstance(expected, (int, float)) and isinstance(actual, (int, float)) \
            and not isinstance(expected, bool):
        tolerance = next(
            (v for k, v in TOLERANCE.items() if path.endswith(k)), 0)
        if abs(expected - actual) > tolerance:
            out.append(f"{path}: {expected} -> {actual}")
    elif expected != actual:
        out.append(f"{path}: {expected!r} -> {actual!r}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bless", action="store_true",
                        help="record current output as the baseline")
    parser.add_argument("fixtures", nargs="*", default=None)
    args = parser.parse_args()

    if not MARQ.exists() or not PDFTOOL.exists():
        raise SystemExit("run `swift build` first (or use `just check`)")

    BASELINES.mkdir(parents=True, exist_ok=True)
    failures = 0

    for fixture in args.fixtures or FIXTURES:
        name = Path(fixture).stem
        baseline = BASELINES / f"{name}.json"
        actual = measure(fixture)

        if args.bless:
            baseline.write_text(json.dumps(actual, indent=2, sort_keys=True) + "\n")
            print(f"blessed {baseline.relative_to(ROOT)}")
            continue

        if not baseline.exists():
            print(f"MISSING {fixture}: no baseline — run `just bless`")
            failures += 1
            continue

        diffs = []
        compare(name, json.loads(baseline.read_text()), actual, diffs)
        if diffs:
            failures += 1
            print(f"CHANGED {fixture}")
            for d in diffs:
                print(f"    {d}")
        else:
            print(f"ok      {fixture}")

    if failures:
        print(f"\n{failures} fixture(s) changed. If the change is intended, "
              f"run `just bless` and commit the new baselines.")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
