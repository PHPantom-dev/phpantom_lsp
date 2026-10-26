#!/usr/bin/env python3
"""Prepare, run, and combine balanced Criterion shards for PR comparisons.

Build once with ``cargo bench --no-run --message-format=json`` and pipe its
output to ``prepare``. Each runner extracts the resulting binaries.tar in
the build directory and runs its shard from the repository root, where
the benchmark fixtures live. Only complete results reach the comparison.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
from typing import TextIO


def _write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def prepare(build_dir: Path, shards: int, stream: TextIO) -> None:
    """Package Cargo's benchmark executables and assign every case once."""
    if shards < 1:
        raise ValueError("shard count must be positive")

    executables = {}
    for line in stream:
        record = json.loads(line)
        if (
            record.get("reason") == "compiler-artifact"
            and "bench" in record["target"]["kind"]
            and record.get("executable")
        ):
            target = record["target"]["name"]
            if target in executables:
                raise ValueError(f"duplicate benchmark target: {target}")
            executables[target] = Path(record["executable"])
    if not executables:
        raise ValueError("Cargo reported no benchmark executables")

    binary_dir = build_dir / "bin"
    binary_dir.mkdir(parents=True, exist_ok=True)
    manifest = {"shards": shards, "executables": {}, "cases": []}
    seen = set()
    for target, source in sorted(executables.items()):
        relative = f"bin/{target}"
        binary = build_dir / relative
        shutil.copy2(source, binary)
        manifest["executables"][target] = relative
        listing = subprocess.run(
            [str(binary.resolve()), "--list", "--format", "terse"],
            check=True,
            stdout=subprocess.PIPE,
            text=True,
        ).stdout
        names = sorted(
            line.removesuffix(": benchmark")
            for line in listing.splitlines()
            if line.endswith(": benchmark")
        )
        if not names:
            raise ValueError(f"{target} listed no benchmark cases")
        for name in names:
            if not name or name in seen:
                raise ValueError(f"empty or duplicate benchmark name: {name!r}")
            seen.add(name)
            manifest["cases"].append(
                {
                    "target": target,
                    "name": name,
                    "shard": len(manifest["cases"]) % shards + 1,
                }
            )
    if shards > len(manifest["cases"]):
        raise ValueError("shard count exceeds the number of benchmark cases")

    _write_json(build_dir / "manifest.json", manifest)
    with tarfile.open(build_dir / "binaries.tar", "w") as archive:
        for relative in manifest["executables"].values():
            archive.add(build_dir / relative, arcname=relative)


def _validate_results(results: object, expected: list[str], source: str) -> None:
    if not isinstance(results, list) or any(
        not isinstance(result, dict) or not isinstance(result.get("name"), str)
        for result in results
    ):
        raise ValueError(f"{source}: expected a list of named benchmark results")
    names = [result["name"] for result in results]
    if sorted(names) != sorted(expected):
        raise ValueError(
            f"{source}: benchmark results are missing, duplicated, or unexpected; "
            f"expected {sorted(expected)!r}, got {sorted(names)!r}"
        )


def run_shard(build_dir: Path, shard: int, output: Path) -> None:
    """Measure one shard, keeping Criterion's normal sampling settings."""
    manifest = json.loads((build_dir / "manifest.json").read_text(encoding="utf-8"))
    if not 1 <= shard <= manifest["shards"]:
        raise ValueError(f"shard must be between 1 and {manifest['shards']}")
    cases = [case for case in manifest["cases"] if case["shard"] == shard]
    if not cases:
        raise ValueError(f"shard {shard} has no benchmark cases")

    results = []
    # Comparison history lives on gh-pages, not in a local Criterion baseline.
    with tempfile.TemporaryDirectory(prefix="phpantom-bench-") as criterion_home:
        env = dict(os.environ, CRITERION_HOME=criterion_home)
        for target, relative in manifest["executables"].items():
            names = [case["name"] for case in cases if case["target"] == target]
            if not names:
                continue
            pattern = "^(?:" + "|".join(re.escape(name) for name in names) + ")$"
            process = subprocess.run(
                [
                    str((build_dir / relative).resolve()),
                    "--bench",
                    "--output-format",
                    "bencher",
                    "--noplot",
                    pattern,
                ],
                stdout=subprocess.PIPE,
                text=True,
                env=env,
            )
            # Keep partial output visible when a benchmark crashes.
            print(process.stdout, end="", flush=True)
            process.check_returncode()
            formatted = subprocess.run(
                [sys.executable, str(Path(__file__).with_name("format_bench_output.py"))],
                input=process.stdout,
                check=True,
                stdout=subprocess.PIPE,
                text=True,
            )
            measured = json.loads(formatted.stdout)
            _validate_results(measured, names, target)
            results.extend(measured)
    _validate_results(results, [case["name"] for case in cases], f"shard {shard}")
    _write_json(output, results)


def merge(build_dir: Path, reports_dir: Path, output: Path) -> None:
    """Require every planned measurement before producing one comparison."""
    manifest = json.loads((build_dir / "manifest.json").read_text(encoding="utf-8"))
    expected_files = {
        f"shard-{shard}.json" for shard in range(1, manifest["shards"] + 1)
    }
    actual_files = {path.name for path in reports_dir.glob("*.json")}
    if actual_files != expected_files:
        raise ValueError(
            f"expected reports {sorted(expected_files)!r}, got {sorted(actual_files)!r}"
        )

    results = []
    for shard in range(1, manifest["shards"] + 1):
        report = reports_dir / f"shard-{shard}.json"
        measured = json.loads(report.read_text(encoding="utf-8"))
        expected = [
            case["name"] for case in manifest["cases"] if case["shard"] == shard
        ]
        _validate_results(measured, expected, report.name)
        results.extend(measured)
    _validate_results(
        results, [case["name"] for case in manifest["cases"]], "combined results"
    )
    by_name = {result["name"]: result for result in results}
    _write_json(output, [by_name[case["name"]] for case in manifest["cases"]])


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prepare_parser = commands.add_parser("prepare")
    prepare_parser.add_argument("--build-dir", type=Path, required=True)
    prepare_parser.add_argument("--shards", type=int, required=True)
    run_parser = commands.add_parser("run")
    run_parser.add_argument("--build-dir", type=Path, required=True)
    run_parser.add_argument("--shard", type=int, required=True)
    run_parser.add_argument("--output", type=Path, required=True)
    merge_parser = commands.add_parser("merge")
    merge_parser.add_argument("--build-dir", type=Path, required=True)
    merge_parser.add_argument("--reports-dir", type=Path, required=True)
    merge_parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "prepare":
            prepare(args.build_dir, args.shards, sys.stdin)
        elif args.command == "run":
            run_shard(args.build_dir, args.shard, args.output)
        else:
            merge(args.build_dir, args.reports_dir, args.output)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()
