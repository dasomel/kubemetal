#!/usr/bin/env bash
# ==============================================================================
# gen_release_manifest.sh — 릴리스 stage의 무결성 매니페스트 생성/검증
#
# --verify는 CI staging gate이며, GitHub 공개 자산은 zip과 manifest뿐이다.
# stage에 실제로 배포할 zip·고지·SBOM을 모두 놓은 뒤 이 스크립트를 실행한다.
# 매니페스트 자신은 자기 해시를 담을 수 없으므로 입력 집합에서 제외한다. 나머지
# regular file은 전부 기록하며, 필수 파일이 없거나 비어 있으면 실패한다.
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
MANIFEST_NAME="release-manifest.json"

usage() {
  echo "사용법: $0 <stage-dir> <tag>" >&2
  echo "       $0 --verify <manifest> <stage-dir>" >&2
  echo "       $0 --self-test" >&2
  exit 2
}

git_sha() {
  git -C "$PROJECT_ROOT" rev-parse --verify HEAD 2>/dev/null || {
    echo "FAIL: git SHA를 확인할 수 없습니다." >&2
    exit 1
  }
}

generate() {
  local stage_dir="$1"
  local tag="$2"
  local manifest="${stage_dir}/${MANIFEST_NAME}"

  [ -d "$stage_dir" ] || { echo "FAIL: stage 디렉터리가 없습니다: $stage_dir" >&2; exit 1; }
  [ -n "$tag" ] || { echo "FAIL: tag가 비어 있습니다." >&2; exit 1; }

  run_manifest_tool generate "$stage_dir" "$tag" "$manifest" "$(git_sha)"
}

verify() {
  local manifest="$1"
  local stage_dir="$2"
  [ -f "$manifest" ] || { echo "FAIL: 매니페스트가 없습니다: $manifest" >&2; exit 1; }
  [ -d "$stage_dir" ] || { echo "FAIL: stage 디렉터리가 없습니다: $stage_dir" >&2; exit 1; }

  run_manifest_tool verify "$manifest" "$stage_dir"
}

run_manifest_tool() {
  python3 - "$@" <<'PY'
import datetime
import hashlib
import json
import os
import sys

mode = sys.argv[1]

def fail(message):
    print(f"FAIL: {message}", file=sys.stderr)
    sys.exit(1)

def digest(path):
    hasher = hashlib.sha256()
    with open(path, "rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()

def required_paths(tag):
    return {
        f"KubeMetal-{tag}-aarch64.zip",
        "LICENSE",
        "NOTICE",
        "THIRD-PARTY-NOTICES.md",
        "sbom-cyclonedx.json",
        "sbom-spdx.json",
    }

def stage_files(stage_dir, manifest_path):
    files = {}
    for root, dirs, names in os.walk(stage_dir):
        dirs.sort()
        for name in sorted(names):
            path = os.path.join(root, name)
            relative = os.path.relpath(path, stage_dir)
            if os.path.abspath(path) == manifest_path:
                continue
            if not os.path.isfile(path) or os.path.islink(path):
                fail(f"stage에는 일반 파일만 포함할 수 없습니다: {relative}")
            size = os.path.getsize(path)
            if size == 0:
                fail(f"stage 파일이 비어 있습니다: {relative}")
            files[relative] = {"path": relative, "sha256": digest(path), "size_bytes": size}
    return files

if mode == "generate":
    _, _, stage_arg, tag, manifest_arg, git_sha = sys.argv
    stage_dir = os.path.abspath(stage_arg)
    manifest_path = os.path.abspath(manifest_arg)
    actual = stage_files(stage_dir, manifest_path)
    missing = sorted(required_paths(tag) - actual.keys())
    if missing:
        fail(f"필수 stage 파일이 없거나 비어 있습니다: {', '.join(missing)}")
    if not actual:
        fail("stage에 기록할 파일이 없습니다.")
    data = {
        "tag": tag,
        "git_sha": git_sha,
        "generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z"),
        "files": [actual[path] for path in sorted(actual)],
    }
    with open(manifest_path, "w", encoding="utf-8", newline="\n") as output:
        json.dump(data, output, indent=2, sort_keys=True)
        output.write("\n")
    print(f"생성: {manifest_path} ({len(actual)}개 파일)")
    sys.exit(0)

if mode != "verify":
    fail(f"알 수 없는 작업입니다: {mode}")

_, _, manifest_arg, stage_arg = sys.argv
manifest_path = os.path.abspath(manifest_arg)
stage_dir = os.path.abspath(stage_arg)

try:
    with open(manifest_path, encoding="utf-8") as source:
        data = json.load(source)
except (OSError, json.JSONDecodeError) as error:
    fail(f"매니페스트 JSON을 읽을 수 없습니다: {error}")

if not isinstance(data, dict) or not all(key in data for key in ("tag", "git_sha", "generated_at", "files")):
    fail("매니페스트 필수 필드(tag, git_sha, generated_at, files)가 없습니다.")
if not isinstance(data["files"], list) or not data["files"]:
    fail("매니페스트 files가 비어 있거나 배열이 아닙니다.")
if not isinstance(data["tag"], str) or not data["tag"]:
    fail("매니페스트 tag가 비어 있거나 문자열이 아닙니다.")

recorded = {}
for entry in data["files"]:
    if not isinstance(entry, dict) or set(entry) != {"path", "sha256", "size_bytes"}:
        fail("매니페스트 파일 항목 형식이 올바르지 않습니다.")
    path = entry["path"]
    if not isinstance(path, str) or not path or os.path.isabs(path) or ".." in path.split(os.sep):
        fail(f"안전하지 않은 매니페스트 경로입니다: {path!r}")
    if path in recorded:
        fail(f"매니페스트에 중복 경로가 있습니다: {path}")
    recorded[path] = entry

actual = stage_files(stage_dir, manifest_path)

if recorded.keys() != actual.keys():
    fail("stage 파일 목록이 매니페스트와 다릅니다.")
missing = sorted(required_paths(data["tag"]) - actual.keys())
if missing:
    fail(f"필수 stage 파일이 없거나 비어 있습니다: {', '.join(missing)}")
for relative, expected in recorded.items():
    if expected != actual[relative]:
        fail(f"무결성 불일치: {relative}")

print(f"검증: {manifest_path} ({len(actual)}개 파일)")
PY
}

self_test() {
  local stage tag before after
  tmp="$(mktemp -d)"
  trap 'rm -rf -- "$tmp"' EXIT
  stage="${tmp}/stage"
  tag="v0.0.0-test"
  mkdir -p "$stage"
  printf 'fake app archive\n' > "${stage}/KubeMetal-${tag}-aarch64.zip"
  printf 'Apache-2.0\n' > "${stage}/LICENSE"
  printf 'notice\n' > "${stage}/NOTICE"
  printf 'third-party notices\n' > "${stage}/THIRD-PARTY-NOTICES.md"
  printf '{"bomFormat":"CycloneDX"}\n' > "${stage}/sbom-cyclonedx.json"
  printf '{"spdxVersion":"SPDX-2.3"}\n' > "${stage}/sbom-spdx.json"

  "$0" "$stage" "$tag"
  python3 -m json.tool "${stage}/${MANIFEST_NAME}" >/dev/null
  "$0" --verify "${stage}/${MANIFEST_NAME}" "$stage"
  before="$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["files"][0]["sha256"])' "${stage}/${MANIFEST_NAME}")"
  printf 'tampered\n' >> "${stage}/KubeMetal-${tag}-aarch64.zip"
  if "$0" --verify "${stage}/${MANIFEST_NAME}" "$stage" >/dev/null 2>&1; then
    echo "FAIL: 변조된 파일이 검증을 통과했습니다." >&2
    return 1
  fi
  "$0" "$stage" "$tag"
  after="$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["files"][0]["sha256"])' "${stage}/${MANIFEST_NAME}")"
  [ "$before" != "$after" ] || { echo "FAIL: 변조 후 digest가 바뀌지 않았습니다." >&2; return 1; }
  : > "${stage}/NOTICE"
  if "$0" "$stage" "$tag" >/dev/null 2>&1; then
    echo "FAIL: 0-byte 필수 파일이 생성을 통과했습니다." >&2
    return 1
  fi
  printf 'notice\n' > "${stage}/NOTICE"
  "$0" "$stage" "$tag"
  printf 'unlisted file\n' > "${stage}/unlisted.txt"
  if "$0" --verify "${stage}/${MANIFEST_NAME}" "$stage" >/dev/null 2>&1; then
    echo "FAIL: 미등록 stage 파일이 검증을 통과했습니다." >&2
    return 1
  fi
  echo "self-test 통과 (JSON, 변조, 0-byte 필수 파일, 미등록 stage 파일)"
}

case "${1:-}" in
  --self-test)
    [ "$#" -eq 1 ] || usage
    self_test
    ;;
  --verify)
    [ "$#" -eq 3 ] || usage
    verify "$2" "$3"
    ;;
  *)
    [ "$#" -eq 2 ] || usage
    generate "$1" "$2"
    ;;
esac
