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

# 수집 쪽의 아카이브 이름 형식. 설치 쪽 lock parser도 /와 :를 _로 해석한다.
image_archive_name() {
  printf '%s' "$1" | tr '/:' '_'
}

# `docker pull` 시점의 provenance와, `docker save`/`load` 뒤에도 남는 image ID를 함께
# 출력한다. RepoDigests가 없는 로컬 이미지 등은 ID를 provenance로 쓴다. 이 형식은
# download/install 양쪽에서만 쓰므로 여기 한 곳에서 고정한다.
# stdout: <resolved RepoDigest-or-ID><TAB><image ID>
image_digest_record() {
  local image="$1" repo_digest image_id

  # RepoDigests가 비어 있으면 `index`가 실패할 수 있다. 그 경우에도 ID는 반드시
  # 얻어야 한다 — 빈 provenance를 기록하거나 추측하지 않는다.
  repo_digest="$(docker inspect --format='{{index .RepoDigests 0}}' "$image" 2>/dev/null)" || repo_digest=""
  image_id="$(docker inspect --format='{{.Id}}' "$image" 2>/dev/null)" || image_id=""
  [ -n "$image_id" ] || return 1
  [ -n "$repo_digest" ] || repo_digest="$image_id"
  printf '%s\t%s\n' "$repo_digest" "$image_id"
}

# digests.lock의 유일한 writer. 입력 파일은 image<TAB>provenance<TAB>image-ID 레코드다.
# C locale 정렬로 수집 순서와 무관하게 바이트 단위로 재현 가능한 lock을 만든다.
write_digest_lock() {
  local records="$1" destination="$2"
  LC_ALL=C sort -u "$records" > "${destination}.part" && mv "${destination}.part" "$destination"
}

# digests.lock의 유일한 parser. archive 이름에 대응하는 한 개의 lock 레코드만 출력한다.
# 빈/중복/잘못된 모든 레코드는 무결성 데이터가 검증 불가능한 상태이므로 실패다.
# stdout: <image><TAB><provenance><TAB><image ID>
digest_lock_record_for_archive() {
  local lock="$1" archive_name="$2"
  awk -F '\t' -v archive="$archive_name" '
    NF != 3 || $1 == "" || $2 == "" || $3 == "" { invalid = 1; next }
    {
      safe = $1
      gsub(/[\/:]/, "_", safe)
      if (safe ".tar.gz" == archive || safe ".tar" == archive) {
        matches++
        record = $0
      }
    }
    END {
      if (invalid || matches != 1) exit 1
      print record
    }
  ' "$lock"
}
