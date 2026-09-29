#!/usr/bin/env python3
"""Fail when the same fact lives in two places and the copies disagree.

Checks (mistakes-log: header version drift, react/react-dom mismatch):
  1. package.json, src-tauri/Cargo.toml [package] and tauri.conf.json (if it pins one) agree.
  2. pnpm-lock.yaml resolves react and react-dom to the same version — a mismatch
     white-screens the app ("Incompatible React versions") and no compiler sees it.
"""
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def cargo_version(text):
  section = re.search(r"^\[package\]\s*$(.*?)(?=^\[|\Z)", text, re.M | re.S)
  m = section and re.search(r'^version\s*=\s*"([^"]+)"', section.group(1), re.M)
  return m.group(1) if m else None


def lock_versions(text, name):
  return set(re.findall(rf"^  {re.escape(name)}@(\d[^\s:(]*)[:(]", text, re.M))


def check(root=ROOT):
  errors = []
  versions = {}
  versions["package.json"] = json.loads((root / "package.json").read_text()).get("version")
  versions["src-tauri/Cargo.toml"] = cargo_version((root / "src-tauri/Cargo.toml").read_text())
  tauri_conf = json.loads((root / "src-tauri/tauri.conf.json").read_text())
  if "version" in tauri_conf:
    versions["src-tauri/tauri.conf.json"] = tauri_conf["version"]
  for src, v in versions.items():
    if not v:
      errors.append(f"{src}: no version found")
  if len({v for v in versions.values() if v}) > 1:
    errors.append("app version diverges: " + ", ".join(f"{k}={v}" for k, v in versions.items()))

  lock = (root / "pnpm-lock.yaml").read_text()
  react, dom = lock_versions(lock, "react"), lock_versions(lock, "react-dom")
  if not react or not dom:
    errors.append("pnpm-lock.yaml: react/react-dom not found")
  elif react != dom:
    errors.append(f"react {sorted(react)} != react-dom {sorted(dom)} in pnpm-lock.yaml")
  return errors


if __name__ == "__main__":
  errs = check()
  for e in errs:
    print(f"check_versions: {e}", file=sys.stderr)
  if not errs:
    print("check_versions: ok")
  sys.exit(1 if errs else 0)
