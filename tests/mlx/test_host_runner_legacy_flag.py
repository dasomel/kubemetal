import ast
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HOST_RUNNER = ROOT / "scripts" / "prefect" / "host_runner.py"
LIB_RS = ROOT / "src-tauri" / "src" / "lib.rs"


class FinetuneBypassRetiredTests(unittest.TestCase):
    """D47: fine-tune no longer runs through Prefect. host_runner imports prefect at module
    level, so assert on the parsed source instead of importing it."""

    def test_host_runner_has_no_finetune_flow_or_deployment(self):
        text = HOST_RUNNER.read_text()
        tree = ast.parse(text)
        funcs = {n.name for n in ast.walk(tree) if isinstance(n, ast.FunctionDef)}
        self.assertNotIn("finetune_flow", funcs)
        self.assertNotIn("finetune_flow", text)
        self.assertNotIn("finetune_wrapper", text)
        self.assertIn("evaluate_flow", funcs)
        self.assertIn("ingest_flow", funcs)

    def test_lib_rs_does_not_register_trigger_finetune_flow(self):
        self.assertNotIn("trigger_finetune_flow", LIB_RS.read_text())
        self.assertIn("trigger_evaluate_flow", LIB_RS.read_text())

    def test_no_script_mentions_the_legacy_flag(self):
        users = {str(p.relative_to(ROOT)) for p in (ROOT / "scripts").rglob("*.py")
                 if "legacy-direct-output" in p.read_text()}
        self.assertEqual(users, set())


if __name__ == "__main__":
    unittest.main()
