# ADR-0004: Rust 의존성 공급망 정책

- 상태: 채택됨 (Accepted)
- 날짜: 2026-09-17

## 배경 (Context)

이슈 #36은 KubeMetal이 빌드 실행 전에 Rust/툴체인 의존성을 고정·검증할 것을
요구한다. 커밋 메시지를 그대로 믿지 않고 파일을 직접 읽어 확인한, 이미
저장소에 있는 것들:

- `src-tauri/Cargo.lock`이 커밋되어 있고, `make supply-chain-check`가
  `cargo deny check`(`deny.toml`, `[graph]`/`[advisories]`/`[sources]` 섹션만
  있음 — 이 ADR의 이전 초안과 달리 **bans나 license는 검사하지 않는다**)를
  실행한다. OSS 라이선스 정책은 별도 게이트인
  `scripts/release/check_licenses.sh`(`make license-check`, `make verify`의
  일부)가 담당한다.
- `rust-toolchain.toml`이 Rust 채널을 `1.98.0`으로 고정한다.
- `scripts/release/gen_sbom.sh`가 릴리스 SBOM에 Rust 빌드 의존성을 포함한다.
- `.github/workflows/supply-chain-fixture.yml`은 **수동**(`workflow_dispatch`만)
  네거티브 픽스처다: `fixtures/build-script-egress-probe/`에 대해 `cargo build`를
  실행하고, 이 크레이트의 `build.rs`는 `example.com`에 접속이 되면 빌드를
  실패시켜 `step-security/harden-runner`의 `egress-policy: block`이 잡이 실행될
  때 인가되지 않은 빌드 스크립트 네트워크 호출을 실제로 막는다는 것을
  증명한다. **이는 `.github/workflows/ci.yml`의 일부가 아니다** — 일반 CI는
  harden-runner를 쓰지 않고 매 push마다 빌드 시점 egress를 차단하지 않는다.
  이 픽스처는 수동 실행 시 그 통제가 작동한다는 증거일 뿐, 상시 게이트가
  아니다. 이 ADR의 이전 초안은 매 CI 실행마다 egress가 제한된다고 잘못
  암시했다.
- `.github/dependabot.yml`이 `/src-tauri`(cargo), `/`(npm), `/`(github-actions)에
  각각 최대 5개까지 주간 PR을 연다. Dependabot PR은 자동 머지가 아니라
  의존성 *제안*이며, 머지 전에 이 ADR이 정의하는 것과 동일한 리뷰를 거친다.

이 저장소에 아직 없는 나머지 통제는 신규 채택 크레이트에 대한 사람의 리뷰
기간과, 오염되거나 yank된 크레이트에 대한 대응 절차다 — 이 ADR은 이 두 가지만
정의한다.

## 결정 (Decision)

`src-tauri/Cargo.toml` / `src-tauri/Cargo.lock`의 Rust 의존성에 다음 두 정책을
채택한다. 직접 의존성뿐 아니라, lockfile 갱신으로 해석된 버전이나 소스가
바뀌는 전이 의존성(Dependabot이 제안한 것 포함)에도 적용된다.

### 1. 신규 공개·신규 채택 크레이트의 쿨링/리뷰 기간

`src-tauri/Cargo.toml`에 크레이트를 추가하거나 `src-tauri/Cargo.lock` 갱신으로
새 크레이트가 처음 도입되기 전, 변경 설명에 다음을 기록해야 한다:

1. 크레이트 이름, 정확한 버전, 소스, crates.io 공개일, 직접 의존성인지 아니면
   해석 과정에서 새로 선택된 전이 의존성인지.
2. 왜 이 크레이트가 필요한지, 어떤 기존 크레이트나 인트리 코드를 대체하는지,
   왜 잘 알려진 유지보수 중인 대안으로는 충분하지 않은지.
3. 브랜치에 대해 실행한 `make supply-chain-check`(advisory/차단 소스 정책)와
   `make license-check`(라이선스 정책) 결과, 그리고 크레이트의 다운로드
   추세·저장소 활동·메인테이너/소유권 이력·`rustsec` advisory DB에 아직 없는
   미공개 advisory에 대한 수동 확인. 크레이트 이름이나 README만 보고 판단해서는
   안 된다.

신규 공개된 크레이트는 crates.io 공개일로부터 최소 14일의 쿨링 기간을 거친
뒤에만 채택할 수 있다. 이 기간 중 채택은 긴급 보안 수정이나 빌드를 막는
결함에 한해서만 가능하며, 왜 기다릴 수 없는지를 설명하는 명시적 예외를 변경
설명에 기록해야 한다. 14일 이상 존재한 크레이트라도 신규 채택 시에는 동일한
리뷰가 필요하다 — 쿨링 기간이 리뷰를 대체하지 않으며, 크레이트의 소스를
바꾸거나 새 전이 의존성을 들여오는 Dependabot 제안 업그레이드에도 동일하게
적용된다.

이 저장소는 활성 인간 메인테이너가 한 명이다(`git log --format=%an`에
`dasomel`과 자동화된 `dependabot[bot]` 커밋만 보임) — 위 리뷰는 변경 설명에
기록된 `make supply-chain-check` / `make license-check` 결과에 근거한
셀프리뷰이지, 제2의 사람 승인자가 아니다. 제2의 메인테이너가 활동하게 되면
이 체크리스트에 더해 그 사람의 승인이 필요하지만, 체크리스트 자체는 제2의
사람이 존재한다고 가정해서는 안 된다.

### 2. yank되거나 오염된 크레이트의 롤백/격리

크레이트가 yank되었거나, 악성이거나, 오염되었거나, 그 외 안전하지 않다고
보고되면, 발견자는 공개가 민감할 경우 `SECURITY.md`에 따라 비공개 보안
리포트를 열고, 해당 해석에 영향을 주는 모든 의존성 업데이트의 머지를
중단한다. 이후 메인테이너는:

1. `src-tauri/Cargo.lock`에서 영향받는 모든 직접·전이 발생을 식별하고, 문제
   버전/소스와 마지막으로 알려진 정상 버전/소스를 기록하며, advisory나 보고서
   참조를 보존한다.
2. 마지막으로 알려진 정상 버전으로 고정하고, `cargo update --locked -p
   <crate>@=<version>`(또는 동등한 방법)으로 `src-tauri/Cargo.lock`을
   재생성한 뒤, `make supply-chain-check`, `make license-check`, CI를 다시
   실행한다. 알려진 정상 업스트림 버전이 없으면, 검증되지 않았거나 유동적인
   버전을 받아들이는 대신 릴리스를 차단한다.
3. 업스트림 수정이 나오기 전에 애플리케이션을 빌드해야 한다면, 명확히 이름
   붙인 리뷰 가능한 저장소 위치(예: 고정된 git 리비전이나 vendor된 경로를
   가리키는 `[patch]` 항목)에 알려진 정상 소스를 vendor하여 격리하고, 정확한
   소스 리비전·출처·제거 조건을 문서화한다. 이 격리는 임시적이며 장기적으로
   일반 crates.io 소스를 조용히 대체해서는 안 된다.
4. 롤백/격리된 빌드를 릴리스하기 전, `src-tauri/Cargo.lock` diff와
   소스/리비전 출처, 그리고 green 상태의 `make supply-chain-check` / CI 결과를
   변경 설명에 기록한다. `.github/workflows/supply-chain-fixture.yml`을 다시
   실행하는 것은 롤백 대상 크레이트에 대해 아무것도 검증하지 않는다 — 이는
   일반적인 빌드 egress 통제가 여전히 작동한다는 것만 증명하므로, 이 단계에
   필요하지 않다. 제2의 메인테이너가 활동 중이면 릴리스 전 승인하고, 활성
   메인테이너가 한 명이면 기록된 확인 결과가 리뷰 증거가 된다.

수정된 버전의 재채택은 §1에 따른 새 리뷰와, 업스트림 문제가 해결됐다는 확인
(advisory 철회, 자체 쿨링 기간을 마친 새 버전 공개, 또는 메인테이너 자신의
수정 감사)을 요구한다. 격리는 그 확인과 green `make verify` 이후에만
해제된다.

## 결과 (Consequences)

- 의존성 추가에 구체적인 최소 관찰 기간과 감사 가능한 리뷰 기록이 생긴다 —
  이 저장소가 매 빌드마다 강제하지 않는 통제가 아니라 실제로 실행되는 명령
  (`make supply-chain-check`, `make license-check`)에 근거한다.
- yank되거나 오염된 크레이트에 대해, 관련 없는 수동 픽스처를 마치 해당
  크레이트를 검증한 것처럼 재실행하지 않는 문서화된 롤백 경로가 생긴다.
- 이 정책은 오늘 당장 메인테이너 한 명으로도 집행 가능하며, 저장소에 제2의
  메인테이너가 생기면 다시 쓸 필요 없이 자동으로 제2 승인자 단계를 얻는다.

## 관련 (Related)

- 이슈 #36.
- `src-tauri/Cargo.lock`, `src-tauri/Cargo.toml`, `rust-toolchain.toml`,
  `deny.toml`, `scripts/release/check_licenses.sh`, `.github/dependabot.yml`,
  `.github/workflows/supply-chain-fixture.yml`.
