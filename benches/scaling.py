#!/usr/bin/env python3
"""Check that analysis cost grows in proportion to input size.

Every performance problem users have hit so far was a growth problem on
an input of an unusual shape, which a benchmark over fixed fixtures
cannot see. This script generates each known pathological shape at N and
2N, runs ``phpantom_lsp analyze`` on both, and fails when doubling the
input multiplies the time by more than ``--max-ratio``.

The fixed cost of a run (start-up, loading stubs) is measured on a
trivial project and subtracted from both timings, so the ratio reflects
only the work the shape adds. A ratio does not depend on how fast the
machine is, so the check can run on a shared CI runner.

When a growth problem on a new shape is fixed, add that shape to
``SHAPES`` so it cannot come back.

Usage (CI)::

    python3 benches/scaling.py --binary target/release/phpantom_lsp

Usage (local, after ``cargo build --release``)::

    python3 benches/scaling.py [--shape switch_literals]
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from typing import Callable

# Doubling the input of a linear shape doubles its time. Allow headroom
# for noise and for genuinely O(n log n) work, but not for O(n²), which
# quadruples it.
DEFAULT_MAX_RATIO = 2.5

# Each size is timed this many times and the fastest run kept, which
# filters out a busy runner better than the mean does.
DEFAULT_RUNS = 3

# Below this, timing noise and the subtracted fixed cost swamp the ratio.
MIN_MEASURABLE = 0.05

# A single analysis that runs longer than this has blown up far past any
# ratio worth reporting.
RUN_TIMEOUT = 300

# ── Shapes ───────────────────────────────────────────────────────────────────
#
# Each generator takes N and returns the source of one PHP file.


def long_script(n: int) -> str:
    """A legacy top-level script of N repeated 11-line blocks."""
    out = ["<?php\n", "function helper(int $i): string { return (string) $i; }\n"]
    for i in range(n):
        out.append(
            f"$v{i} = helper({i});\n"
            f"if ($v{i} === '0') {{ $r{i} = 1; }} elseif ($v{i} === '1') {{ $r{i} = 2; }} else {{ $r{i} = 3; }}\n"
            f"switch ($r{i}) {{\n"
            f"    case 1: $s{i} = 'a'; break;\n"
            f"    default: $s{i} = 'b';\n"
            f"}}\n"
            f"try {{ $t{i} = strlen($s{i}); }} catch (\\Exception $e) {{ $t{i} = 0; }}\n"
            f"while ($t{i} > 0) {{ $t{i}--; }}\n"
            f"$c{i} = function ($x) use ($s{i}) {{ return $x . $s{i}; }};\n"
            f"echo strtoupper($s{i}), helper($t{i});\n"
            f"echo strlen($s{i});\n"
        )
    return "".join(out)


def long_script_closure_calls(n: int) -> str:
    """A top-level script of N blocks, each calling a closure with a typed
    parameter through the variable that holds it."""
    out = ["<?php\n", "class Foo { public function name(): string { return ''; } }\n"]
    for i in range(n):
        out.append(
            f"$a{i} = new Foo();\n"
            f"$f{i} = function (Foo $p) {{ return $p->name(); }};\n"
            f"echo $f{i}($a{i});\n"
        )
    return "".join(out)


def switch_literals(n: int) -> str:
    """A ``switch`` that assigns a different literal in each of N cases."""
    out = [
        "<?php\n",
        "function takesInt(int $x): void {}\n",
        "function countryName(string $code): void {\n",
        "    switch ($code) {\n",
    ]
    for i in range(n):
        out.append(f"        case 'C{i}': $name = 'Country {i}'; break;\n")
    out.append("        default: $name = 'Unknown';\n    }\n    takesInt($name);\n}\n")
    return "".join(out)


def elseif_literals(n: int) -> str:
    """The same lookup table as an ``if``/``elseif`` chain of N arms."""
    out = [
        "<?php\n",
        "function takesInt(int $x): void {}\n",
        "function countryName(string $code): void {\n",
    ]
    for i in range(n):
        keyword = "if" if i == 0 else "} elseif"
        out.append(f"    {keyword} ($code === 'C{i}') {{ $name = 'Country {i}';\n")
    out.append("    } else { $name = 'Unknown'; }\n    takesInt($name);\n}\n")
    return "".join(out)


def conditional_array_writes(n: int) -> str:
    """An array filled in one literal key at a time over N ``if``s."""
    out = [
        "<?php\n",
        "function takesInt(int $x): void {}\n",
        "function build(bool $flag): void {\n",
        "    $data = [];\n",
    ]
    for i in range(n):
        out.append(f"    if ($flag) {{ $data['k{i}'] = {i}; }}\n")
    out.append("    takesInt($data);\n}\n")
    return "".join(out)


def conditional_dynamic_writes(n: int) -> str:
    """N ``if``s each writing a different shape under the same dynamic key."""
    out = [
        "<?php\n",
        "function takesInt(int $x): void {}\n",
        "function build(bool $flag, int $id): void {\n",
        "    $data = [];\n",
    ]
    for i in range(n):
        out.append(f"    if ($flag) {{ $data[$id] = ['v{i}' => {i}]; }}\n")
    out.append("    takesInt($data);\n}\n")
    return "".join(out)


def undefined_functions(n: int) -> str:
    """N calls to functions that are declared nowhere."""
    out = ["<?php\n", "function run(string $s): void {\n"]
    for i in range(n):
        out.append(f"    missing_helper_{i}($s);\n")
    out.append("}\n")
    return "".join(out)


def method_chain(n: int) -> str:
    """A generated fluent chain of N links."""
    return (
        "<?php\n"
        "class Fluent {\n"
        "    /** @return static */\n"
        "    public function self(): static { return $this; }\n"
        "    public function finish(): int { return 0; }\n"
        "}\n"
        "function run(Fluent $start): void {\n"
        "    $out = $start" + "->self()" * n + ";\n"
        "    $out->finish();\n"
        "}\n"
    )


@dataclass(frozen=True)
class Shape:
    generate: Callable[[int], str]
    # Chosen so that N costs a few hundred milliseconds on a release
    # build: enough to stand clear of timing noise, little enough that
    # the whole check stays around a minute.
    n: int
    # A shape that still grows faster than its input is measured and
    # reported, but does not fail the check. Once it passes, drop the
    # flag (and re-tune N) so it cannot regress.
    known_slow: bool = False


SHAPES: dict[str, Shape] = {
    "long_script": Shape(long_script, 2000),
    "undefined_functions": Shape(undefined_functions, 30000),
    "long_script_closure_calls": Shape(long_script_closure_calls, 8000),
    "switch_literals": Shape(switch_literals, 16000),
    "elseif_literals": Shape(elseif_literals, 4000),
    "conditional_array_writes": Shape(conditional_array_writes, 200, known_slow=True),
    "conditional_dynamic_writes": Shape(conditional_dynamic_writes, 2000),
    "method_chain": Shape(method_chain, 600, known_slow=True),
}

BASELINE_PHP = "<?php\necho 'hello';\n"

# ── Measurement ──────────────────────────────────────────────────────────────


def time_analyze(binary: str, project_dir: str) -> float:
    """Return the wall-clock time of one analysis, in seconds."""
    start = time.perf_counter()
    proc = subprocess.run(
        [
            binary,
            "analyze",
            "--project-root",
            project_dir,
            "--format",
            "json",
            "--no-colour",
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        timeout=RUN_TIMEOUT,
    )
    elapsed = time.perf_counter() - start
    # 0 is a clean run and 1 means diagnostics were reported; anything
    # else (a panic, a signal) means the timing is not of a finished
    # analysis.
    if proc.returncode not in (0, 1):
        sys.stderr.write(proc.stderr.decode(errors="replace"))
        raise RuntimeError(f"analyze exited with {proc.returncode} on {project_dir}")
    return elapsed


def time_pair(binary: str, small_dir: str, large_dir: str, runs: int) -> tuple[float, float]:
    """Return the fastest time of each project over ``runs`` rounds.

    The two are timed in alternation, so a burst of load on a shared
    runner slows both rather than skewing the ratio between them.
    """
    small = large = float("inf")
    for _ in range(runs):
        small = min(small, time_analyze(binary, small_dir))
        large = min(large, time_analyze(binary, large_dir))
    return small, large


def write_project(work_dir: str, name: str, source: str) -> str:
    project_dir = os.path.join(work_dir, name)
    os.makedirs(project_dir, exist_ok=True)
    with open(os.path.join(project_dir, "shape.php"), "w") as f:
        f.write(source)
    return project_dir


def measure(
    binary: str, small_dir: str, large_dir: str, runs: int, overhead: float
) -> tuple[float, float, float]:
    """Return the cost of N and 2N above the fixed cost, and their ratio."""
    small, large = time_pair(binary, small_dir, large_dir, runs)
    small = max(small - overhead, 0.0)
    large = max(large - overhead, 0.0)
    return small, large, large / small if small > 0 else float("inf")


def find_binary() -> str | None:
    candidate = "target/release/phpantom_lsp"
    if os.path.isfile(candidate) and os.access(candidate, os.X_OK):
        return candidate
    return shutil.which("phpantom_lsp")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Check that analysis cost grows linearly with input size.",
    )
    parser.add_argument(
        "--binary",
        default=None,
        help="Path to the phpantom_lsp binary. Auto-detected if omitted.",
    )
    parser.add_argument(
        "--shape",
        choices=sorted(SHAPES),
        action="append",
        help="Shape to check (repeatable). Defaults to every shape.",
    )
    parser.add_argument(
        "--scale",
        type=float,
        default=1.0,
        help="Multiply every shape's N by this factor (default: 1).",
    )
    parser.add_argument(
        "--runs",
        type=int,
        default=DEFAULT_RUNS,
        help=f"Timed runs per size; the fastest is kept (default: {DEFAULT_RUNS}).",
    )
    parser.add_argument(
        "--max-ratio",
        type=float,
        default=DEFAULT_MAX_RATIO,
        help=f"Largest allowed time ratio for 2N over N (default: {DEFAULT_MAX_RATIO}).",
    )
    args = parser.parse_args()

    binary = args.binary or find_binary()
    if binary is None:
        sys.exit("phpantom_lsp binary not found; build it or pass --binary")

    shapes = args.shape or list(SHAPES)
    failures = []
    now_linear = []

    with tempfile.TemporaryDirectory(prefix="phpantom-scaling-") as work_dir:
        baseline_dir = write_project(work_dir, "baseline", BASELINE_PHP)
        overhead = min(time_analyze(binary, baseline_dir) for _ in range(args.runs))
        print(f"fixed cost per run: {overhead:.3f}s\n")
        print(f"{'shape':<28} {'N':>6} {'t(N)':>9} {'t(2N)':>9} {'ratio':>7}")

        for name in shapes:
            shape = SHAPES[name]
            n = max(1, round(shape.n * args.scale))
            small_dir = write_project(work_dir, f"{name}_n", shape.generate(n))
            large_dir = write_project(work_dir, f"{name}_2n", shape.generate(2 * n))

            small, large, ratio = measure(binary, small_dir, large_dir, args.runs, overhead)
            grew = ratio > args.max_ratio
            if grew and not shape.known_slow:
                # A real growth problem shows again; a noisy round rarely does.
                small, large, ratio = measure(binary, small_dir, large_dir, args.runs, overhead)
                grew = ratio > args.max_ratio
            if shape.known_slow:
                mark = "  known slow" if grew else "  known slow, now passes"
                if not grew:
                    now_linear.append(name)
            else:
                mark = "  FAIL" if grew else ""
                if grew:
                    failures.append(name)
            if small < MIN_MEASURABLE:
                mark += "  (too fast to measure reliably, raise N)"
            print(f"{name:<28} {n:>6} {small:>8.3f}s {large:>8.3f}s {ratio:>7.2f}{mark}")

    if now_linear:
        print(
            f"\nNo longer growing faster than their input: {', '.join(now_linear)}. "
            "Drop their known_slow flag so the check holds them to it."
        )
    if failures:
        sys.exit(
            f"\nDoubling the input more than {args.max_ratio}x the analysis "
            f"time for: {', '.join(failures)}. Some per-element work now "
            "grows with the size of the input."
        )


if __name__ == "__main__":
    main()
