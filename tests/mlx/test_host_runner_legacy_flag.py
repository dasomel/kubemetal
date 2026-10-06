import ast
import unittest
from pathlib import Path

HOST_RUNNER = Path(__file__).resolve().parents[2] / "scripts" / "prefect" / "host_runner.py"


class HostRunnerLegacyFlagTests(unittest.TestCase):
    """host_runner imports prefect at module level, so assert on the parsed source instead
    of importing it: finetune_flow's wrapper command must carry the explicit opt-in (D45)."""

    def test_finetune_flow_command_passes_legacy_direct_output(self):
        tree = ast.parse(HOST_RUNNER.read_text())
        flow = next(n for n in ast.walk(tree)
                    if isinstance(n, ast.FunctionDef) and n.name == "finetune_flow")
        commands = [n for n in ast.walk(flow) if isinstance(n, ast.List)
                    and "--adapter-name" in {e.value for e in n.elts if isinstance(e, ast.Constant)}]
        self.assertEqual(len(commands), 1)
        consts = [e.value for e in commands[0].elts if isinstance(e, ast.Constant)]
        self.assertIn("--legacy-direct-output", consts)
        self.assertNotIn("--output-dir", consts)

    def test_only_host_runner_passes_the_flag(self):
        root = HOST_RUNNER.parents[2]
        users = {str(p.relative_to(root)) for p in (root / "scripts").rglob("*.py")
                 if "--legacy-direct-output" in p.read_text()}
        self.assertEqual(users, {"scripts/mlx/finetune_wrapper.py", "scripts/prefect/host_runner.py"})


if __name__ == "__main__":
    unittest.main()
