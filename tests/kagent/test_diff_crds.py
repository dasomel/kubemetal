"""Inline schema fixtures; no Helm, PyYAML or network required."""
import importlib.util
from pathlib import Path
import unittest
import tempfile
from unittest.mock import patch

HELPER = Path(__file__).resolve().parents[2] / "evidence/kagent/round4-chart/diff_crds.py"
SPEC = importlib.util.spec_from_file_location("diff_crds", HELPER)
module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(module)


class SchemaTests(unittest.TestCase):
  def test_flatten_nested_array_map_and_alternatives(self):
    schema = {"type": "object", "properties": {
      "spec": {"type": "object", "properties": {
        "tools": {"type": "array", "items": {"type": "object", "properties": {
          "name": {"type": "string"}}}},
        "labels": {"type": "object", "additionalProperties": {"type": "string"}},
        "choice": {"oneOf": [{"type": "string"}, {"type": "integer"}]},
      }},
    }}
    self.assertEqual(module.flatten(schema), {
      "spec": "object", "spec.tools": "array", "spec.tools[]": "object",
      "spec.tools[].name": "string", "spec.labels": "object", "spec.labels.*": "string",
      "spec.choice": "<unspecified>", "spec.choice<oneOf:0>": "string",
      "spec.choice<oneOf:1>": "integer",
    })

  def test_added_removed_and_type_changed(self):
    self.assertEqual(module.diff({"gone": "string", "same": "object", "n": "string"},
                                 {"new": "boolean", "same": "object", "n": "integer"}), {
      "added": {"new": "boolean"}, "removed": {"gone": "string"},
      "type_changed": {"n": {"old": "string", "new": "integer"}},
    })

  def test_crds_preserve_each_api_version_and_compare_groups(self):
    documents = [{"kind": "CustomResourceDefinition", "spec": {
      "group": "old.example", "names": {"kind": kind}, "versions": [{
        "name": "v1", "served": True, "storage": True,
        "schema": {"openAPIV3Schema": {"properties": {"spec": {"type": "object"}}}},
      }],
    }} for kind in module.KINDS]
    inventory, paths = module.extract([None, {"kind": "Service"}] + documents)
    self.assertEqual(len(inventory), 3)
    self.assertEqual(inventory[0], {"group": "old.example", "kind": "Agent", "versions": [
      {"name": "v1", "served": True, "storage": True}]})
    new = {key.replace("old.example/v1", "new.example/v2"): {"spec": "string"} for key in paths}
    compared = module.compare(paths, new)
    self.assertEqual(len(compared), 3)
    self.assertEqual(compared["old.example/v1/Agent -> new.example/v2/Agent"]["type_changed"],
                     {"spec": {"old": "object", "new": "string"}})
    documents[0]["spec"]["versions"].append({
      "name": "v0", "served": True, "storage": False,
      "schema": {"openAPIV3Schema": {"properties": {"legacy": {"type": "boolean"}}}},
    })
    _, multi = module.extract(documents)
    self.assertEqual(multi["old.example/v0/Agent"], {"legacy": "boolean"})
    self.assertEqual(len(module.compare(multi, new)), 4)

  def test_failed_command_preserves_argv_rc_and_bounded_streams(self):
    import json
    import sys
    with tempfile.TemporaryDirectory() as temporary:
      root = Path(temporary)
      output, work = root / "output", root / "work"
      output.mkdir()
      work.mkdir()
      recorder = module.Recorder(output, work)
      argv = [sys.executable, "-B", "-c",
              "import sys; print('captured output'); print('missing tag', file=sys.stderr); sys.exit(7)"]
      rc, _ = recorder.run("failure", argv)
      self.assertEqual(rc, 7)
      self.assertTrue(recorder.failed)
      record = json.loads((output / "commands.json").read_text())[0]
      self.assertEqual(record["argv"], argv)
      self.assertEqual(record["exit_code"], 7)
      self.assertEqual((output / "failure.stdout.txt").read_text(), "captured output\n")
      self.assertEqual((output / "failure.stderr.txt").read_text(), "missing tag\n")
      self.assertFalse(record["stdout"]["truncated"])
      previous_limit = module.LIMIT
      try:
        module.LIMIT = 3
        recorder.run("bounded", [sys.executable, "-B", "-c", "print('abcdef')"])
      finally:
        module.LIMIT = previous_limit
      self.assertEqual((output / "bounded.stdout.txt").read_bytes(), b"abc")
      self.assertTrue(recorder.records[-1]["stdout"]["truncated"])

  def test_missing_tags_are_recorded_and_entire_probe_fails(self):
    import json
    import subprocess
    real_run = subprocess.run

    def unavailable_helm(argv, **kwargs):
      if argv[0] == "helm":
        kwargs["stderr"].write(b"fixture: chart tag not found\n")
        return subprocess.CompletedProcess(argv, 1)
      return real_run(argv, **kwargs)

    with tempfile.TemporaryDirectory() as temporary:
      root = Path(temporary)
      with patch.object(module.subprocess, "run", side_effect=unavailable_helm):
        self.assertEqual(module.probe(root / "output", root, "0.9.12", "1.0.0-alpha17"), 1)
      records = json.loads((root / "output/commands.json").read_text())
      pulls = [row for row in records if row["name"].endswith("-pull")]
      self.assertEqual(len(pulls), 4)
      self.assertTrue(all(row["exit_code"] == 1 for row in pulls))
      for row in pulls:
        self.assertEqual((root / "output" / row["stderr"]["file"]).read_text(),
                         "fixture: chart tag not found\n")
      self.assertFalse((root / "output/schema-diff.json").exists())
      self.assertEqual(json.loads((root / "output/result.json").read_text())["exit_code"], 1)

  def test_missing_crds_fail_instead_of_empty_success(self):
    with self.assertRaisesRegex(ValueError, "No CRDs"):
      module.extract([{"kind": "Service"}])


if __name__ == "__main__":
  unittest.main()
