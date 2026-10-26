"""Exercise the PR benchmark runner with small Criterion-compatible executables."""

import contextlib
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
assert "--noplot" in args
assert args[args.index("--output-format") + 1] == "bencher"
assert not any(arg in args for arg in (
    "--sample-size", "--measurement-time", "--warm-up-time", "--quick"
)), "sampling must remain unchanged"
pattern = next(arg for arg in args if arg.startswith("^"))
names = [name for name in config["names"] if re.search(pattern, name)]
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

    def binary(self, target, names, mode="normal"):
        path = self.root / target
        config = {"names": names, "mode": mode, "log": str(self.root / (target + ".log"))}
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

    def prepare(self, records, shards=2):
        stream = io.StringIO("".join(json.dumps(record) + "\n" for record in records))
        with contextlib.redirect_stdout(self.stdout), contextlib.redirect_stderr(self.stderr):
            ci.prepare(self.build, shards, stream)
        return json.loads((self.build / "manifest.json").read_text())

    def run_shard(self, shard):
        output = self.reports / ("shard-" + str(shard) + ".json")
        with contextlib.redirect_stdout(self.stdout), contextlib.redirect_stderr(self.stderr):
            ci.run_shard(self.build, shard, output)
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


if __name__ == "__main__":
    unittest.main()
