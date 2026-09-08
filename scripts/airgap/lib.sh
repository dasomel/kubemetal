#!/usr/bin/env bash
# ==============================================================================
# lib.sh — 폐쇄망 스크립트 공용 함수
#
# download_airgap_bundle.sh(수집)와 verify_offline_images.sh(검증)는 **같은 두 출처**
# (매니페스트의 image: 라인 + images-helm.txt)를 읽는다. 목록 파일만 공유하고 파싱
# 규칙을 각자 들고 있으면 규칙 쪽에서 다시 어긋나므로, 두 함수 모두 여기서만 정의한다.
# ==============================================================================

# `#` 주석과 빈 줄을 허용하는 이미지 목록 파일을 한 줄에 하나씩 출력한다.
# $1 = 목록 파일 경로
read_image_list() {
  local img
  # `|| [ -n "$img" ]` — 마지막 줄에 개행이 없어도 흘리지 않는다.
  while IFS= read -r img || [ -n "$img" ]; do
    img="${img%%#*}"              # 줄 끝 주석 제거
    img="${img//[[:space:]]/}"    # 공백·탭·CR 제거 (파라미터 확장 — 프로세스를 쓰지 않는다)
    [ -n "$img" ] && printf '%s\n' "$img"
  done < "$1"
  return 0
}

# scripts/k8s/*.yaml이 선언하는 image: 참조를 정렬·중복제거해 출력한다.
# $1 = 프로젝트 루트
manifest_images() {
  grep -rhoE 'image: *[^ ]+' "$1"/scripts/k8s/*.yaml | sed 's/image: *//' | sort -u
}

# 수집 파일명 규칙은 Rust 상태 조회와도 같아야 한다. lock을 archive에서 찾을 때도 이
# 함수만 쓴다.
image_archive_name() {
  printf '%s' "$1" | tr '/:' '_'
}

# pull 직후 runtime이 관측한 registry RepoDigest를 provenance로 기록한다. locally-built
# 이미지처럼 RepoDigests가 없는 경우에는 그 사실을 `unverified`로 명시한다. `docker save`
# / `load`는 이 값을 보존하지 않으므로 설치 무결성 판정에는 쓰지 않는다(D25).
# stdout: <registry@sha256 digest|unverified>
image_repo_digest_lock_value() {
  local image="$1" repo_digest
  repo_digest="$(docker inspect --format='{{range .RepoDigests}}{{println .}}{{end}}' "$image" 2>/dev/null | head -n 1)" || return 1
  if [ -z "$repo_digest" ]; then
    printf '%s\n' 'unverified'
  else
    printf '%s\n' "$repo_digest"
  fi
}

# docker save/load 뒤에도 남는 config digest를 가져온다. 이 값이 설치 시 실제로 비교하는
# content-addressed ID다.
# stdout: <sha256 image ID>
image_id_lock_value() {
  docker image inspect --format='{{.Id}}' "$1" 2>/dev/null
}

# digests.lock의 writer. 레코드는
# `<image:tag> <registry RepoDigest|unverified> <image ID>` 한 줄이며, 정렬해 수집 순서와
# 무관하게 재현 가능하게 만든다.
write_digest_lock() {
  local records="$1" destination="$2"
  LC_ALL=C sort -u "$records" > "${destination}.part" && mv "${destination}.part" "$destination"
}

# 이미지 또는 archive 이름으로 정확히 하나인 lock 항목을 읽는다. 형식 오류·중복·누락은
# 검증할 수 없는 상태라 실패로 돌린다.
digest_lock_record_for_archive() {
  local lock="$1" archive_name="$2"
  awk -v archive="$archive_name" '
    NF == 0 { next }
    NF != 3 || $1 !~ /:.+/ || ($2 != "unverified" && $2 !~ /@sha256:.+/) || $3 !~ /^sha256:.+/ { invalid = 1; next }
    {
      safe = $1
      gsub(/[\/:]/, "_", safe)
      if (safe ".tar.gz" == archive || safe ".tar" == archive) {
        matches++
        record = $0
      }
    }
    END {
      if (invalid || matches != 1) {
        exit 1
      } else {
        print record
      }
    }
  ' "$lock"
}

# 캐시 archive는 재-pull 없이 이전 provenance를 그대로 계승해야 한다.
preserve_digest_lock_record() {
  local lock="$1" archive_name="$2" destination="$3" record
  record="$(digest_lock_record_for_archive "$lock" "$archive_name")" || return 1
  printf '%s\n' "$record" >> "$destination"
}
