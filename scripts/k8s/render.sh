#!/usr/bin/env bash
#=========================================================================
# render.sh — MLOps 스택 매니페스트를 대상 클러스터에 맞게 렌더링한다.
#
# 왜 존재하나: 매니페스트는 원래 colima k3s 한 곳만 가정했다(ns=default,
# ExternalName=host.lima.internal, 기본 StorageClass=local-path). 외부 클러스터에
# 배포하려면 이 세 값이 대상마다 달라진다. kustomize 오버레이를 그때그때 만들어
# 치환하고, Rust(provision)·Makefile·GitOps export가 **모두 이 스크립트를 거친다** —
# 렌더링 규칙이 세 곳에 복제되지 않게 하기 위해서다.
#
# 사용법:
#   render.sh --namespace kubemetal --bridge-host 192.168.56.1
#   render.sh --namespace default   --keep-bridge            # colima 기존 동작
#   render.sh --namespace kubemetal --bridge-host 1.2.3.4 --storage-class nfs-csi
#
# 렌더 결과는 stdout으로 나간다. 로그는 stderr로 — `$(render.sh ...)`가 로그를
# 매니페스트로 삼키지 않도록.
#=========================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

NAMESPACE=""
STORAGE_CLASS=""
BRIDGE_HOST=""
KEEP_BRIDGE=0
IMAGE_REGISTRY=""
# 브리지가 IP일 때만 쓰인다(아래 설명 참조). 8080은 D1의 모델 서빙 포트,
# 8081은 docs/08에서 kagent가 실제로 소비한 포트다 — 둘 다 열어 둔다.
BRIDGE_PORTS="8080,8081"

log() { echo "[render] $*" >&2; }
die() { echo "[render] 오류: $*" >&2; exit 1; }

is_ipv4() {
  case "$1" in
    ''|*[!0-9.]*) return 1 ;;
    *) [ "$(echo "$1" | tr -cd '.' | wc -c)" -eq 3 ] ;;
  esac
}

# Kubernetes의 ExternalName은 **DNS 이름만** 허용한다. IP를 넣으면 CoreDNS가
# `192.168.56.1.`로 CNAME을 만들고, 그건 유효한 호스트명이 아니라 조회가 NXDOMAIN으로
# 끝난다(2026-07-26 narwhal 실측 — 파드는 다 Running인데 브리지만 조용히 죽어 있었다).
#
# 그래서 IP 대상에서는 셀렉터 없는 Service + EndpointSlice로 바꾼다. 이 방식은 D10의
# "포트를 선언하지 않는다"는 성질을 포기한다 — Endpoints는 포트가 필수다. colima처럼
# DNS 이름을 쓸 수 있는 대상은 기존 ExternalName 그대로다.
rewrite_bridge_to_endpoints() {
  local host="$1" ports="$2" out="$3"
  {
    echo "apiVersion: v1"
    echo "kind: Service"
    echo "metadata:"
    echo "  name: mac-gpu-service"
    echo "  namespace: default"
    echo "spec:"
    echo "  ports:"
    echo "$ports" | tr ',' '\n' | while read -r p; do
      [ -n "$p" ] || continue
      echo "    - name: host-${p}"
      echo "      port: ${p}"
      echo "      targetPort: ${p}"
      echo "      protocol: TCP"
    done
    echo "---"
    echo "apiVersion: discovery.k8s.io/v1"
    echo "kind: EndpointSlice"
    echo "metadata:"
    echo "  name: mac-gpu-service-host"
    echo "  namespace: default"
    echo "  labels:"
    echo "    kubernetes.io/service-name: mac-gpu-service"
    echo "addressType: IPv4"
    echo "endpoints:"
    echo "  - addresses:"
    echo "      - ${host}"
    echo "    conditions:"
    echo "      ready: true"
    echo "ports:"
    echo "$ports" | tr ',' '\n' | while read -r p; do
      [ -n "$p" ] || continue
      echo "  - name: host-${p}"
      echo "    port: ${p}"
      echo "    protocol: TCP"
    done
  } > "$out"
}

while [ $# -gt 0 ]; do
  case "$1" in
    --namespace)      NAMESPACE="${2:-}"; shift 2 ;;
    --storage-class)  STORAGE_CLASS="${2:-}"; shift 2 ;;
    --bridge-host)    BRIDGE_HOST="${2:-}"; shift 2 ;;
    --keep-bridge)    KEEP_BRIDGE=1; shift ;;
    --image-registry) IMAGE_REGISTRY="${2:-}"; shift 2 ;;
    --bridge-ports)   BRIDGE_PORTS="${2:-}"; shift 2 ;;
    *) die "알 수 없는 인자: $1" ;;
  esac
done

[ -n "$NAMESPACE" ] || die "--namespace는 필수다"

# 브리지 주소는 명시하거나 명시적으로 유지하거나 — 둘 중 하나여야 한다.
# 이걸 선택 인자로 두면 외부 클러스터에 host.lima.internal이 그대로 실려 나가고,
# 파드는 DNS 해석 실패로 조용히 죽는다. 침묵 대신 실패를 택한다(D22-D25).
if [ "$KEEP_BRIDGE" -eq 0 ] && [ -z "$BRIDGE_HOST" ]; then
  die "--bridge-host <주소> 또는 --keep-bridge 중 하나를 지정해야 한다. \
브리지 주소를 검증하지 못했다면 배포를 중단하라 — 추측값을 실으면 안 된다."
fi
if [ "$KEEP_BRIDGE" -eq 1 ] && [ -n "$BRIDGE_HOST" ]; then
  die "--keep-bridge와 --bridge-host는 함께 쓸 수 없다"
fi

command -v kubectl >/dev/null 2>&1 || die "kubectl을 찾을 수 없다 (kustomize 내장 필요)"

# kustomize의 resources 루트는 절대경로일 수 없고(`cannot be absolute`), 오버레이를 base
# 안에 두면 사이클로 판정한다(`cycle detected`). 그래서 base를 리포 밖 임시 디렉터리로
# 복사해 `../base`라는 상대경로를 만든다. 리포에 임시 파일을 남기지 않는 부수 효과도 있다.
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "${WORK}/base" "${WORK}/overlay"
cp "${SCRIPT_DIR}"/*.yaml "${WORK}/base/"
OVERLAY="${WORK}/overlay"

if [ -n "$BRIDGE_HOST" ] && is_ipv4 "$BRIDGE_HOST"; then
  log "브리지 대상이 IP(${BRIDGE_HOST}) — ExternalName 대신 Service+EndpointSlice로 전환 (포트: ${BRIDGE_PORTS})"
  rewrite_bridge_to_endpoints "$BRIDGE_HOST" "$BRIDGE_PORTS" "${WORK}/base/mac-gpu-bridge.yaml"
fi

{
  echo "apiVersion: kustomize.config.k8s.io/v1beta1"
  echo "kind: Kustomization"
  echo "namespace: ${NAMESPACE}"
  echo "resources:"
  echo "  - ../base"

  # commonLabels/labels는 selector까지 건드린다 — Deployment의 selector.matchLabels는
  # 불변 필드라 기존 배포가 있으면 apply가 거부된다. 라벨은 base 매니페스트의 파드
  # 템플릿에 직접 박아뒀고, 여기서는 절대 추가하지 않는다.

  # 사내 레지스트리/미러로 이미지 출처를 갈아끼운다. 외부 클러스터는 Docker Hub에
  # 직접 나가지 못하거나(폐쇄망) 익명 rate limit에 걸리는 경우가 흔하다 — narwhal에서
  # 실측한 실패가 정확히 후자였다(2026-07-26: docker.io만 429가 HTML로 반환돼 모든
  # 이미지가 같은 config 다이제스트를 받고 ErrImagePull, quay.io는 정상).
  # kustomize의 images 트랜스포머를 쓰므로 base 매니페스트는 업스트림 참조를 유지한다 —
  # 폐쇄망 번들 목록(images_from_manifests)이 계속 업스트림에서 파생되도록.
  if [ -n "$IMAGE_REGISTRY" ]; then
    echo "images:"
    grep -h '^\s*image:\s*docker\.io/' "${WORK}/base"/*.yaml \
      | sed 's/^[[:space:]]*image:[[:space:]]*//' | sed 's/[[:space:]]*$//' | sort -u \
      | while IFS= read -r ref; do
          name="${ref%%:*}"                        # docker.io/org/img
          path="${name#docker.io/}"                # org/img
          log "이미지 재지정 ${name} -> ${IMAGE_REGISTRY}/${path}"
          echo "  - name: ${name}"
          echo "    newName: ${IMAGE_REGISTRY}/${path}"
        done
  fi

  if [ -n "$STORAGE_CLASS" ] || [ -n "$BRIDGE_HOST" ]; then
    echo "patches:"
  fi

  # DNS 이름일 때만 ExternalName을 유지한다. IP는 여기서 처리하지 않는다 —
  # 아래 rewrite_bridge_to_endpoints()가 base 파일 자체를 갈아끼운다.
  if [ -n "$BRIDGE_HOST" ] && ! is_ipv4 "$BRIDGE_HOST"; then
    log "브리지 ExternalName -> ${BRIDGE_HOST}"
    cat <<EOF
  - target:
      kind: Service
      name: mac-gpu-service
    patch: |-
      - op: replace
        path: /spec/externalName
        value: ${BRIDGE_HOST}
EOF
  fi

  if [ -n "$STORAGE_CLASS" ]; then
    log "Prefect PVC storageClassName -> ${STORAGE_CLASS}"
    cat <<EOF
  - target:
      kind: PersistentVolumeClaim
      name: prefect-data
    patch: |-
      - op: add
        path: /spec/storageClassName
        value: ${STORAGE_CLASS}
EOF
  fi
} > "${OVERLAY}/kustomization.yaml"

[ "$KEEP_BRIDGE" -eq 1 ] && log "브리지 ExternalName 유지 (base 값 그대로)"
log "네임스페이스 -> ${NAMESPACE}"

kubectl kustomize "$OVERLAY"
