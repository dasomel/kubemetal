# KubeMetal — developer entrypoints
# Recipes mirror the canonical commands in CLAUDE.md / README.md.
# Cluster values follow the D4 profile for this 64GB host; inside the app
# the VM size is auto-derived from detected RAM — the app remains canonical.

CARGO_MANIFEST := src-tauri/Cargo.toml

# 배포 대상(D26). 기본값은 colima — 지정하지 않으면 기존 동작 그대로다.
# 외부 클러스터 예: make provision CONTEXT=narwhal NAMESPACE=kubemetal \
#                     BRIDGE_HOST=192.168.56.1 STORAGE_CLASS=nfs-csi
CONTEXT ?= colima
NAMESPACE ?= $(if $(filter colima,$(CONTEXT)),default,kubemetal)
STORAGE_CLASS ?=
IMAGE_REGISTRY ?=
BRIDGE_HOST ?=

KUBECTL_CTX := kubectl --context $(CONTEXT)
KUBECTL := $(KUBECTL_CTX) -n $(NAMESPACE)

# colima는 D10 실측값(host.lima.internal)을 그대로 쓴다. 그 외 컨텍스트는 BRIDGE_HOST가
# 필수 — render.sh가 미지정을 거부한다(추측 주소를 클러스터에 실으면 파드가 조용히 죽는다).
RENDER_FLAGS := --namespace $(NAMESPACE) \
  $(if $(filter colima,$(CONTEXT)),--keep-bridge,--bridge-host $(BRIDGE_HOST)) \
  $(if $(STORAGE_CLASS),--storage-class $(STORAGE_CLASS)) \
  $(if $(IMAGE_REGISTRY),--image-registry $(IMAGE_REGISTRY))
# vite.config.ts는 strictPort: true — 포트가 막혀 있으면 대체 포트로 넘어가지 않고 즉시 죽는다.
VITE_PORT := 5173

.DEFAULT_GOAL := help

.PHONY: help install dev free-dev-port build bin app install-app check test test-e2e verify-airgap \
        lint fmt verify license-check clean-light cluster-up cluster-down provision provision-all kagent-up \
        preflight render export-gitops \
        forward forward-stop status index-code analyze-code serve-codegraph clean

help: ## 타깃 목록
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

install: ## 프론트엔드 의존성 설치 (pnpm)
	pnpm install

# 앞선 `tauri dev`가 비정상 종료하면 vite만 살아남아 $(VITE_PORT)를 계속 물고 있고,
# strictPort 때문에 다음 `make dev`가 "Port is already in use"로 즉시 죽는다.
# 이 프로젝트 디렉터리에서 뜬 프로세스일 때만 정리한다 — 다른 프로젝트의 dev 서버는
# 죽이지 않고 사실을 알린 뒤 중단한다(포트 소유자를 말없이 죽이지 않는다).
free-dev-port: ## dev 포트(5173)를 물고 있는 이 프로젝트의 잔여 vite 정리
	@pid=$$(lsof -ti tcp:$(VITE_PORT) -sTCP:LISTEN 2>/dev/null | head -1); \
	if [ -z "$$pid" ]; then exit 0; fi; \
	cwd=$$(lsof -a -p $$pid -d cwd -Fn 2>/dev/null | sed -n 's/^n//p' | head -1); \
	if [ "$$cwd" != "$(CURDIR)" ]; then \
	  echo "포트 $(VITE_PORT)를 다른 디렉터리의 프로세스가 사용 중입니다 (pid $$pid, cwd=$${cwd:-확인불가})."; \
	  echo "자동 종료하지 않습니다 — 직접 정리하거나 해당 서버를 멈춘 뒤 다시 실행하세요."; \
	  exit 1; \
	fi; \
	echo "잔여 vite 정리: pid $$pid (cwd=$$cwd)"; \
	kill $$pid 2>/dev/null || true; \
	for i in 1 2 3 4 5; do \
	  lsof -ti tcp:$(VITE_PORT) -sTCP:LISTEN >/dev/null 2>&1 || break; sleep 1; \
	done; \
	if lsof -ti tcp:$(VITE_PORT) -sTCP:LISTEN >/dev/null 2>&1; then kill -9 $$pid 2>/dev/null || true; sleep 1; fi; \
	if lsof -ti tcp:$(VITE_PORT) -sTCP:LISTEN >/dev/null 2>&1; then \
	  echo "포트 $(VITE_PORT) 해제 실패 — 수동 확인이 필요합니다."; exit 1; \
	fi

dev: free-dev-port ## 앱 개발 모드 실행 (vite는 beforeDevCommand로 자동 기동)
	pnpm tauri dev

build: clean-light ## 릴리스 번들(.app/.dmg) 빌드 — 헤드리스 dmg 이슈는 README 참고
	pnpm tauri build

bin: clean-light ## 순수 실행 바이너리 생성 (src-tauri/target/release/kubemetal)
	# 플레인 `cargo build`는 custom-protocol 피처가 빠져 devUrl(5173)을 바라보는
	# 빈 화면 바이너리가 된다 — 반드시 tauri CLI 경유로 빌드한다.
	pnpm tauri build --no-bundle
	@echo "실행 파일: src-tauri/target/release/kubemetal"

# 서명: 키체인에 유효한 codesigning 아이덴티티가 있으면 그것으로 서명한다 — TCC
# local-network 승인이 코드 아이덴티티에 묶이므로 ad-hoc(빌드마다 다른 식별자)로는
# 승인이 고정되지 않는다(mistakes-log 2026-07-27). 없으면 기존 ad-hoc 그대로.
# Developer ID로 교체할 때는 make app SIGNING_IDENTITY="Developer ID Application: ..."
SIGNING_IDENTITY ?= $(shell security find-identity -v -p codesigning 2>/dev/null | awk -F'"' 'NR==1 && NF>1 {print $$2}')

app: clean-light ## .app 번들만 생성 (dmg 생략 — 헤드리스 안전)
	$(if $(SIGNING_IDENTITY),APPLE_SIGNING_IDENTITY="$(SIGNING_IDENTITY)") pnpm tauri build --bundles app
	@echo "번들: src-tauri/target/release/bundle/macos/KubeMetal.app$(if $(SIGNING_IDENTITY), (서명: $(SIGNING_IDENTITY)))"

install-app: app ## .app 빌드 후 /Applications에 설치(기존본 교체)
	rm -rf /Applications/KubeMetal.app
	ditto src-tauri/target/release/bundle/macos/KubeMetal.app /Applications/KubeMetal.app
	@echo "설치 완료: /Applications/KubeMetal.app"

check: ## Rust 타입/컴파일 체크
	cargo check --manifest-path $(CARGO_MANIFEST)

test: ## Rust 단위 테스트 (경로 방어·가드레일 포함)
	cargo test --manifest-path $(CARGO_MANIFEST) --lib

test-e2e: ## 종합 E2E 자율 피드백 검증 스위트 실행 (합성데이터→파인튜닝→kagent진단→코딩패치)
	./scripts/e2e/run_full_e2e_verification.sh

# 호스트 네트워크를 건드리지 않고 "레지스트리 접근 0" 조건을 kubelet에 강제해 판정한다.
verify-airgap: ## 폐쇄망 기동 가능성 검증 (imagePullPolicy: Never 프로브)
	./scripts/airgap/verify_offline_images.sh

lint: ## clippy(-D warnings) + tsc + DESIGN.md 토큰 린트
	cargo clippy --manifest-path $(CARGO_MANIFEST) --all-targets -- -D warnings
	npx tsc --noEmit
	npx @google/design.md lint DESIGN.md

# NOTICE의 "금지 라이선스 없음" 주장이 lockfile과 어긋나면 여기서 깨진다(이슈 #9).
license-check: ## 번들 의존성 라이선스 정책 게이트 (self-test 포함)
	./scripts/release/check_licenses.sh --self-test
	./scripts/release/check_licenses.sh

fmt: ## rustfmt
	cargo fmt --manifest-path $(CARGO_MANIFEST)

verify: test lint license-check ## 완료 게이트 스위트 (test + lint + 라이선스 정책 + 웹 빌드)
	pnpm build

cluster-up: ## Colima K3s 시작 (vz/virtiofs, 6CPU/12GB — 64GB 호스트 D4 값)
	colima start --cpu 6 --memory 12 --vm-type=vz --mount-type=virtiofs --kubernetes

cluster-down: ## Colima 정지
	colima stop

# 매니페스트 목록은 `scripts/k8s/kustomization.yaml`이 단일 출처다 — 예전에는 여기와
# `provision.rs::MANIFESTS`에 같은 목록이 이중으로 있어 한쪽만 바뀌면 조용히 어긋났다.
# 렌더링(ns/브리지/StorageClass/레지스트리 치환)은 render.sh가 소유한다.

preflight: ## 배포 대상 사전점검 (도달성/기본SC/ArgoCD 소유권/Kyverno Enforce/브리지 후보)
	@echo "== 컨텍스트: $(CONTEXT) / 네임스페이스: $(NAMESPACE) =="
	@$(KUBECTL_CTX) --request-timeout=20s get nodes \
	  -o custom-columns=NODE:.metadata.name,IP:.status.addresses[?\(@.type==\"InternalIP\"\)].address
	@echo "-- StorageClass (default 표시 확인) --"; $(KUBECTL_CTX) get sc
	@echo "-- 이 네임스페이스를 소유한 ArgoCD Application (비어야 직접 apply 가능) --"
	@$(KUBECTL_CTX) get applications -A \
	  -o jsonpath='{range .items[?(@.spec.destination.namespace=="$(NAMESPACE)")]}{.metadata.name}{"\n"}{end}' \
	  2>/dev/null || echo "(ArgoCD 없음)"
	@echo "-- Kyverno Enforce 정책 --"
	@$(KUBECTL_CTX) get cpol -o jsonpath='{range .items[?(@.spec.validationFailureAction=="Enforce")]}{.metadata.name}{"\n"}{end}' \
	  2>/dev/null || echo "(Kyverno 없음)"
	@echo "-- 노드 서브넷과 겹치는 호스트 인터페이스 (브리지 후보) --"
	@ifconfig -a | awk '/^[a-z0-9]+:/{ifn=$$1} /inet /{print ifn, $$2, $$4}' | grep -v 127.0.0.1

render: ## 대상에 맞춰 매니페스트만 렌더링해 표준출력으로 (적용하지 않음)
	@./scripts/k8s/render.sh $(RENDER_FLAGS)

provision: ## MLOps 스택 적용 (mlflow/seaweedfs/bridge/prefect/secret)
	@$(KUBECTL_CTX) create namespace $(NAMESPACE) --dry-run=client -o yaml | $(KUBECTL_CTX) apply -f -
	@./scripts/k8s/render.sh $(RENDER_FLAGS) | $(KUBECTL_CTX) apply -f -

KAGENT_VERSION := $(shell cat scripts/helm/kagent-version.txt)

kagent-up: ## kagent 경량화 설치 (CRD 차트 선행, 버전 단일 출처 scripts/helm/kagent-version.txt, D33)
	$(KUBECTL_CTX) create namespace kagent --dry-run=client -o yaml | $(KUBECTL_CTX) apply -f -
	# CRD가 먼저다 — 본 차트 템플릿의 Agent/ModelConfig/RemoteMCPServer는 CRD 없이 렌더되지 않는다.
	helm upgrade --install kagent-crds oci://ghcr.io/kagent-dev/kagent/helm/kagent-crds \
	  --version $(KAGENT_VERSION) -n kagent --kube-context $(CONTEXT) --reuse-values
	helm upgrade --install kagent oci://ghcr.io/kagent-dev/kagent/helm/kagent \
	  --version $(KAGENT_VERSION) -n kagent -f scripts/helm/kagent-values.yaml --kube-context $(CONTEXT) --reuse-values

provision-all: provision kagent-up ## MLOps 스택 + kagent 종합 환경 한 번에 프로비저닝

# 외부 클러스터를 로컬 kagent로 진단한다(D34). 외부 클러스터에는 읽기 전용 SA 하나만 남고
# 워크로드는 전부 로컬에 있다 — 외부 노드가 kagent 이미지를 못 당기거나 여유 CPU가 없어도
# 성립하는 경로다. REMOTE_CONTEXT는 필수(대상은 설정이지 상수가 아니다, D26).
remote-reader-up: ## 외부 클러스터 진단 연결 (사용: make remote-reader-up REMOTE_CONTEXT=narwhal)
	@./scripts/k8s/remote-reader/setup-remote-reader.sh "$(REMOTE_CONTEXT)" "$(CONTEXT)"

remote-reader-down: ## 외부 클러스터 진단 연결 해제 (양쪽 클러스터에서 제거)
	@./scripts/k8s/remote-reader/teardown-remote-reader.sh "$(REMOTE_CONTEXT)" "$(CONTEXT)"

# GitOps 편입(D27). kubemetal은 Gitea에 **쓰지 않는다** — 파일만 narwhal 레포에 내려놓고,
# 실제 반영은 사용자가 narwhal의 scripts/gitops/push-to-gitea.sh로 수행한다. 그 경계 덕분에
# kubemetal이 Gitea 자격증명·포트포워딩·narwhal 레포 구조에 의존하지 않는다.
export-gitops: ## narwhal 레포에 ArgoCD Application + 렌더된 매니페스트 내보내기 (NARWHAL_DIR 필수)
	@test -n "$(NARWHAL_DIR)" || { echo "NARWHAL_DIR=/path/to/narwhal 를 지정하세요"; exit 1; }
	@test -d "$(NARWHAL_DIR)/gitops" || { echo "$(NARWHAL_DIR)/gitops 가 없습니다 — 경로를 확인하세요"; exit 1; }
	@test -n "$(BRIDGE_HOST)" || { echo "BRIDGE_HOST 미지정 — 검증된 브리지 주소 없이는 내보내지 않습니다 (make preflight로 후보 확인)"; exit 1; }
	@./scripts/k8s/render.sh $(RENDER_FLAGS) > "$(NARWHAL_DIR)/gitops/resources/kubemetal.yaml"
	@cp scripts/k8s/gitops/kubemetal-application.yaml \
	   "$(NARWHAL_DIR)/gitops/charts/narwhal-apps/templates/kubemetal.yaml"
	@echo "내보냄:"
	@echo "  $(NARWHAL_DIR)/gitops/resources/kubemetal.yaml"
	@echo "  $(NARWHAL_DIR)/gitops/charts/narwhal-apps/templates/kubemetal.yaml"
	@echo ""
	@echo "남은 단계 (narwhal 레포에서 직접):"
	@echo "  1) values.yaml 에 'kubemetal: {enabled: true}' 추가"
	@echo "  2) scripts/airgap/images.txt 에 kubemetal 이미지 4개 추가"
	@echo "  3) scripts/gitops/push-to-gitea.sh 'feat(kubemetal): MLOps 스택 편입'"

# kagent UI는 8090 — 8080은 D1에서 모델 서빙(mlx_lm.server)이 선점하는 포트다.
forward: ## 포트포워딩 시작 (5001 MLflow / 8333 S3 / 8888 Filer / 8090 kagent UI)
	nohup $(KUBECTL) port-forward svc/mlflow 5001:5000 >/dev/null 2>&1 &
	nohup $(KUBECTL) port-forward svc/seaweedfs 8333:8333 >/dev/null 2>&1 &
	nohup $(KUBECTL) port-forward svc/seaweedfs 8888:8888 >/dev/null 2>&1 &
	nohup $(KUBECTL_CTX) -n kagent port-forward svc/kagent-ui 8090:8080 >/dev/null 2>&1 &
	@sleep 2; for p in 5001 8333 8888 8090; do \
	  curl -s -o /dev/null -m 3 -w "localhost:$$p -> HTTP %{http_code}\n" http://localhost:$$p/ || true; \
	done

forward-stop: ## 포트포워딩 프로세스 종료 (mlflow/seaweedfs/kagent 대상)
	-pkill -f "port-forward.*svc/mlflow"
	-pkill -f "port-forward.*svc/seaweedfs"
	-pkill -f "port-forward.*svc/kagent-ui"

status: ## 클러스터·파드 상태 요약
	-colima status --json
	-$(KUBECTL) get pods

# CodeGraph는 선택 도구다(docs/18). 미설치 환경에서 알 수 없는 오류로 죽지 않도록 먼저 안내한다.
index-code: ## CodeGraph 코드베이스 심볼 인덱싱 및 지식 그래프 갱신
	@command -v codegraph >/dev/null 2>&1 || { echo "codegraph CLI가 없습니다 — docs/18-codegraph-graphify-analysis.md 설치 절차 참고"; exit 1; }
	codegraph init . || codegraph sync .

analyze-code: index-code ## CodeGraph 상태 및 심볼 영향도 리포트 출력
	codegraph status
	@echo "=== Top Symbol Impact Analysis ==="
	codegraph impact "get_cluster_status" || true

serve-codegraph: ## AI 에이전트용 CodeGraph MCP 서버 시작
	@command -v codegraph >/dev/null 2>&1 || { echo "codegraph CLI가 없습니다 — docs/18-codegraph-graphify-analysis.md 설치 절차 참고"; exit 1; }
	codegraph serve

clean-light: ## 최소 캐시 정리 — 최신 반영 보장 + 의존성 캐시 유지 (기본 빌드 선행)
	# 프론트 산출물은 항상 새로 (vite build는 수 초).
	rm -rf dist node_modules/.vite
	rm -f tsconfig.tsbuildinfo
	# 의존성 크레이트 캐시는 유지하고, kubemetal 크레이트의 fingerprint만 제거해
	# 임베드 자산(dist) 재포함을 포함한 자기 크레이트 재컴파일을 강제한다.
	rm -rf src-tauri/target/release/.fingerprint/kubemetal-*
	@echo "✓ 최소 캐시 정리 완료 (의존성 캐시 유지)"

clean: clean-light ## 전체 캐시 제거 — 이상 징후 시 수동 실행 (풀 리빌드, 수 분)
	rm -rf src-tauri/target/release/build
	rm -rf src-tauri/target/release/incremental
	rm -rf src-tauri/target/release/.fingerprint
	@echo "✓ 전체 캐시 정리 완료"
