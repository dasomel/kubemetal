import io
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts/mlx"))
import finetune_wrapper

WRAPPER = Path(finetune_wrapper.__file__)
# Real Python entrypoint with both external boundaries stubbed, including mutations
# that bypass admission. No MLX import, training, HTTP, or real HOME writes.
BOOTSTRAP = """
import io, sys
from unittest.mock import Mock
sys.path.insert(0, sys.argv.pop(1))
import finetune_wrapper as wrapper
wrapper.MlflowReporter = Mock()
wrapper.subprocess.Popen = Mock(return_value=Mock(
    stdout=io.StringIO(''), stderr=io.StringIO(''), returncode=0))
sys.exit(wrapper.main())
"""
ARGS = ["--model", "stub", "--data", "stub", "--iters", "1", "--batch-size", "1",
        "--learning-rate", "0.001", "--adapter-name", "unused"]


class OutputAdmissionTests(unittest.TestCase):
    def test_output_dir_is_required(self):
        with tempfile.TemporaryDirectory() as temp:
            result = subprocess.run([sys.executable, str(WRAPPER), *ARGS], cwd=temp,
                                    capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("--output-dir", result.stderr)
            self.assertIn("arguments are required", result.stderr)
            self.assertEqual(list(Path(temp).iterdir()), [])
            self.assertEqual(result.stdout, "")

    def test_invalid_output_emits_error_without_any_write(self):
        for kind in ["missing", "file", "symlink", "symlink-slash", "nonempty"]:
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                out = root / "out"
                if kind == "file":
                    out.write_bytes(b"keep")
                elif kind.startswith("symlink"):
                    target = root / "target"
                    target.mkdir()
                    out.symlink_to(target, target_is_directory=True)
                elif kind == "nonempty":
                    out.mkdir()
                    (out / "sentinel").write_bytes(b"keep")
                def snapshot():
                    return {str(p.relative_to(root)): (p.lstat().st_mode, p.lstat().st_mtime_ns,
                            p.read_bytes() if p.is_file() else None)
                            for p in root.rglob("*")}
                before = snapshot()
                result = subprocess.run([sys.executable, "-c", BOOTSTRAP, str(WRAPPER.parent), *ARGS,
                                         "--output-dir", str(out) + ("/" if kind == "symlink-slash" else "")],
                                        cwd=temp, capture_output=True, text=True, timeout=10)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual([e["type"] for e in map(json.loads, result.stdout.splitlines())], ["error"])
                self.assertEqual(snapshot(), before)

    def test_runtime_output_flag_mapping(self):
        for runtime, output_flag in [("mlx-lm", "--adapter-path"), ("mlx-vlm", "--output-path")]:
            with self.subTest(runtime=runtime), tempfile.TemporaryDirectory() as temp:
                out = Path(temp) / "out"
                out.mkdir()
                reporter = Mock()
                child = Mock(stdout=io.StringIO(""), stderr=io.StringIO(""), returncode=0)
                stdout = io.StringIO()
                with patch.object(sys, "argv", [str(WRAPPER), *ARGS, "--runtime", runtime, "--output-dir", str(out)]), \
                        patch.object(finetune_wrapper, "MlflowReporter", return_value=reporter), \
                        patch.object(finetune_wrapper.subprocess, "Popen", return_value=child) as spawn, \
                        patch.object(sys, "stdout", stdout):
                    self.assertEqual(finetune_wrapper.main(), 0)
                cmd = spawn.call_args.args[0]
                self.assertEqual(cmd[cmd.index(output_flag) + 1], str(out))
                if runtime == "mlx-vlm":
                    self.assertNotIn("--adapter-path", cmd)
                self.assertEqual(json.loads(stdout.getvalue()),
                                 {"type": "done", "adapter_path": str(out), "last_loss": None})
                self.assertEqual(list(out.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
