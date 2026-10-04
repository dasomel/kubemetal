#!/usr/bin/env python3
"""Check resolved versions using Tauri's documented synchronization contract."""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PAIRS = (("tauri", "api", False), ("tauri-plugin-dialog", "plugin-dialog", True),
         ("tauri-plugin-opener", "plugin-opener", True))


def check(cargo, pnpm):
  errors = []
  # D2: plugins require exact versions, core only major.minor. CLI/build have
  # independent release lines; cost: no build guarantee, escape: run tauri build.
  for crate, package, exact in PAIRS:
    rust = set()
    for block in cargo.split("[[package]]")[1:]:
      if re.search(rf'^name = "{re.escape(crate)}"$', block, re.M):
        rust.update(re.findall(r'^version = "([^"]+)"$', block, re.M))
    js = set(re.findall(rf"^  ['\"]?@tauri-apps/{package}@([^'\"\s:(]+)['\"]?[:(]", pnpm, re.M))
    valid = lambda v: re.fullmatch(r"\d+\.\d+\.\d+(?:-[\w.-]+)?(?:\+[\w.-]+)?", v)
    if len(rust) != 1 or len(js) != 1 or not all(valid(v) for v in rust | js):
      errors.append(f"{crate}/{package}: missing, malformed or ambiguous resolved versions")
      continue
    rv, jv = next(iter(rust)), next(iter(js))
    # Pre-releases are compared exactly rather than silently equated to stable.
    key = lambda v: v if exact or '-' in v else '.'.join(v.split('.')[:2])
    if key(rv) != key(jv):
      errors.append(f"{crate} {rv} != @tauri-apps/{package} {jv} ({'exact' if exact else 'major.minor'})")
  return errors


def main():
  try:
    errors = check((ROOT / "src-tauri/Cargo.lock").read_text(),
                   (ROOT / "pnpm-lock.yaml").read_text())
  except OSError as exc:
    errors = [str(exc)]
  for error in errors:
    print(f"check_tauri_versions: {error}", file=sys.stderr)
  if not errors:
    print("check_tauri_versions: ok")
  return bool(errors)


if __name__ == "__main__":
  sys.exit(main())
