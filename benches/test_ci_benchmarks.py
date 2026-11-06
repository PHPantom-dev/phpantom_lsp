"""Exercise the PR benchmark runner with small Criterion-compatible executables."""

import contextlib
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).with_name("ci_benchmarks.py")
SPEC = importlib.util.spec_from_file_location("ci_benchmarks", SCRIPT)
ci = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ci)


FAKE_CRITERION = r'''
import json
import os
from pathlib import Path
import re
import sys

config = CONFIG
args = sys.argv[1:]
with Path(config["log"]).open("a") as log:
    log.write(json.dumps(args) + "\n")
if "--list" in args:
    if config["mode"] == "list_failure":
        sys.exit(2)
    print("Criterion setup output")
    for name in config["names"]:
        print(name + ": benchmark")
    sys.exit(0)
assert "--bench" in args, "direct execution must enable benchmark mode"
criterion_home = Path(os.environ["CRITERION_HOME"])
assert criterion_home.is_dir()
Path(config["log"] + ".home").write_text(str(criterion_home))
if config.get("fixture") is not None:
    assert Path("fixture.txt").read_text() == config["fixture"]
assert "--noplot" in args
assert args[args.index("--output-format") + 1] == "bencher"
assert not any(arg in args for arg in (
    "--sample-size", "--measurement-time", "--warm-up-time", "--quick"
)), "sampling must remain unchanged"
pattern = next(arg for arg in args if arg.startswith("^"))
names = [name for name in config["names"] if re.search(pattern, name)]
with Path(config["log"] + ".contexts").open("a") as log:
    log.write(json.dumps({
        "cwd": str(Path.cwd()), "home": str(criterion_home), "names": names
    }) + "\n")
if config["mode"] == "missing":
    names = names[1:]
elif config["mode"] == "duplicate":
    names += names[:1]
elif config["mode"] == "unexpected":
    names.append("unexpected_case")
elif config["mode"] == "unreported":
    print("test " + names.pop(0) + " ... ")
elif config["mode"] == "unnamed":
    print("bench: 1,000,000 ns/iter (+/- 1,000)")
for name in names:
    print("test " + name + " ... bench: 1,234,567 ns/iter (+/- 10,000)")
if config["mode"] == "run_failure":
    sys.exit(3)
'''


class BenchmarkShardingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.build = self.root / "build"
        self.reports = self.root / "reports"
        self.reports.mkdir()
        self.output = self.root / "output.json"
        self.stdout = io.StringIO()
        self.stderr = io.StringIO()

    def binary(self, target, names, mode="normal", *, revision=None, fixture=None):
        filename = target if revision is None else revision + "-" + target
        path = self.root / filename
        config = {
            "names": names, "mode": mode, "log": str(self.root / (filename + ".log")),
            "fixture": fixture,
        }
        path.write_text(
            "#!" + sys.executable + "\n" + FAKE_CRITERION.replace("CONFIG", repr(config)),
            encoding="utf-8",
        )
        path.chmod(0o755)
        return {
            "reason": "compiler-artifact",
            "target": {"name": target, "kind": ["bench"]},
            "executable": str(path),
        }

    def prepare(self, records, shards=2, baseline_dir=None):
        stream = io.StringIO("".join(json.dumps(record) + "\n" for record in records))
        with contextlib.redirect_stdout(self.stdout), contextlib.redirect_stderr(self.stderr):
            ci.prepare(self.build, shards, stream, baseline_dir=baseline_dir)
        return json.loads((self.build / "manifest.json").read_text())

    def run_shard(self, shard, baseline_source=None):
        output = self.reports / ("shard-" + str(shard) + ".json")
        with contextlib.redirect_stdout(self.stdout), contextlib.redirect_stderr(self.stderr):
            ci.run_shard(self.build, shard, output, baseline_source=baseline_source)
        return json.loads(output.read_text())

    def merge(self):
        with contextlib.redirect_stdout(self.stdout), contextlib.redirect_stderr(self.stderr):
            ci.merge(self.build, self.reports, self.output)
        return json.loads(self.output.read_text())

    def invoke(self, arguments, input_text=""):
        with mock.patch.object(sys, "argv", [str(SCRIPT), *arguments]), mock.patch.object(
            sys, "stdin", io.StringIO(input_text)
        ), contextlib.redirect_stdout(self.stdout), contextlib.redirect_stderr(self.stderr):
            runpy.run_path(str(SCRIPT), run_name="__main__")

    def prepare_pair(self, base, head, shards=2, base_shards=1):
        self.baseline_build = self.root / "baseline-build"
        self.baseline_source = self.root / "baseline-source"
        self.baseline_source.mkdir(exist_ok=True)
        stream = io.StringIO("".join(json.dumps(record) + "\n" for record in base))
        with contextlib.redirect_stdout(self.stdout), contextlib.redirect_stderr(self.stderr):
            ci.prepare(self.baseline_build, base_shards, stream)
        return self.prepare(head, shards=shards, baseline_dir=self.baseline_build)

    @staticmethod
    def result(name, value=10):
        return {"name": name, "unit": "ms", "value": value, "range": "± 0.010"}

    def compare(self, results, threshold=1.3):
        self.output.write_text(json.dumps(results))
        summary = self.root / "summary.md"
        with contextlib.redirect_stdout(self.stdout), contextlib.redirect_stderr(self.stderr):
            ci.compare(self.build, self.output, summary, threshold=threshold)
        return summary.read_text()

    def test_round_trip_preserves_every_case_once_and_executable_permissions(self):
        names = ["group/a+b", "group/a.b", "group/é", "group/(item)", "group/item[1]"]
        records = [self.binary("second", ["last"]), self.binary("first", names)]
        manifest = self.prepare(records, shards=4)
        self.assertEqual(manifest["shards"], 4)
        cases = manifest["cases"]
        self.assertEqual(
            [(case["target"], case["name"]) for case in cases],
            sorted([("first", name) for name in names] + [("second", "last")]),
        )
        self.assertEqual([case["shard"] for case in cases], [1, 2, 3, 4, 1, 2])

        shutil.rmtree(self.build / "bin")
        with tarfile.open(self.build / "binaries.tar") as archive:
            archive.extractall(self.build)
        for relative in manifest["executables"].values():
            self.assertTrue(os.access(self.build / relative, os.X_OK))

        for shard in range(1, 5):
            self.run_shard(shard)
        merged = self.merge()
        self.assertEqual([result["name"] for result in merged], [case["name"] for case in cases])
        self.assertEqual(len(merged), 6)
        for result in merged:
            self.assertEqual(result["unit"], "ms")
            self.assertEqual(result["value"], 1.235)
            self.assertEqual(result["range"], "± 0.010")
        for target in ("first", "second"):
            calls = [json.loads(line) for line in (self.root / (target + ".log")).read_text().splitlines()]
            self.assertEqual(sum("--list" in args for args in calls), 1)
            expected = len({case["shard"] for case in cases if case["target"] == target})
            self.assertEqual(sum("--bench" in args for args in calls), expected)

    def test_prepare_ignores_non_bench_cargo_messages(self):
        records = [
            {"reason": "build-finished", "success": True},
            {"reason": "compiler-artifact", "target": {"kind": ["lib"]}},
            {"reason": "compiler-artifact", "target": {"kind": ["bench"]}, "executable": None},
            self.binary("bench", ["case"]),
        ]
        stream = io.StringIO("\n".join(json.dumps(record) for record in records))
        with contextlib.redirect_stdout(self.stdout), contextlib.redirect_stderr(self.stderr):
            ci.prepare(self.build, 1, stream)
        self.assertEqual(len(json.loads((self.build / "manifest.json").read_text())["cases"]), 1)

    def test_prepare_rejects_invalid_shard_counts(self):
        record = self.binary("bench", ["case"])
        for count in (0, -1, 2):
            with self.subTest(shards=count), self.assertRaises(ValueError):
                self.prepare([record], shards=count)

    def test_prepare_rejects_missing_or_duplicate_benchmarks(self):
        first = self.binary("first", ["case"])
        second = self.binary("second", ["case"])
        empty = self.binary("empty", [])
        empty_name = self.binary("empty_name", [""])
        duplicate = self.binary("duplicate", ["same", "same"])
        for records in ([], [first, first], [first, second], [empty], [empty_name], [duplicate]):
            with self.subTest(records=records), self.assertRaises(ValueError):
                self.prepare(records, shards=1)

    def test_prepare_propagates_listing_failure(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self.prepare([self.binary("bench", ["case"], "list_failure")], shards=1)

    def test_run_rejects_invalid_shards(self):
        self.prepare([self.binary("bench", ["one", "two"])])
        for shard in (0, -1, 3):
            with self.subTest(shard=shard), self.assertRaises(ValueError):
                self.run_shard(shard)

    def test_run_uses_a_fresh_criterion_home_and_removes_it(self):
        self.prepare([self.binary("bench", ["one"])], shards=1)
        baseline = self.root / "old-criterion"
        baseline.mkdir()
        marker = baseline / "existing-baseline"
        marker.write_text("keep")
        with mock.patch.dict(os.environ, {"CRITERION_HOME": str(baseline)}):
            self.run_shard(1)
        actual_home = Path((self.root / "bench.log.home").read_text())
        self.assertNotEqual(actual_home, baseline)
        self.assertFalse(actual_home.exists())
        self.assertEqual(marker.read_text(), "keep")

    def test_run_rejects_an_empty_shard(self):
        manifest = self.prepare([self.binary("bench", ["one", "two"])])
        manifest["cases"] = [case for case in manifest["cases"] if case["shard"] != 2]
        (self.build / "manifest.json").write_text(json.dumps(manifest))
        with self.assertRaises(ValueError):
            self.run_shard(2)

    def test_run_rejects_missing_duplicate_unexpected_or_unreported_results(self):
        for mode in ("missing", "duplicate", "unexpected", "unreported"):
            with self.subTest(mode=mode):
                self.prepare([self.binary("bench", ["one", "two"], mode)], shards=1)
                with self.assertRaises(ValueError):
                    self.run_shard(1)
                self.assertFalse((self.reports / "shard-1.json").exists())

    def test_run_propagates_failed_benchmark_and_keeps_its_output(self):
        self.prepare([self.binary("bench", ["one"], "run_failure")], shards=1)
        with self.assertRaises(subprocess.CalledProcessError):
            self.run_shard(1)
        self.assertIn("test one", self.stdout.getvalue())
        self.assertFalse((self.reports / "shard-1.json").exists())

    def test_run_propagates_formatter_failure(self):
        self.prepare([self.binary("bench", ["one"], "unnamed")], shards=1)
        with self.assertRaises(subprocess.CalledProcessError):
            self.run_shard(1)

    def test_merge_rejects_missing_or_extra_report_files(self):
        self.prepare([self.binary("bench", ["one", "two"])])
        self.run_shard(1)
        with self.assertRaises(ValueError):
            self.merge()
        self.run_shard(2)
        (self.reports / "shard-3.json").write_text("[]")
        with self.assertRaises(ValueError):
            self.merge()
        self.assertFalse(self.output.exists())

    def test_merge_rejects_missing_duplicate_unexpected_or_misassigned_names(self):
        self.prepare([self.binary("bench", ["one", "two", "three", "four"])])
        first = self.run_shard(1)
        second = self.run_shard(2)
        variants = [first[:-1], first + first[:1], first + second[:1], second]
        for results in variants:
            with self.subTest(results=results):
                (self.reports / "shard-1.json").write_text(json.dumps(results))
                with self.assertRaises(ValueError):
                    self.merge()
                self.assertFalse(self.output.exists())

    def test_merge_rejects_malformed_results(self):
        self.prepare([self.binary("bench", ["one"])], shards=1)
        for value in ({"name": "one"}, ["one"], [{}], [{"name": 1}]):
            with self.subTest(value=value):
                (self.reports / "shard-1.json").write_text(json.dumps(value))
                with self.assertRaises(ValueError):
                    self.merge()

    def test_cli_runs_all_three_commands(self):
        record = self.binary("bench", ["one"])
        self.invoke(["prepare", "--build-dir", str(self.build), "--shards", "1"], json.dumps(record))
        self.invoke([
            "run", "--build-dir", str(self.build), "--shard", "1",
            "--output", str(self.reports / "shard-1.json"),
        ])
        self.invoke([
            "merge", "--build-dir", str(self.build), "--reports-dir", str(self.reports),
            "--output", str(self.output),
        ])
        self.assertEqual(json.loads(self.output.read_text())[0]["name"], "one")

    def test_cli_reports_invalid_input_and_execution_errors(self):
        cases = [
            (["prepare", "--build-dir", str(self.build), "--shards", "0"], ""),
            (["prepare", "--build-dir", str(self.build), "--shards", "1"], "invalid json"),
            (["run", "--build-dir", str(self.build), "--shard", "1", "--output", str(self.output)], ""),
            (["prepare", "--build-dir", str(self.build), "--shards", "1"], json.dumps(
                self.binary("bench", ["one"], "list_failure")
            )),
        ]
        for args, text in cases:
            with self.subTest(args=args, text=text), self.assertRaises(SystemExit) as error:
                self.invoke(args, text)
            self.assertNotEqual(error.exception.code, 0)
        self.assertTrue(self.stderr.getvalue())

    def test_paired_cases_follow_head_shards_after_insertions_and_target_moves(self):
        manifest = self.prepare_pair(
            [self.binary("bench", ["b", "c", "d", "removed"], revision="base")],
            [
                self.binary("bench", ["a", "b", "c"], revision="head"),
                self.binary("moved", ["d"], revision="head"),
            ],
            base_shards=2,
        )
        baseline = json.loads((self.baseline_build / "manifest.json").read_text())
        cases = {case["name"]: case for case in manifest["cases"]}
        original = {case["name"]: case for case in baseline["cases"]}
        self.assertEqual(original["b"]["shard"], 1)
        self.assertEqual(cases["b"]["shard"], 2)
        self.assertNotIn("baseline_target", cases["a"])
        self.assertEqual(cases["d"]["target"], "moved")
        self.assertEqual(cases["d"]["baseline_target"], "bench")
        self.assertEqual(manifest["removed"], ["removed"])

        # A shard receives only the archive and manifest, not the builder's tree.
        shutil.rmtree(self.baseline_build)
        shutil.rmtree(self.build / "bin")
        shutil.rmtree(self.build / "baseline-bin")
        with tarfile.open(self.build / "binaries.tar") as archive:
            archive.extractall(self.build)
        for executables in (manifest["executables"], manifest["baseline_executables"]):
            for relative in executables.values():
                self.assertTrue(os.access(self.build / relative, os.X_OK))

        for shard in (1, 2):
            report = self.run_shard(shard, self.baseline_source)
            expected = [case for case in manifest["cases"] if case["shard"] == shard]
            self.assertEqual(
                sorted(result["name"] for result in report["head"]),
                sorted(case["name"] for case in expected),
            )
            self.assertEqual(
                sorted(result["name"] for result in report["base"]),
                sorted(case["name"] for case in expected if "baseline_target" in case),
            )
        merged = self.merge()
        self.assertEqual([result["name"] for result in merged["head"]], ["a", "b", "c", "d"])
        self.assertEqual([result["name"] for result in merged["base"]], ["b", "c", "d"])
        self.assertEqual(merged["removed"], ["removed"])

    def test_paired_shard_with_only_new_cases_needs_no_base_measurements(self):
        self.prepare_pair(
            [self.binary("bench", ["z"], revision="base")],
            [self.binary("bench", ["a", "z"], revision="head")],
        )
        report = self.run_shard(1, self.baseline_source)
        self.assertEqual(report["base"], [])
        self.assertEqual([result["name"] for result in report["head"]], ["a"])
        base_calls = [
            json.loads(line) for line in (self.root / "base-bench.log").read_text().splitlines()
        ]
        self.assertFalse(any("--bench" in args for args in base_calls))
        self.run_shard(2, self.baseline_source)
        merged = self.merge()
        self.assertEqual([result["name"] for result in merged["base"]], ["z"])

    def test_paired_prepare_cannot_overwrite_its_own_baseline(self):
        manifest = self.prepare(
            [self.binary("bench", ["case"], revision="base")], shards=1
        )
        binary = self.build / manifest["executables"]["bench"]
        original = binary.read_bytes()
        with self.assertRaises(ValueError):
            self.prepare(
                [self.binary("bench", ["case"], revision="head")],
                shards=1,
                baseline_dir=self.build / ".",
            )
        self.assertEqual(binary.read_bytes(), original)

    def test_paired_runs_use_their_own_fixtures_and_separate_criterion_homes(self):
        self.prepare_pair(
            [self.binary("bench", ["case"], revision="base", fixture="base fixture")],
            [self.binary("bench", ["case"], revision="head", fixture="head fixture")],
            shards=1,
        )
        (self.baseline_source / "fixture.txt").write_text("base fixture")
        head_source = self.root / "head-source"
        head_source.mkdir()
        (head_source / "fixture.txt").write_text("head fixture")
        previous_home = self.root / "previous-criterion"
        previous_home.mkdir()
        marker = previous_home / "baseline"
        marker.write_text("keep")
        previous_directory = Path.cwd()
        try:
            os.chdir(head_source)
            with mock.patch.dict(os.environ, {"CRITERION_HOME": str(previous_home)}):
                self.run_shard(1, self.baseline_source)
        finally:
            os.chdir(previous_directory)

        contexts = {}
        for revision in ("base", "head"):
            entries = (self.root / (revision + "-bench.log.contexts")).read_text().splitlines()
            self.assertEqual(len(entries), 1)
            contexts[revision] = json.loads(entries[0])
            self.assertFalse(Path(contexts[revision]["home"]).exists())
            self.assertNotEqual(Path(contexts[revision]["home"]), previous_home)
        self.assertEqual(Path(contexts["base"]["cwd"]), self.baseline_source.resolve())
        self.assertEqual(Path(contexts["head"]["cwd"]), head_source.resolve())
        self.assertNotEqual(contexts["base"]["home"], contexts["head"]["home"])
        self.assertEqual(marker.read_text(), "keep")

    def test_paired_run_requires_a_baseline_source_directory(self):
        self.prepare_pair(
            [self.binary("bench", ["case"], revision="base")],
            [self.binary("bench", ["case"], revision="head")],
            shards=1,
        )
        for source in (None, self.root / "missing-source", self.root / "base-bench"):
            with self.subTest(source=source), self.assertRaises(ValueError):
                self.run_shard(1, source)
        self.assertFalse((self.reports / "shard-1.json").exists())

    def test_paired_run_rejects_incomplete_or_failed_base_and_head(self):
        for revision in ("base", "head"):
            for mode, exception in (
                ("missing", ValueError),
                ("run_failure", subprocess.CalledProcessError),
            ):
                with self.subTest(revision=revision, mode=mode):
                    self.prepare_pair(
                        [self.binary(
                            "bench", ["one", "two"],
                            mode if revision == "base" else "normal", revision="base",
                        )],
                        [self.binary(
                            "bench", ["one", "two"],
                            mode if revision == "head" else "normal", revision="head",
                        )],
                        shards=1,
                    )
                    with self.assertRaises(exception):
                        self.run_shard(1, self.baseline_source)
                    self.assertFalse((self.reports / "shard-1.json").exists())

    def test_paired_merge_requires_all_reports_and_every_measurement_on_its_shard(self):
        self.prepare_pair(
            [self.binary("bench", ["a", "b", "c", "d"], revision="base")],
            [self.binary("bench", ["a", "b", "c", "d"], revision="head")],
        )
        first = self.run_shard(1, self.baseline_source)
        with self.assertRaises(ValueError):
            self.merge()
        second = self.run_shard(2, self.baseline_source)
        variants = [{}, first["head"], {"base": first["base"]}, {"head": first["head"]}]
        for revision in ("base", "head"):
            for measurements in (
                first[revision][:-1],
                first[revision] + first[revision][:1],
                first[revision] + [self.result("unexpected")],
                second[revision],
                "invalid measurements",
            ):
                variants.append({**first, revision: measurements})
        for report in variants:
            with self.subTest(report=report):
                (self.reports / "shard-1.json").write_text(json.dumps(report))
                with self.assertRaises(ValueError):
                    self.merge()
                self.assertFalse(self.output.exists())

    def test_compare_accepts_thirty_percent_and_reports_new_and_removed_cases(self):
        self.prepare_pair(
            [self.binary("bench", ["shared", "removed"], revision="base")],
            [self.binary("bench", ["shared", "new"], revision="head")],
            shards=1,
        )
        results = {
            "base": [self.result("shared", 10)],
            "head": [self.result("shared", 13), self.result("new", 1000)],
            "removed": ["removed"],
        }
        summary = self.compare(results)
        for name in ("shared", "new", "removed"):
            self.assertIn(name, summary)
        self.assertIn("10", summary)
        self.assertIn("13", summary)
        self.assertRegex(summary, r"1\.30")

    def test_compare_rejects_regression_before_rounding_and_keeps_its_summary(self):
        self.prepare_pair(
            [self.binary("bench", ["case"], revision="base")],
            [self.binary("bench", ["case"], revision="head")],
            shards=1,
        )
        results = {
            "base": [self.result("case", 10)],
            "head": [self.result("case", 13.001)],
            "removed": [],
        }
        with self.assertRaises(ValueError):
            self.compare(results)
        summary = (self.root / "summary.md").read_text()
        self.assertIn("case", summary)
        self.assertIn("13.001", summary)
        self.compare(results, threshold=1.4)

    def test_compare_revalidates_complete_results_against_the_manifest(self):
        self.prepare_pair(
            [self.binary("bench", ["shared", "removed"], revision="base")],
            [self.binary("bench", ["shared", "new"], revision="head")],
            shards=1,
        )
        valid = {
            "base": [self.result("shared")],
            "head": [self.result("shared"), self.result("new")],
            "removed": ["removed"],
        }
        variants = [
            {}, [], {**valid, "removed": []},
            {key: value for key, value in valid.items() if key != "removed"},
        ]
        for revision in ("base", "head"):
            variants.extend([
                {key: value for key, value in valid.items() if key != revision},
                {**valid, revision: valid[revision][:-1]},
                {**valid, revision: valid[revision] + valid[revision][:1]},
                {**valid, revision: valid[revision] + [self.result("unexpected")]},
            ])
        for results in variants:
            with self.subTest(results=results), self.assertRaises(ValueError):
                self.compare(results)

    def test_compare_rejects_invalid_values_and_units_on_either_revision(self):
        self.prepare_pair(
            [self.binary("bench", ["case"], revision="base")],
            [self.binary("bench", ["case"], revision="head")],
            shards=1,
        )
        valid = {
            "base": [self.result("case")],
            "head": [self.result("case")],
            "removed": [],
        }
        invalid_fields = [
            ("value", value)
            for value in (0, -1, True, False, "10", None, float("nan"), float("inf"), float("-inf"))
        ] + [("unit", value) for value in ("ns", "s", "", None)]
        for revision in ("base", "head"):
            for field, value in invalid_fields:
                with self.subTest(revision=revision, field=field, value=value):
                    results = copy.deepcopy(valid)
                    results[revision][0][field] = value
                    with self.assertRaises(ValueError):
                        self.compare(results)
            for field in ("value", "unit"):
                results = copy.deepcopy(valid)
                del results[revision][0][field]
                with self.subTest(revision=revision, missing=field), self.assertRaises(ValueError):
                    self.compare(results)

    def test_compare_rejects_invalid_thresholds(self):
        self.prepare_pair(
            [self.binary("bench", ["case"], revision="base")],
            [self.binary("bench", ["case"], revision="head")],
            shards=1,
        )
        results = {
            "base": [self.result("case")],
            "head": [self.result("case")],
            "removed": [],
        }
        for threshold in (0, 0.99, -1, True, "1.3", None, float("nan"), float("inf")):
            with self.subTest(threshold=threshold), self.assertRaises(ValueError):
                self.compare(results, threshold=threshold)

    def test_cli_runs_paired_measurement_and_comparison(self):
        baseline_build = self.root / "baseline-build"
        base = self.binary("bench", ["case"], revision="base")
        head = self.binary("bench", ["case"], revision="head")
        self.invoke([
            "prepare", "--build-dir", str(baseline_build), "--shards", "1",
        ], json.dumps(base))
        self.invoke([
            "prepare", "--build-dir", str(self.build), "--shards", "1",
            "--baseline-dir", str(baseline_build),
        ], json.dumps(head))
        self.invoke([
            "run", "--build-dir", str(self.build), "--shard", "1",
            "--baseline-source", str(self.root),
            "--output", str(self.reports / "shard-1.json"),
        ])
        self.invoke([
            "merge", "--build-dir", str(self.build), "--reports-dir", str(self.reports),
            "--output", str(self.output),
        ])
        summary = self.root / "summary.md"
        self.invoke([
            "compare", "--build-dir", str(self.build), "--results", str(self.output),
            "--summary", str(summary), "--threshold", "1.3",
            "--base-revision", "base-commit", "--head-revision", "head-commit",
        ])
        for expected in ("case", "base-commit", "head-commit"):
            self.assertIn(expected, summary.read_text())


if __name__ == "__main__":
    unittest.main()
