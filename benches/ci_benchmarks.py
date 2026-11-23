#!/usr/bin/env python3
"""Prepare, run, and combine balanced Criterion shards for PR comparisons.

Build with ``cargo bench --no-run --message-format=json`` and pipe its output
to ``prepare``. A prepared baseline can be included, or independently built
manifests joined with ``pair``, so each shard measures both revisions on the
same runner. Each revision uses its own fixtures; only complete results reach
the comparison.
"""

from __future__ import annotations

import argparse
import json
import math
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


def _pair_manifest(manifest: dict, baseline: dict) -> None:
    baseline_cases = {}
    for case in baseline["cases"]:
        name = case["name"]
        if not name or name in baseline_cases:
            raise ValueError(f"empty or duplicate baseline benchmark name: {name!r}")
        if case["target"] not in baseline["executables"]:
            raise ValueError(f"missing baseline executable for {name!r}")
        baseline_cases[name] = case["target"]

    manifest["baseline_executables"] = {
        target: f"baseline-bin/{target}" for target in baseline["executables"]
    }
    for case in manifest["cases"]:
        case.pop("baseline_target", None)
        if case["name"] in baseline_cases:
            case["baseline_target"] = baseline_cases[case["name"]]
    manifest["removed"] = sorted(set(baseline_cases) - {case["name"] for case in manifest["cases"]})


def pair(build_dir: Path, baseline_dir: Path) -> None:
    """Pair independently built manifests; extract baseline binaries into baseline-bin."""
    if baseline_dir.resolve() == build_dir.resolve():
        raise ValueError("baseline and head build directories must be distinct")
    manifest = json.loads((build_dir / "manifest.json").read_text(encoding="utf-8"))
    baseline = json.loads((baseline_dir / "manifest.json").read_text(encoding="utf-8"))
    _pair_manifest(manifest, baseline)
    _write_json(build_dir / "manifest.json", manifest)


def partition(build_dir: Path, shards: int) -> None:
    """Assign cached executables to the requested number of measurement runners."""
    manifest = json.loads((build_dir / "manifest.json").read_text(encoding="utf-8"))
    if not 1 <= shards <= len(manifest["cases"]):
        raise ValueError("shard count must be positive and cannot exceed benchmark cases")
    manifest["shards"] = shards
    for index, case in enumerate(manifest["cases"]):
        case["shard"] = index % shards + 1
    _write_json(build_dir / "manifest.json", manifest)


def prepare(
    build_dir: Path,
    shards: int,
    stream: TextIO,
    baseline_dir: Path | None = None,
) -> None:
    """Package Cargo's benchmark executables and assign every case once."""
    if shards < 1:
        raise ValueError("shard count must be positive")
    if baseline_dir is not None and baseline_dir.resolve() == build_dir.resolve():
        raise ValueError("baseline and head build directories must be distinct")

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

    if baseline_dir is not None:
        baseline = json.loads((baseline_dir / "manifest.json").read_text(encoding="utf-8"))
        _pair_manifest(manifest, baseline)
        (build_dir / "baseline-bin").mkdir(parents=True, exist_ok=True)
        for target, source in baseline["executables"].items():
            relative = manifest["baseline_executables"][target]
            shutil.copy2(baseline_dir / source, build_dir / relative)

    _write_json(build_dir / "manifest.json", manifest)
    with tarfile.open(build_dir / "binaries.tar", "w") as archive:
        paths = list(manifest["executables"].values())
        paths.extend(manifest.get("baseline_executables", {}).values())
        for relative in paths:
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


def _measure(binary: Path, names: list[str], source: Path, label: str) -> list[dict]:
    pattern = "^(?:" + "|".join(re.escape(name) for name in names) + ")$"
    with tempfile.TemporaryDirectory(prefix="phpantom-bench-") as criterion_home:
        process = subprocess.run(
            [
                str(binary.resolve()),
                "--bench",
                "--output-format",
                "bencher",
                "--noplot",
                pattern,
            ],
            cwd=source,
            stdout=subprocess.PIPE,
            text=True,
            env=dict(os.environ, CRITERION_HOME=criterion_home),
        )
    # Keep partial output visible when a benchmark crashes.
    print(process.stdout, end="", flush=True)
    process.check_returncode()
    formatted = subprocess.run(
        [sys.executable, str(Path(__file__).resolve().with_name("format_bench_output.py"))],
        input=process.stdout,
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    measured = json.loads(formatted.stdout)
    _validate_results(measured, names, label)
    return measured


def _validate_pair(results: object, cases: list[dict], source: str) -> None:
    if not isinstance(results, dict):
        raise ValueError(f"{source}: expected paired base and head results")
    _validate_results(results.get("head"), [case["name"] for case in cases], f"{source} head")
    _validate_results(
        results.get("base"),
        [case["name"] for case in cases if "baseline_target" in case],
        f"{source} base",
    )


def run_shard(
    build_dir: Path,
    shard: int,
    output: Path,
    baseline_source: Path | None = None,
) -> None:
    """Measure one shard, keeping Criterion's normal sampling settings."""
    manifest = json.loads((build_dir / "manifest.json").read_text(encoding="utf-8"))
    if not 1 <= shard <= manifest["shards"]:
        raise ValueError(f"shard must be between 1 and {manifest['shards']}")
    cases = [case for case in manifest["cases"] if case["shard"] == shard]
    if not cases:
        raise ValueError(f"shard {shard} has no benchmark cases")

    paired = "baseline_executables" in manifest
    if paired and (baseline_source is None or not baseline_source.is_dir()):
        raise ValueError("paired measurements require --baseline-source pointing to its checkout")
    head_results = []
    base_results = []
    for target, relative in manifest["executables"].items():
        target_cases = [case for case in cases if case["target"] == target]
        if not target_cases:
            continue
        # Assign shared cases by the head manifest, even when a case moved
        # between executables. Independent partitioning would compare machines.
        baseline_targets = {}
        for case in target_cases:
            if "baseline_target" in case:
                baseline_targets.setdefault(case["baseline_target"], []).append(case["name"])
        for baseline_target, names in baseline_targets.items():
            base_results.extend(
                _measure(
                    build_dir / manifest["baseline_executables"][baseline_target],
                    names,
                    baseline_source,
                    f"base {baseline_target}",
                )
            )
        head_results.extend(
            _measure(
                build_dir / relative,
                [case["name"] for case in target_cases],
                Path.cwd(),
                f"head {target}",
            )
        )
    if paired:
        results = {"base": base_results, "head": head_results}
        _validate_pair(results, cases, f"shard {shard}")
    else:
        results = head_results
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

    paired = "baseline_executables" in manifest
    results = {"base": [], "head": []} if paired else []
    for shard in range(1, manifest["shards"] + 1):
        report = reports_dir / f"shard-{shard}.json"
        measured = json.loads(report.read_text(encoding="utf-8"))
        cases = [case for case in manifest["cases"] if case["shard"] == shard]
        if paired:
            _validate_pair(measured, cases, report.name)
            for revision in ("base", "head"):
                results[revision].extend(measured[revision])
        else:
            _validate_results(measured, [case["name"] for case in cases], report.name)
            results.extend(measured)
    if paired:
        _validate_pair(results, manifest["cases"], "combined results")
        for revision in ("base", "head"):
            by_name = {result["name"]: result for result in results[revision]}
            results[revision] = [
                by_name[case["name"]] for case in manifest["cases"] if case["name"] in by_name
            ]
        results["removed"] = manifest["removed"]
        _write_json(output, results)
        return
    _validate_results(
        results, [case["name"] for case in manifest["cases"]], "combined results"
    )
    by_name = {result["name"]: result for result in results}
    _write_json(output, [by_name[case["name"]] for case in manifest["cases"]])


def _validate_measurements(results: list[dict], source: str) -> None:
    for result in results:
        value = result.get("value")
        if (
            type(value) not in (int, float)
            or not math.isfinite(value)
            or value <= 0
            or result.get("unit") != "ms"
        ):
            raise ValueError(f"{source}: {result['name']!r} must have a finite positive value in ms")


def _code(text: str) -> str:
    text = text.replace("|", r"\|").replace("\r", " ").replace("\n", " ")
    delimiter = "`" * (1 + max((len(run) for run in re.findall(r"`+", text)), default=0))
    return f"{delimiter} {text} {delimiter}"


def compare(
    build_dir: Path,
    results_path: Path,
    summary_path: Path,
    threshold: float = 1.3,
    base_revision: str | None = None,
    head_revision: str | None = None,
) -> None:
    """Compare measurements paired on one runner, preserving the alert threshold."""
    if type(threshold) not in (int, float) or not math.isfinite(threshold) or threshold < 1:
        raise ValueError("threshold must be finite and at least 1")
    manifest = json.loads((build_dir / "manifest.json").read_text(encoding="utf-8"))
    if "baseline_executables" not in manifest:
        raise ValueError("comparison requires a paired benchmark manifest")
    results = json.loads(results_path.read_text(encoding="utf-8"))
    _validate_pair(results, manifest["cases"], "comparison")
    if results.get("removed") != manifest["removed"]:
        raise ValueError("comparison: removed benchmarks do not match the manifest")
    for revision in ("base", "head"):
        _validate_measurements(results[revision], revision)
    base = {result["name"]: result for result in results["base"]}
    head = {result["name"]: result for result in results["head"]}

    lines = ["## Benchmark comparison", ""]
    if base_revision:
        lines.append(f"Base: {_code(base_revision)}")
    if head_revision:
        lines.append(f"Head: {_code(head_revision)}")
    lines.extend([
        "",
        "Each base/head pair was measured on the same runner with normal Criterion sampling.",
        f"A head/base ratio above {threshold:.2f} fails the comparison.",
        "",
        "| Benchmark | Base (ms) | Head (ms) | Head / base |",
        "|---|---:|---:|---:|",
    ])
    regressions = []
    for case in manifest["cases"]:
        name = case["name"]
        current = head[name]["value"]
        if name not in base:
            lines.append(f"| {_code(name)} | — | {current:.3f} | New |")
            continue
        previous = base[name]["value"]
        ratio = current / previous
        lines.append(f"| {_code(name)} | {previous:.3f} | {current:.3f} | {ratio:.3f} |")
        if ratio > threshold:
            regressions.append(name)
    lines.extend(["", "Removed benchmarks:", ""])
    lines.extend(f"- {_code(name)}" for name in manifest["removed"])
    if not manifest["removed"]:
        lines.append("None.")
    lines.extend(["", f"Regressions: {len(regressions)}.", ""])
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    summary_path.write_text("\n".join(lines), encoding="utf-8")
    if regressions:
        raise ValueError(f"benchmark regressions above {threshold:.2f}: {', '.join(regressions)}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prepare_parser = commands.add_parser("prepare")
    prepare_parser.add_argument("--build-dir", type=Path, required=True)
    prepare_parser.add_argument("--shards", type=int, required=True)
    prepare_parser.add_argument("--baseline-dir", type=Path)
    pair_parser = commands.add_parser("pair")
    pair_parser.add_argument("--build-dir", type=Path, required=True)
    pair_parser.add_argument("--baseline-dir", type=Path, required=True)
    partition_parser = commands.add_parser("partition")
    partition_parser.add_argument("--build-dir", type=Path, required=True)
    partition_parser.add_argument("--shards", type=int, required=True)
    run_parser = commands.add_parser("run")
    run_parser.add_argument("--build-dir", type=Path, required=True)
    run_parser.add_argument("--shard", type=int, required=True)
    run_parser.add_argument("--output", type=Path, required=True)
    run_parser.add_argument("--baseline-source", type=Path)
    merge_parser = commands.add_parser("merge")
    merge_parser.add_argument("--build-dir", type=Path, required=True)
    merge_parser.add_argument("--reports-dir", type=Path, required=True)
    merge_parser.add_argument("--output", type=Path, required=True)
    compare_parser = commands.add_parser("compare")
    compare_parser.add_argument("--build-dir", type=Path, required=True)
    compare_parser.add_argument("--results", type=Path, required=True)
    compare_parser.add_argument("--summary", type=Path, required=True)
    compare_parser.add_argument("--threshold", type=float, default=1.3)
    compare_parser.add_argument("--base-revision")
    compare_parser.add_argument("--head-revision")
    args = parser.parse_args()
    try:
        if args.command == "prepare":
            prepare(args.build_dir, args.shards, sys.stdin, args.baseline_dir)
        elif args.command == "pair":
            pair(args.build_dir, args.baseline_dir)
        elif args.command == "partition":
            partition(args.build_dir, args.shards)
        elif args.command == "run":
            run_shard(args.build_dir, args.shard, args.output, args.baseline_source)
        elif args.command == "merge":
            merge(args.build_dir, args.reports_dir, args.output)
        else:
            compare(
                args.build_dir,
                args.results,
                args.summary,
                args.threshold,
                args.base_revision,
                args.head_revision,
            )
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()
