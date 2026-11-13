"""Artifact-only Helm probe; schema path/type comparison is not validation/runtime evidence."""

import json
from pathlib import Path
import subprocess
import sys

KINDS = ("Agent", "ModelConfig", "RemoteMCPServer")
LIMIT = 16 * 1024 * 1024


def flatten(schema, path=""):
  """Include containers, array items, map values and explicitly indexed alternatives."""
  result = {}
  if path:
    result[path] = schema.get("type", "<unspecified>")
  for name, child in sorted(schema.get("properties", {}).items()):
    result.update(flatten(child, f"{path}.{name}" if path else name))
  if isinstance(schema.get("items"), dict):
    result.update(flatten(schema["items"], path + "[]"))
  if isinstance(schema.get("additionalProperties"), dict):
    result.update(flatten(schema["additionalProperties"], path + ".*"))
  for keyword in ("allOf", "anyOf", "oneOf"):
    for index, child in enumerate(schema.get(keyword, [])):
      result.update(flatten(child, f"{path}<{keyword}:{index}>"))
  return result


def diff(old, new):
  return {
    "added": {key: new[key] for key in sorted(new.keys() - old.keys())},
    "removed": {key: old[key] for key in sorted(old.keys() - new.keys())},
    "type_changed": {
      key: {"old": old[key], "new": new[key]}
      for key in sorted(old.keys() & new.keys()) if old[key] != new[key]
    },
  }


def extract(documents):
  inventory, schemas = [], {}
  for document in documents:
    if not isinstance(document, dict) or document.get("kind") != "CustomResourceDefinition":
      continue
    spec = document["spec"]
    kind, group = spec["names"]["kind"], spec["group"]
    versions = []
    for version in spec["versions"]:
      versions.append({key: version[key] for key in ("name", "served", "storage")})
      if kind in KINDS:
        key = f'{group}/{version["name"]}/{kind}'
        if key in schemas:
          raise ValueError(f"duplicate schema: {key}")
        schemas[key] = flatten(version["schema"]["openAPIV3Schema"])
    inventory.append({"group": group, "kind": kind, "versions": versions})
  if not inventory:
    raise ValueError("No CRDs found in rendered output")
  for kind in KINDS:
    if not any(row["kind"] == kind for row in inventory):
      raise ValueError(f"Missing target CRD: {kind}")
  return sorted(inventory, key=lambda row: (row["group"], row["kind"])), schemas


def compare(old, new):
  # D22: compare every API-version pair explicitly, at quadratic output cost;
  # consumers can select a served/storage pair rather than silently assuming one.
  result = {}
  for kind in KINDS:
    for left in sorted(key for key in old if key.endswith("/" + kind)):
      for right in sorted(key for key in new if key.endswith("/" + kind)):
        result[f"{left} -> {right}"] = diff(old[left], new[right])
  return result


def write_json(path, value):
  path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


class Recorder:
  def __init__(self, output, work):
    self.output, self.work = output, work
    self.records = []
    self.failed = False

  def run(self, name, argv):
    stdout, stderr = self.work / (name + ".stdout"), self.work / (name + ".stderr")
    with stdout.open("wb") as out, stderr.open("wb") as err:
      try:
        rc = subprocess.run(argv, stdout=out, stderr=err, check=False).returncode
      except OSError as error:
        rc = 127
        err.write((str(error) + "\n").encode())
    record = {"name": name, "argv": argv, "exit_code": rc}
    for label, source in (("stdout", stdout), ("stderr", stderr)):
      size = source.stat().st_size
      destination = self.output / (name + f".{label}.txt")
      with source.open("rb") as stream:
        destination.write_bytes(stream.read(LIMIT))
      record[label] = {"file": destination.name, "bytes": size, "truncated": size > LIMIT}
      if size > LIMIT:
        self.failed = True
    self.records.append(record)
    write_json(self.output / "commands.json", self.records)
    self.failed |= rc != 0
    return rc, stdout


def probe(output, work, old_version, new_version):
  output, work = Path(output).resolve(), Path(work).resolve()
  repo = Path(__file__).resolve().parents[3]
  if work == repo or repo in work.parents:
    raise ValueError("Temporary download directory must be outside the repository")
  # Fresh output prevents stale successful evidence surviving a failed rerun.
  output.mkdir(parents=True, exist_ok=False)
  recorder = Recorder(output, work)
  yaml_rc, _ = recorder.run("yaml-availability", [
    "python3", "-B", "-c",
    "import sys\ntry:\n import yaml\nexcept ImportError:\n"
    " sys.exit('PyYAML unavailable: CRD parsing requires import yaml; no install attempted')\n"
    "print('PyYAML available:', yaml.__version__)",
  ])
  if not yaml_rc:
    import yaml
  schemas = []
  for index, version in enumerate((old_version, new_version)):
    if not version or any(char not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.-+" for char in version):
      raise ValueError(f"Invalid chart version: {version!r}")
    version_schemas = None
    for chart in ("kagent-crds", "kagent"):
      name = f"{index}-{chart}-{version}"
      download = work / name
      download.mkdir()
      reference = "oci://ghcr.io/kagent-dev/kagent/helm/" + chart
      recorder.run(name + "-pull", ["helm", "pull", reference, "--version", version,
                                   "--destination", str(download)])
      archive = download / f"{chart}-{version}.tgz"
      recorder.run(name + "-sha256", ["shasum", "-a", "256", str(archive)])
      for mode in ("chart", "values", "crds"):
        recorder.run(name + "-show-" + mode, ["helm", "show", mode, str(archive)])
      rc, rendered = recorder.run(name + "-template", [
        "helm", "template", "km-r4", str(archive), "--namespace", "kagent", "--include-crds",
      ])
      if chart == "kagent-crds":
        try:
          if yaml_rc:
            raise ValueError("PyYAML unavailable; schema extraction not performed")
          if rc:
            raise ValueError(f"helm template failed with exit code {rc}; schema extraction not performed")
          with rendered.open() as stream:
            inventory, version_schemas = extract(yaml.safe_load_all(stream))
          write_json(output / (name + "-inventory.json"), inventory)
          write_json(output / (name + "-paths.json"), version_schemas)
        except Exception as error:
          recorder.failed = True
          (output / (name + "-schema-error.txt")).write_text(str(error) + "\n")
    schemas.append(version_schemas)
  if all(item is not None for item in schemas):
    write_json(output / "schema-diff.json", compare(*schemas))
  else:
    recorder.failed = True
    (output / "schema-diff-error.txt").write_text("Both versions require successful CRD extraction; no diff produced.\n")
  write_json(output / "result.json", {"exit_code": int(recorder.failed), "versions": [old_version, new_version],
                                    "scope": "artifact-only; no install/startup/runtime verification"})
  return int(recorder.failed)


if __name__ == "__main__":
  if len(sys.argv) != 6 or sys.argv[1] != "probe":
    sys.exit("Usage: diff_crds.py probe OUTPUT WORK OLD_VERSION NEW_VERSION")
  sys.exit(probe(*sys.argv[2:]))
