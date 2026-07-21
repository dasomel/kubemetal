# KubeMetal — developer entrypoints
# Recipes mirror the canonical commands in CLAUDE.md / README.md.
# Cluster values follow the D4 profile for this 64GB host; inside the app
# the VM size is auto-derived from detected RAM — the app remains canonical.

CARGO_MANIFEST := src-tauri/Cargo.toml
KUBECTL := kubectl --context colima -n default

.DEFAULT_GOAL := help

.PHONY: help install dev build bin app install-app check test lint fmt verify \
        cluster-up cluster-down provision forward forward-stop status clean

help: ## 타깃 목록
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

install: ## 프론트엔드 의존성 설치 (pnpm)
	pnpm install

dev: ## 앱 개발 모드 실행 (vite는 beforeDevCommand로 자동 기동)
	pnpm tauri dev

build: ## 릴리스 번들(.app/.dmg) 빌드 — 헤드리스 dmg 이슈는 README 참고
	pnpm tauri build

bin: ## 순수 실행 바이너리 생성 (src-tauri/target/release/kubemetal)
	pnpm build
	cargo build --release --manifest-path $(CARGO_MANIFEST)
	@echo "실행 파일: src-tauri/target/release/kubemetal"

app: ## .app 번들만 생성 (dmg 생략 — 헤드리스 안전)
	pnpm tauri build --bundles app
	@echo "번들: src-tauri/target/release/bundle/macos/KubeMetal.app"

install-app: app ## .app 빌드 후 /Applications에 설치(기존본 교체)
	rm -rf /Applications/KubeMetal.app
	ditto src-tauri/target/release/bundle/macos/KubeMetal.app /Applications/KubeMetal.app
	@echo "설치 완료: /Applications/KubeMetal.app"

check: ## Rust 타입/컴파일 체크
	cargo check --manifest-path $(CARGO_MANIFEST)

test: ## Rust 단위 테스트 (경로 방어·가드레일 포함)
	cargo test --manifest-path $(CARGO_MANIFEST) --lib

lint: ## clippy(-D warnings) + tsc + DESIGN.md 토큰 린트
	cargo clippy --manifest-path $(CARGO_MANIFEST) --all-targets -- -D warnings
	npx tsc --noEmit
	npx @google/design.md lint DESIGN.md

fmt: ## rustfmt
	cargo fmt --manifest-path $(CARGO_MANIFEST)

verify: test lint ## 완료 게이트 스위트 (test + lint + 웹 빌드)
	pnpm build

cluster-up: ## Colima K3s 시작 (vz/virtiofs, 6CPU/12GB — 64GB 호스트 D4 값)
	colima start --cpu 6 --memory 12 --vm-type=vz --mount-type=virtiofs --kubernetes

cluster-down: ## Colima 정지
	colima stop

provision: ## MLOps 스택 매니페스트 적용 (mlflow/seaweedfs/bridge/secret)
	$(KUBECTL) apply -f scripts/k8s/

forward: ## 포트포워딩 시작 (5001 MLflow / 8333 S3 / 8888 Filer, nohup 상주)
	nohup $(KUBECTL) port-forward svc/mlflow 5001:5000 >/dev/null 2>&1 &
	nohup $(KUBECTL) port-forward svc/seaweedfs 8333:8333 >/dev/null 2>&1 &
	nohup $(KUBECTL) port-forward svc/seaweedfs 8888:8888 >/dev/null 2>&1 &
	@sleep 2; for p in 5001 8333 8888; do \
	  curl -s -o /dev/null -m 3 -w "localhost:$$p -> HTTP %{http_code}\n" http://localhost:$$p/ || true; \
	done

forward-stop: ## 포트포워딩 프로세스 종료 (mlflow/seaweedfs 대상만, 인자 순서 무관)
	-pkill -f "port-forward.*svc/mlflow"
	-pkill -f "port-forward.*svc/seaweedfs"

status: ## 클러스터·파드 상태 요약
	-colima status --json
	-$(KUBECTL) get pods

clean: ## 웹 빌드 산출물 제거 (Rust target은 cargo clean을 직접 실행)
	rm -rf dist
