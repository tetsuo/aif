#!/usr/bin/env python3
"""Compare CLI throughput and verify identical output on generated JSON lines."""

import argparse
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parent.parent
CASES = {
    "bare": "needle",
    "equality": "state = open",
    "numeric": "priority > 50 AND score < 3.5",
    "nested": "user.settings.theme = dark",
    "list": "tags:beta",
    "regex": 'msg =~ "[a-z]+ needle[0-9]+$"',
    "regex_group": 'msg =~ ("[a-z]+ needle[0-9]+$" OR "^skip")',
}


def digest(binary, expression, data):
    result = subprocess.run(
        [str(binary), expression, str(data)], capture_output=True, check=True
    )
    return hashlib.sha256(result.stdout).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--records", type=int, default=100_000)
    parser.add_argument("--runs", type=int, default=5)
    args = parser.parse_args()
    if args.records <= 0 or args.runs <= 0:
        parser.error("records and runs must be positive")
    binaries = [args.baseline.resolve(), args.candidate.resolve()]
    data = ROOT / "target" / "benchmarks" / "records.jsonl"
    data.parent.mkdir(parents=True, exist_ok=True)
    with data.open("w") as output:
        for i in range(args.records):
            record = {
                "id": i,
                "msg": f"hello needle{i}" if i % 5 == 0 else "skip this record",
                "state": "open" if i % 2 else "closed",
                "priority": i % 100,
                "score": (i % 10) / 2,
                "user": {"settings": {"theme": "dark" if i % 3 else "light"}},
                "tags": ["stable", "beta" if i % 4 else "alpha"],
            }
            output.write(json.dumps(record, separators=(",", ":")) + "\n")
    print(f"{args.records:,} records; {data.stat().st_size:,} bytes; median of {args.runs} runs")
    print("case          baseline(s) candidate(s) speedup")
    for label, expression in CASES.items():
        assert digest(binaries[0], expression, data) == digest(binaries[1], expression, data), label
        samples = [[], []]
        for run in range(args.runs):
            for index in ([0, 1] if run % 2 == 0 else [1, 0]):
                start = time.perf_counter()
                subprocess.run(
                    [str(binaries[index]), expression, str(data)],
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.PIPE,
                    check=True,
                )
                samples[index].append(time.perf_counter() - start)
        before, after = (statistics.median(sample) for sample in samples)
        print(f"{label:13} {before:11.4f} {after:12.4f} {before / after:6.2f}x")
    for binary in binaries:
        print(f"{binary.name}: {binary.stat().st_size:,} bytes")


if __name__ == "__main__":
    main()
