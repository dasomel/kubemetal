# 21 — Remote Compute / External Agent Integration Feasibility (#96, #63)

두 이슈 모두 "워크로드가 호스트 MLX 프로세스가 아닌 다른 곳에서 돈다"는 같은 문제의 변형이다. #96은
compute(어디서 GPU 연산이 도는가), #63은 agent(누가 추론/도구 호출을 오케스트레이션하는가) 축.
둘 다 아직 Rust 코드는 없다 [measured-local]: `grep -rn "ComputeBackend" src-tauri/src` → 0건,
`grep -rn "AgentRuntime" src-tauri/src` → 0건, `grep -rln "colab\|Colab" src-tauri/src` → 0건.
반면 `ComputeBackend`는 설계 문서에 이미 있다 [measured-local]: `grep -rn "ComputeBackend"
docs/04-architecture.md` → 54, 57, 62행(§1.1 산문 + mermaid 노드명), 사본 `docs/architecture.md:30`,
`docs/architecture-ko.md:29`. 즉 **계약은 문서에만 있고 구현은 없다**.


## 공유 섹션 — ComputeBackend / External Executor 계약

- 두 이슈가 공통으로 요구하는 것: **어디서 실행되는지와 무엇을 실행하는지를 분리**하고, 실행 결과를
  evidence로 남긴다. #96의 `ComputeBackend` 계약(`launch/cancel/observe/evidence`)과 #63의
  `AgentRun` 상관관계 필드(actor, provider, model, ComputeBackend, tool/MCP, policy, trace id)는
  같은 뼈대 — 실행 단위를 추적 가능한 레코드로 만든다는 점에서.
- 기존 유사 패턴이 이미 저장소에 있다: `src-tauri/src/commands/agent_execution_security.rs`가
  `AgentRiskLevel`(L0Query..L3ApprovedAction, `pub enum AgentRiskLevel` 정의는 19행)과
  `SessionAuthorityGrant`(agent_identity, session_id, allowed_tools, allowed_target_prefixes,
  만료시각, `pub struct SessionAuthorityGrant` 정의는 29행)를 정의한다. 이 역할은 파일 헤더
  주석(1–5행)이 "It is the fail-closed boundary a future L3 (approved action) executor must
  consume before any side effect is introduced"라고 명시한 것과 같다 [measured-local: `sed -n
  '1,40p' src-tauri/src/commands/agent_execution_security.rs` 재실행 — 헤더 주석 1–5행,
  `AgentRiskLevel` 정의 19행, `SessionAuthorityGrant` 정의 29행 확인. 이전 버전은 두 타입 정의가
  헤더 주석(1–5행) 자체에 있다고 잘못 인용했었다].
  #63의 `ExternalAgentRunner`/`AgentRuntime`는 이 권한 경계 위에 얹는 것이 합리적 — 새 신뢰 모델을
  또 만들 필요가 없다.
- `DeployTarget`(D26, `src-tauri/src/services/deploy_target.rs`)은 "K8s를 어디에 배포하는가"만
  다루고 컴퓨트/에이전트 축과 독립을 유지해야 한다는 이슈 원문의 요구는 이미 저장소 관례와 일치한다
  — D26/D30 모두 "축 분리"를 반복 강조하는 결정이었다.
- 두 축 모두 공통으로 거쳐야 할 관문: 외부 프로세스 스폰은 `resolve_cli_path`/`external_command`
  (`src-tauri/src/services/process.rs:19,64`), 실패는 조용히 다른 백엔드로 넘어가지 않고 표면화
  (D22–D25 "상태 조작 금지"와 동일 원칙 — #96 "No silent fallback"이 이를 재진술).

## Issue #96 — Colab Ephemeral GPU ComputeBackend

### Colab이 공식적으로 지원하는 것 (프로그래밍적/아웃바운드 연결)

- **자동화 제한** [upstream-doc, https://research.google.com/colaboratory/faq.html, 접속 2026-09-28]:
  "Colab prioritizes users who are actively programming in a notebook" — 무료 티어는 "remote control
  such as SSH shells, remote desktops"를 제한 대상으로 명시. 이슈가 요구하는 "outbound agent /
  no inbound SSH" 설계는 이 제약과 정면으로 부합한다 — inbound SSH를 안 쓰는 게 아니라 **못 쓴다**.
- **세션 한도** [upstream-doc, 위 URL, 2026-09-28]: "Runtimes will time out if you are idle." 무료
  티어는 "notebooks can run for at most 12 hours, depending on availability and your usage
  patterns." Pro+ "supports continuous code execution for up to 24 hours if you have sufficient
  compute units. Idle timeouts only apply if code execution terminates." → 이슈의
  `online|offline|busy|expired|unavailable|unverified` 상태 모델은 필수이지 선택이 아니다 — 12–24시간
  하드 상한이 실재.
- **GPU 보장 없음** [upstream-doc, 위 URL, 2026-09-28]: "Colab resources are not guaranteed and not
  unlimited," 무료 버전은 "access to expensive resources like GPUs is heavily restricted," "types of
  GPUs and TPUs that are available in Colab vary over time." → 이슈의 "Never hard-code T4/L4/A100,
  discover actual GPU" 규칙과 정확히 일치. 이 문서는 인바운드 연결 자체를 직접 언급하지 않았으므로
  "인바운드 불가"는 이 FAQ에서 직접 인용할 수 없다 — SSH 제한 조항으로부터의 합리적 추론이지 원문
  명시 사실은 아니다 **[unverified: "Colab이 인바운드 연결을 원천 차단한다"는 명시적 문장은 확인 못함,
  자동화/원격제어 제한 조항으로부터의 추론]**.

### 페어링/토큰 모델 (Google 자격증명 비저장)

- 이슈 설계(등록 → 하트비트 → job claim → 실행 → 업로드 → evidence)는 Google 계정 자격증명을 앱이
  절대 보유하지 않는 구조와 양립한다: 노트북 안에서 사용자가 이미 로그인한 Colab 런타임이 KubeMetal이
  발급한 **단명 pairing token**(앱 쪽에서 생성, Colab 쪽 agent가 아웃바운드로 제시)만 갖고, OAuth나
  세션 쿠키는 노트북 프로세스 밖으로 나오지 않는다. 이는 #63의 "공식 CLI 로그인 흐름을 앱이 절대
  소유/파싱하지 않는다" 원칙과 동일한 패턴 — 한 파일에서 두 이슈가 같은 자격증명 경계를 요구한다.
- **[unverified]**: 이 pairing-token 흐름의 실제 보안 속성(토큰 폐기 API, 재사용 방지, TLS 종단)은
  Phase B/E 구현 전까지 설계뿐이며 코드나 상위 문서 어디에도 존재하지 않는다.

### 데이터 경계 규칙

- 이슈 본문이 참조하는 #24(데이터 경계/라우팅 정책)는 이 저장소의 `docs/03-mvp-design.md` D-registry에
  아직 번호가 없다 [measured-local: `grep -n "D24" docs/03-mvp-design.md` → 매치 없음, #24 이슈 자체가
  이 워크트리에 제공되지 않아 원문 대조 불가]. → **[unverified]**: #24의 정확한 데이터 경계 규칙 문구.
  #96 완료 전 #24가 먼저 확정되어야 한다는 의존관계(P0/P1 백로그 순서)는 이슈 본문 그대로다.

### D26/D30과의 매핑

- **D26** (`docs/03-mvp-design.md:643`): DeployTarget은 "어느 K8s 클러스터에 배포할지"만 담당,
  colima 수명주기는 앱이 소유, 외부 클러스터는 소유하지 않음. `colab-ephemeral`은 K8s
  DeployTarget이 **아니다** — 이슈가 명시("Colab must not be modeled as a Kubernetes DeployTarget")
  한 그대로 D26의 범위 밖이라 충돌 없음. 다만 D26이 세운 원칙("배포 대상은 1급 개념, 하드코딩 금지")은
  `ComputeBackend` 레지스트리 설계에도 그대로 재사용 가능한 선례.
- **D30** (`docs/03-mvp-design.md:623`): 외부 클러스터 기본 통합은 "agent-only(L1)", 풀스택(L2)은
  옵트인, L0(읽기 전용)까지 있음. `colab-ephemeral`은 K8s 클러스터가 아니라 컴퓨트 백엔드이므로 D30의
  L0/L1/L2 계층 자체와는 직교(orthogonal)하지만, "본 스택의 정식 거처는 자체 k3s뿐, 외부 자원은
  기본적으로 최소 권한/옵트인"이라는 D30의 태도는 Colab을 "Experimental, opt-in, 기본 비활성"으로
  두는 이슈의 completion criteria와 같은 방향.
- **줄번호 재확인과 순서 주의**: `grep -n "^| D26 |\|^| D30 |" docs/03-mvp-design.md` 재실행 결과
  D26은 643행, D30은 623행으로 위 인용과 일치한다. 다만 D30(2026-08-06 결정)이 D26(2026-07-26
  결정)보다 나중에 만들어졌는데도 registry 파일에서는 더 앞쪽(623행)에 있고 D26이 더 뒤쪽(643행)에
  있다 — 이 registry는 줄 순서가 결정 시각순이 아니라 각 항목이 최초 삽입된 위치 순이므로, "번호가
  크거나 날짜가 늦은 결정일수록 줄번호도 크다"고 가정하면 안 된다. [measured-local]

### 완료 조건 체크리스트 — done-now / next-step / blocked

Phase A (공통 기반):
- [ ] Extract `ComputeBackend` from #94 — **blocked**: #94(krunkit feasibility) 원문이 이 워크트리에
  없고, `src-tauri/src/services/compute/` 디렉터리 자체가 없음 [measured-local: `find src-tauri/src
  -iname "*compute*"` → 매치 없음]. next-step: `#94` 이슈 원문 확보 후 `src-tauri/src/services/compute/mod.rs`
  신설.
- [ ] host-mlx를 무회귀로 적응 — **blocked**: 현재 MLX 실행 경로 소유 파일 확인 필요
  (`src-tauri/src/commands/mlx.rs` 존재 여부는 미확인, 이슈 본문이 "can remain in commands/mlx.rs
  initially"라고만 언급). next-step: `find src-tauri/src -iname "mlx*"`로 현재 소유 파일 특정.

Phase B~F (Colab agent/실행/evidence/UI): 전부 **blocked** — Phase A 공통 계약이 없는 상태에서 착수
불가라는 이슈 자체의 의존순서(P0 foundation → P1 policy/security → P1 Colab impl)를 그대로 따름.
어느 하위 체크박스도 코드/PR로 시작된 흔적이 없음 [measured-local: git log에 `colab` 관련 커밋 없음 —
`git log --all --oneline -i --grep=colab` 결과는 이 세션에서 실행하지 않았으므로 **[unverified]**로
남김. 코드 부재는 디렉터리 검색으로 확인(measured), 커밋 이력 전체 탐색은 미실행].

## Issue #63 — External CLI Agent Integration (Apache Maka 참조)

### Apache Maka 검증

[upstream-doc, https://github.com/apache/maka, 접속 2026-09-28]: 저장소는 실재하며 "a high-performance
agent workspace that keeps a complete record of everything it did"로 소개되고, **Apache Incubator
프로젝트(incubating)** 상태. 핵심 아키텍처는 "every model message, tool call, permission decision
and termination is an append-only RuntimeEvent" — 이슈가 요청한 "append-only RuntimeEvent model" 항목은
원문에 그대로 존재함을 확인. Desktop(Electron)/TUI/CLI가 "thin clients of a central Runtime Host"라는
구조도 이슈 원문과 일치. "sessions, settings and run records stay local," 사용자가 직접 공급하는
모델(cloud API/local/호환 게이트웨이)이라는 설명도 확인됨. → 이슈가 인용한 Maka 개념(Runtime
Host/AgentRun 분리, RuntimeEvent, 모델 공급자 추상화, 로컬 우선 저장)은 **[upstream-doc]로 검증됨**,
날조된 참조가 아니다. 다만 "compatible gateway 지원" "tool execution/permission model"의 세부 구현
방식(권한 스코프 문법, MCP 통합 방식)은 이번 페치로는 확인되지 않아 **[unverified]** — README 요약
수준 확인이며 코드 레벨 대조는 하지 않음.

### 이슈의 하드 제약 (설계는 이 안에서만)

이슈 본문 체크리스트 그대로, 재해석 없이: 공식 CLI로만 인증, `~/.codex`/`~/.claude` 등 자격증명
저장소 파싱 금지, 비공개/내부 엔드포인트 리버스엔지니어링 금지, 계정 로테이션/rate-limit 우회 금지,
문서화된 CLI 진입점만 호출, 각 CLI 어댑터를 독립적으로 비활성화 가능하게. 이 세션의 로컬 프로브는
이 제약을 준수 — `~/.codex`, `~/.claude`, `~/.gemini` 내부를 읽지 않았고, `command -v`/`--version`만
실행했다.

### 외부 CLI agent 스폰 방식 — `external_command`/`resolve_cli_path` 대조

- `src-tauri/src/services/process.rs:19-32`의 `resolve_cli_path`는 6개 경로만 탐색한다:
  `/opt/homebrew/bin`, `/usr/local/bin`, `/usr/bin`, `/bin`, `/usr/sbin`, `/sbin`
  (`SEARCH_PATHS`, `process.rs:10-17`). **이 목록에 `~/.local/bin`이 없다.**
- [measured-local] 이 Mac에서 실제 설치 경로:
  ```
  $ command -v codex claude gemini
  /Users/m/.local/bin/codex
  /Users/m/.local/bin/claude
  /opt/homebrew/bin/gemini
  $ codex --version   → codex-cli 0.158.0
  $ claude --version  → 2.1.283 (Claude Code)
  $ gemini --version  → 0.58.0
  ```
  즉 `gemini`는 현재 `SEARCH_PATHS`로 해석 가능하지만, **`codex`와 `claude`는 `resolve_cli_path`가
  찾지 못한다** — 이 머신의 설치 위치(`~/.local/bin`)가 탐색 경로 밖이기 때문. `external_command("codex")`/
  `external_command("claude")`를 그대로 호출하면 `process.rs:28-31`의 에러 문자열
  ("could not find executable 'codex'. Search paths: ...")을 그대로 반환하며 실패한다.
  이는 설계 이전에 나온 **구체적이고 재현 가능한 구현 장애물**이며, 이슈의 "executable discovery(PATH)"
  요구사항이 이 저장소의 기존 discovery 함수와 그대로 맞지 않는다는 뜻이다.
- next-step (정확한 위치): `process.rs:10`의 `SEARCH_PATHS` 배열에 사용자 홈 기준 CLI 설치 경로
  (`~/.local/bin`, 그리고 node/npm 전역 설치 시 흔한 `~/.npm-global/bin` 등)를 추가하거나, 외부 에이전트
  CLI 전용으로 사용자 설정 가능한 추가 탐색 경로를 `ExternalAgentRunner`가 별도로 주입하는 설계가
  필요하다. `SEARCH_PATHS`를 무조건 넓히면 시스템 바이너리 탐색(D5 원래 목적)과 목적이 섞이므로,
  agent CLI 전용 resolver를 새로 두는 편이 D26/D30이 반복해온 "축 분리" 원칙에 맞다.
- `external_command`(`process.rs:64-69`)가 자식에게 물려주는 `augmented_path()`도 같은 `SEARCH_PATHS`
  기반이라 동일 문제를 상속한다(`process.rs:42-60`).

### 워크스페이스 격리

- 이슈가 요구하는 항목(작업 디렉터리, 환경변수 allowlist, 권한 경계, 실행/이벤트 로깅)은
  `external_command`가 현재 제공하는 것(절대경로 해석 + PATH 보강)보다 범위가 넓다 — 작업 디렉터리
  고정, 환경변수 화이트리스트, stdout/stderr 캡처는 `process.rs`에 존재하지 않음
  [measured-local: 파일 전체 171줄, `Command::current_dir`/`env_clear` 호출 없음].
- `agent_execution_security.rs`의 `SessionAuthorityGrant`(`allowed_tools`, `allowed_target_prefixes`,
  만료시각)는 "무엇을 허용하는가"를 이미 모델링하지만 "어떤 프로세스로 스폰하는가"는 다루지 않는다 —
  즉 `ExternalAgentRunner`는 이 권한 모델과 `external_command`의 프로세스 스폰 사이를 잇는 새 계층이
  필요하다.

### Deliverables 체크리스트 — done-now / next-step / blocked

- [ ] Architecture comparison (KubeMetal vs Maka) — **done-now(부분)**: Maka의 핵심 개념(RuntimeEvent,
  Runtime Host/thin-client 프론트엔드, 로컬 우선 저장)은 이번 조사로 [upstream-doc] 검증됨(위 섹션).
  KubeMetal 쪽 대응 개념은 `agent_execution_security.rs`의 `AgentRiskLevel`/`SessionAuthorityGrant`뿐,
  RuntimeEvent에 대응하는 append-only 로그는 아직 없음. next-step: 이 비교표를 별도 ADR로 정식화.
- [ ] External Agent Runner design — **blocked**: 위에서 발견한 `resolve_cli_path` PATH 공백(codex/claude
  미해석)을 먼저 해결해야 PoC 자체가 동작하지 않음. next-step: `process.rs:10` 확장 또는 전용 resolver.
- [ ] Provider-policy/security boundary doc — **next-step**: 이슈의 9개 체크박스(공식 인증만, 자격증명
  파싱 금지 등)를 `docs/adr/`에 정식 ADR로 옮기고 D-registry에 새 D 번호 부여.
- [ ] Minimal PoC (generic CLI adapter) — **blocked**: 위 PATH 이슈 + 워크스페이스 격리 계층 부재.
- [ ] Codex/Claude/Gemini 어댑터 개별 평가 — **blocked**: PoC 선행 필요.
- [ ] implement/defer/keep-experimental 결정 — **blocked**: 위 항목 전부 미완.

## Owner decisions needed

1. `#94`(ComputeBackend 공통 계약)과 `#24`(데이터 경계 정책) 원문이 확정되기 전까지 `colab-ephemeral`은
   설계 문서 이상으로 진행할 수 없음 — 두 이슈를 먼저 닫을지, 이 이슈와 병행 설계할지 결정 필요.
2. `resolve_cli_path`의 `SEARCH_PATHS`를 확장할지, 아니면 외부 agent CLI 전용 별도 resolver를 새로
   만들지 — 시스템 바이너리 탐색(D5)과 사용자별 CLI 설치 탐색은 실패 모드가 다르다(전자는 macOS 버전에
   안정적, 후자는 사용자 설치 방식에 따라 제각각 — Homebrew/npm/`.local/bin`/`nvm` 등).
3. Colab 데이터 경계: 학습 데이터/모델 아티팩트를 Colab 런타임으로 내보내는 것을 "명시적 사용자 승인"
   조건으로 게이트할지, 아니면 처음부터 화이트리스트된 워크로드 타입(예: GPU 검증용 합성 텐서 연산)만
   허용할지 — 이슈의 "Explicit dataset transfer" 항목은 아직 정책 미정.
4. #63의 `AgentRuntime`을 `agent_execution_security.rs`의 `AgentRiskLevel`/`SessionAuthorityGrant` 위에
   얹을지, 별도 병렬 신뢰 모델로 둘지 — 얹는 쪽이 축 분리 원칙에 맞지만 오너 승인 필요.

## Recommended next slice

**`process.rs`의 `SEARCH_PATHS`가 `codex`/`claude`를 해석하지 못한다는 측정된 사실 하나만 놓고**,
`ExternalAgentRunner` PoC 이전에 먼저 처리할 최소 단위 작업: 사용자 홈 기준 CLI 설치 경로 탐색을
`resolve_cli_path`와 분리된 새 함수(예: `resolve_agent_cli_path`, 홈 디렉터리 하위 후보 경로 목록을
인자로 받음)로 추가하고, `resolve_cli_path_finds_system_shells`류 회귀 테스트와 동일한 패턴으로
"이 머신에 설치된 codex/claude/gemini 중 하나 이상을 실제로 해석하는지" 유닛 테스트를 추가한다. 이는
#96/#63 두 이슈의 어떤 아키텍처 결정에도 앞서 필요한, 되돌리기 쉬운 단일 파일 변경이며 지금 이미
측정된 구체적 실패를 고친다.
