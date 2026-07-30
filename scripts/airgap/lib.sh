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
