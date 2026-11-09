import errno
import io
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts/mlx"))
import storage_benchmark as benchmark


class StorageBenchmarkTests(unittest.TestCase):
    def run_cli(self, args):
        stdout = io.StringIO()
        with patch.object(sys, "stdout", stdout):
            code = benchmark.main(args)
        return code, json.loads(stdout.getvalue())

    def test_real_file_counts_times_environment_and_cleanup(self):
        with tempfile.TemporaryDirectory() as directory:
            sentinel = Path(directory) / "keep"
            sentinel.write_bytes(b"existing data")
            code, report = self.run_cli(["--directory", directory, "--size-mib", "1"])
            self.assertEqual(code, 0)
            self.assertEqual(report["status"], "ok")
            self.assertEqual(report["requested_bytes"], benchmark.MIB)
            for phase in ("write_fsync", "read"):
                self.assertEqual(report[phase]["bytes"], benchmark.MIB)
                self.assertGreater(report[phase]["elapsed_seconds"], 0)
            self.assertEqual(report["environment"]["directory"], str(Path(directory).resolve()))
            self.assertEqual(report["environment"]["machine"], benchmark.platform.machine())
            self.assertTrue(report["temporary_file_closed"])
            self.assertEqual(report["errors"], [])
            self.assertIn("OS cache", " ".join(report["limitations"]))
            self.assertEqual(list(Path(directory).iterdir()), [sentinel])
            self.assertEqual(sentinel.read_bytes(), b"existing data")

    def test_default_directory_real_run(self):
        code, report = self.run_cli(["--size-mib", "1"])
        self.assertEqual(code, 0)
        self.assertEqual(report["status"], "ok")
        self.assertEqual(report["requested_bytes"], benchmark.MIB)
        self.assertEqual(report["environment"]["directory"], str(Path(tempfile.gettempdir()).resolve()))
        self.assertTrue(report["temporary_file_closed"])
        self.assertEqual(report["errors"], [])

    def test_subprocess_execution_success(self):
        with tempfile.TemporaryDirectory() as directory:
            script = Path(benchmark.__file__).resolve()
            proc = subprocess.run(
                [sys.executable, "-B", str(script), "--directory", directory, "--size-mib", "1"],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(proc.returncode, 0)
            data = json.loads(proc.stdout)
            self.assertEqual(data["status"], "ok")
            self.assertEqual(data["requested_bytes"], benchmark.MIB)
            self.assertEqual(data["write_fsync"]["bytes"], benchmark.MIB)
            self.assertEqual(data["read"]["bytes"], benchmark.MIB)
            self.assertTrue(data["temporary_file_closed"])
            self.assertEqual(data["errors"], [])

    def test_subprocess_execution_failure(self):
        script = Path(benchmark.__file__).resolve()
        proc = subprocess.run(
            [sys.executable, "-B", str(script), "--size-mib", "0"],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(proc.returncode, 1)
        data = json.loads(proc.stdout)
        self.assertEqual(data["status"], "error")
        self.assertGreater(len(data["errors"]), 0)
        self.assertEqual(data["errors"][0]["phase"], "validation")

        proc_arg = subprocess.run(
            [sys.executable, "-B", str(script), "--unknown"],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(proc_arg.returncode, 1)
        data_arg = json.loads(proc_arg.stdout)
        self.assertEqual(data_arg["status"], "error")
        self.assertGreater(len(data_arg["errors"]), 0)
        self.assertEqual(data_arg["errors"][0]["phase"], "arguments")

    def test_defaults_are_passed_to_measure(self):
        with patch.object(benchmark, "measure") as measure:
            code, _ = self.run_cli([])
        self.assertEqual(code, 1)  # no measurement is no success
        self.assertEqual(measure.call_args.args[1:],
                         (tempfile.gettempdir(), benchmark.DEFAULT_SIZE_MIB * benchmark.MIB))

    def test_help_does_not_create_a_file(self):
        with patch.object(sys, "stdout", io.StringIO()), \
                patch.object(benchmark.tempfile, "TemporaryFile") as create:
            with self.assertRaises(SystemExit) as caught:
                benchmark.main(["--help"])
        self.assertEqual(caught.exception.code, 0)
        create.assert_not_called()

    def test_invalid_arguments_emit_json_without_creating_file(self):
        for args in (["--size-mib", "no"], ["--size-mib"], ["--unknown"],
                     ["--size-mib", "0"], ["--size-mib", "-1"],
                     ["--size-mib", str(benchmark.MAX_SIZE_MIB + 1)]):
            with self.subTest(args=args), patch.object(benchmark.tempfile, "TemporaryFile") as create:
                code, report = self.run_cli(args)
                self.assertEqual(code, 1)
                self.assertEqual(report["status"], "error")
                self.assertEqual(len(report["errors"]), 1)
                self.assertIsNone(report["write_fsync"]["elapsed_seconds"])
                self.assertIsNone(report["temporary_file_closed"])
                create.assert_not_called()

    def test_size_bounds_and_multiple_chunks(self):
        for size in (1, benchmark.CHUNK_BYTES + 7, benchmark.MAX_SIZE_MIB * benchmark.MIB):
            # Mock I/O tests the upper bound without allocating/writing 256 MiB.
            with self.subTest(size=size), tempfile.TemporaryDirectory() as directory:
                file = Mock()
                file.write.side_effect = lambda chunk: len(chunk)
                file.read.side_effect = lambda count: b"x" * count
                with patch.object(benchmark.tempfile, "TemporaryFile", return_value=file), \
                        patch.object(benchmark.os, "urandom", return_value=b"x" * benchmark.MIB), \
                        patch.object(benchmark.os, "fsync"):
                    report = benchmark.new_report()
                    benchmark.measure(report, directory, size)
                self.assertEqual(report["status"], "ok")
                self.assertEqual(report["read"]["bytes"], size)
                self.assertTrue(report["temporary_file_closed"])
                self.assertEqual(file.method_calls[-1][0], "close")
                calls = [call[0] for call in file.method_calls]
                self.assertLess(calls.index("write"), calls.index("flush"))
                self.assertLess(calls.index("flush"), calls.index("fileno"))
                self.assertLess(calls.index("fileno"), calls.index("read"))

    def test_missing_directory_and_create_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            code, report = self.run_cli(["--directory", str(Path(directory) / "missing")])
            self.assertEqual(code, 1)
            self.assertEqual(report["errors"][0]["phase"], "validation")
            with patch.object(benchmark.tempfile, "TemporaryFile",
                              side_effect=PermissionError(errno.EACCES, "denied")):
                code, report = self.run_cli(["--directory", directory])
            self.assertEqual(code, 1)
            self.assertEqual(report["errors"][0]["phase"], "create")
            self.assertEqual(report["errors"][0]["errno"], errno.EACCES)
            self.assertEqual(list(Path(directory).iterdir()), [])
            target = Path(directory) / "file"
            target.write_bytes(b"keep")
            code, report = self.run_cli(["--directory", str(target)])
            self.assertEqual(code, 1)
            self.assertEqual(report["errors"][0]["phase"], "create")
            self.assertEqual(target.read_bytes(), b"keep")

    def test_io_failures_preserve_error_and_close_real_file(self):
        for failure in ("prepare", "write", "short_write", "flush", "fsync", "seek",
                        "read", "short_read", "mismatch", "interrupt", "clock_write", "clock_read"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as directory:
                real = tempfile.TemporaryFile(mode="w+b", dir=directory)
                file = Mock(wraps=real)
                error = OSError(errno.ENOSPC, "injected failure")
                method = {"short_write": "write", "short_read": "read", "mismatch": "read",
                          "interrupt": "write"}.get(failure, failure)
                if method in ("write", "flush", "seek", "read"):
                    getattr(file, method).side_effect = error
                if failure == "short_write":
                    file.write.side_effect = None
                    file.write.return_value = 3
                elif failure in ("short_read", "mismatch"):
                    file.read.side_effect = None
                    file.read.return_value = b"" if failure == "short_read" else b"bad bytes"
                elif failure == "interrupt":
                    file.write.side_effect = KeyboardInterrupt()
                with patch.object(benchmark.tempfile, "TemporaryFile", return_value=file), \
                        patch.object(benchmark.os, "urandom", side_effect=error if failure == "prepare" else None,
                                     return_value=b"payload!"), \
                        patch.object(benchmark.os, "fsync", side_effect=error if failure == "fsync" else None), \
                        patch.object(benchmark.time, "perf_counter", side_effect=(
                            [1, 1] if failure == "clock_write" else
                            [1, 2, 3, 3] if failure == "clock_read" else [1, 2, 3, 4])):
                    report = benchmark.new_report()
                    benchmark.measure(report, directory, 8)
                self.assertEqual(report["status"], "error")
                self.assertEqual(len(report["errors"]), 1)
                self.assertTrue(real.closed)
                self.assertTrue(report["temporary_file_closed"])
                self.assertEqual(list(Path(directory).iterdir()), [])
                phase = report["errors"][0]["phase"]
                expected_phase = ("prepare" if failure == "prepare" else "read"
                                  if failure in ("read", "seek", "short_read", "mismatch", "clock_read")
                                  else "write_fsync")
                self.assertEqual(phase, expected_phase)
                if phase != "prepare":
                    self.assertIsNone(report[phase]["elapsed_seconds"])
                if failure == "short_write":
                    self.assertEqual(report["write_fsync"]["bytes"], 3)
                if failure in ("read", "seek", "short_read", "mismatch", "clock_read"):
                    self.assertEqual(report["write_fsync"]["elapsed_seconds"], 1)

    def test_cleanup_failure_is_not_success_and_preserves_primary_error(self):
        for primary_error in (False, True):
            with self.subTest(primary_error=primary_error), tempfile.TemporaryDirectory() as directory:
                file = Mock()
                file.write.side_effect = OSError("write failed") if primary_error else None
                file.write.return_value = 8
                file.read.return_value = b"payload!"
                file.close.side_effect = OSError("close failed")
                with patch.object(benchmark.tempfile, "TemporaryFile", return_value=file), \
                        patch.object(benchmark.os, "urandom", return_value=b"payload!"), \
                        patch.object(benchmark.os, "fsync"):
                    report = benchmark.new_report()
                    benchmark.measure(report, directory, 8)
                self.assertEqual(report["status"], "error")
                self.assertFalse(report["temporary_file_closed"])
                self.assertEqual([e["phase"] for e in report["errors"]],
                                 ["write_fsync", "cleanup"] if primary_error else ["cleanup"])


if __name__ == "__main__":
    unittest.main()
