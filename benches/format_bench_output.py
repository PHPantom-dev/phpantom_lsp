#!/usr/bin/env python3
"""Convert Criterion bencher-format output (stdin) to customSmallerIsBetter JSON (stdout).

Parses lines like:
    test cold_start_completion ... bench:   2610870 ns/iter (+/- 10235)

and emits a JSON array with nanosecond values converted to milliseconds:
    [{"name": "cold_start_completion", "unit": "ms", "value": 2.611, "range": "± 0.010"}, ...]

The name and the measurement are matched separately rather than as one
line, because Criterion prints the `test <name> ... ` prefix before it
runs the benchmark and the `bench: ...` value after. Anything it writes
to stdout in between (a warning about a missing baseline, say) lands in
the middle and splits the line in two.

Exits non-zero when nothing parsed, so a benchmark run that died before
reporting fails here instead of silently handing an empty array to the
benchmark-tracking action.
"""

import json
import re
import sys

_NAME_RE = re.compile(r"test\s+(?P<name>\S+)\s+\.\.\.")
_BENCH_RE = re.compile(r"bench:\s+(?P<value>\d+)\s+ns/iter\s+\(\+/-\s+(?P<range>\d+)\)")

# Criterion itself never colours the bencher reporter, but the output can
# still pick up escape sequences from whatever is sharing the pipe.
_ANSI_RE = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")

NS_PER_MS = 1_000_000

# How many raw lines to echo back when nothing parsed.
_PREVIEW_LINES = 20


def main() -> None:
    results = []
    seen = []
    pending = None
    unnamed = 0
    unreported = []

    for line in sys.stdin:
        line = _ANSI_RE.sub("", line).replace("\r", "").strip()
        if line:
            seen.append(line)

        name = _NAME_RE.search(line)
        if name:
            if pending is not None:
                # The previous benchmark announced itself but never
                # reported a measurement.
                unreported.append(pending)
            pending = name.group("name")

        measurement = _BENCH_RE.search(line)
        if not measurement:
            continue
        if pending is None:
            unnamed += 1
            continue

        value_ms = round(int(measurement.group("value")) / NS_PER_MS, 3)
        range_ms = round(int(measurement.group("range")) / NS_PER_MS, 3)
        results.append(
            {
                "name": pending,
                "unit": "ms",
                "value": value_ms,
                "range": f"± {range_ms:.3f}",
            }
        )
        pending = None

    if pending is not None:
        unreported.append(pending)

    for name in unreported:
        print(f"warning: {name} reported no measurement", file=sys.stderr)
    if unnamed:
        # A measurement with no preceding name means the output shape
        # changed and the pairing above no longer holds.
        sys.exit(f"{unnamed} measurement(s) could not be matched to a benchmark name")

    if not results:
        preview = "\n".join(f"  | {line}" for line in seen[:_PREVIEW_LINES])
        sys.exit(
            f"no benchmark results parsed from {len(seen)} non-empty input "
            "lines -- the benchmark run probably failed to build or crashed "
            f"before reporting. First lines read:\n{preview}"
        )

    json.dump(results, sys.stdout, indent=2)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
