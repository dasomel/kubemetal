# 19. krunkit / smolvm Compute Backend Feasibility (#94, #97)

증거 등급: `[measured-local]`(이 워크트리에서 실행한 명령+실측 출력) / `[upstream-doc]`(URL + 접근일
2026-09-28 + 인용) / `[unverified]`(확인 불가, 사실로 단정하지 않음). Colima/K3s 생명주기 조작,
설치, 이미지 pull은 이 조사 범위에서 수행하지 않았다 — 모두 읽기 전용 프로브다.

기존 `ComputeBackend` 계약(§1.1)은 `docs/04-architecture.md:54-106`이 canonical — 여기서
새 계약을 만들지 않고 그 표/mermaid를 그대로 인용한다.

---

## 0. 호스트 실측 (이 워크트리)

```
$ sw_vers        # ProductVersion 27.0, BuildVersion 26A428          [measured-local]
$ uname -m        # arm64                                            [measured-local]
$ sysctl -n hw.memsize  # 68719476736 (64GB)                         [measured-local]
$ colima version  # 0.10.3, git 00f6c297e92a82c04a4ab507db0a61435650d7e8 [measured-local]
$ limactl --version  # 2.2.0                                         [measured-local]
$ colima list
PROFILE      STATUS     ARCH       CPUS    MEMORY    DISK      RUNTIME       ADDRESS
default      Running    aarch64    6       12GiB     100GiB    docker+k3s
nqa-node2    Stopped    aarch64    2       3GiB      15GiB     docker+k3s     [measured-local]
$ command -v krunkit   # not found                                   [measured-local]
$ command -v smolvm    # not found                                   [measured-local]
$ brew search krunkit  # no formula in default taps ("runit"/"run-kit" 근접 매치) [measured-local]
$ brew search smolvm   # 매치 없음                                    [measured-local]
$ colima start --help | grep vm-type
  -t, --vm-type string   virtual machine type (qemu, vz, krunkit) (default "vz") [measured-local]
$ colima status --json
level=fatal msg="error retrieving current runtime: empty value"      [measured-local, 실패]
$ grep -n vmType ~/.colima/_lima/colima/lima.yaml ~/.colima/default/colima.yaml
/Users/m/.colima/_lima/colima/lima.yaml:1:vmType: vz
/Users/m/.colima/default/colima.yaml:162:vmType: vz                  [measured-local]
```

- 이 Mac은 64GB RAM → D4 sizing(64GB+→12GB/6CPU)과 실행 중인 `default` 프로필(12GiB/6CPU)이
  정확히 일치한다 — D4가 실측 호스트에서 실제로 적용되고 있음을 교차 확인. `[measured-local]`
- `colima start --vm-type krunkit`은 CLI 플래그로는 이미 노출되어 있다(colima 0.10.3). 그러나
  `krunkit` 바이너리 자체는 이 머신에 설치돼 있지 않다 — 별도 tap(`slp/krunkit`, deprecated →
  후속 tap)에서 설치해야 한다. `[measured-local]` + `[upstream-doc]`(아래 §1)
- `smolvm` CLI는 Homebrew 기본 탭에 없다 — 업스림 저장소에서 직접 빌드/설치해야 한다.
  `[measured-local]`

---

## 1. krunkit / libkrun GPU 경로 (#94 공유 섹션)

### 1.1 버전/전제조건

- Colima는 v0.10.0에서 `krunkit` vm-type을 GPU 지원과 함께 추가했다: "Addition of `krunkit`
  virtual machine type with GPU support." [upstream-doc, https://colima.run/announcements/colima-v0.10.0-release/, 접근 2026-09-28]
- Lima(colima의 기반) 쪽 문서: krunkit은 "Lima >= 2.0, macOS >= 14 (Sonoma+), Apple Silicon
  (arm64)"를 요구하며, GPU 지원은 "GPU support in the guest via Mesa's Venus Vulkan driver"이고
  컨테이너에서 GPU에 접근하려면 `--device /dev/dri`를 명시적으로 넘겨야 한다. krunkit 드라이버는
  "experimental"로 명시돼 있다. [upstream-doc, https://lima-vm.io/docs/config/vmtype/krunkit/, 접근 2026-09-28]
- colima 자체 AI 문서: "Krunkit is required for GPU access on Apple Silicon. Install it via
  Homebrew" — Kubernetes 언급 없음, GPU 기술 스택(Vulkan/Venus/virtio-gpu) 세부 언급도 없음.
  [upstream-doc, https://colima.run/docs/ai/, 접근 2026-09-28]
- 이 워크트리 macOS는 27.0(Sonoma 이후 세대) + arm64 — macOS/아키텍처 전제조건은 충족.
  `[measured-local]`. colima 0.10.3 ≥ 0.10.0 — 버전 전제조건도 충족. `[measured-local]`
- krunkit 바이너리 미설치 상태이므로 실제 `--vm-type krunkit` 기동은 **시도하지 않았다**(범위
  밖 — 프로파일 생성/기동은 금지된 작업). 따라서 "이 머신에서 krunkit이 실제로 동작하는지"는
  `[unverified]`.

### 1.2 GPU 파이프라인 — Metal이 아니라 Vulkan(Venus)

업스트림 세부 스택 — sinrega.org 블로그 원문(WebFetch로 2026-09-28 직접 재확인)을 그대로 인용하면:
"we can leverage on Venus to serialize Vulkan commands and MoltenVK to translate Vulkan shaders
to MSL (Metal Shading Language)"이고, 별도로 "virtio-gpu already provides the two main
mechanisms we need to build a transport for the GPU between the guest and the host: shared
memory and a communication channel"라고 명시한다.
[upstream-doc, https://sinrega.org/2024-03-06-enabling-containers-gpu-macos/, 접근 2026-09-28]
다만 "게스트 Vulkan 호출 → Venus 직렬화 → virtio-gpu 전송 → MoltenVK가 Metal로 번역"이라는 하나의
연속된 파이프라인 문장은 이 페이지에 그대로 존재하지 않는다 — 위 두 인용문을 조합한 이 조사의
해석이며, 그 조합(순서·인과관계) 자체는 `[unverified]`로 남긴다.

**AGENTS.md 불변식과의 관계 (중요)**: 이 경로는 **Vulkan API를 MoltenVK가 Metal로 변환**하는
것이지, 게스트 Linux 커널에 Metal/MPS가 노출되는 것이 아니다. 즉:

- `krunkit-container` 경로가 검증되더라도 "K8s가 Metal 연산을 실행한다"는 불변식 위반이 되지
  않는다 — 게스트는 여전히 Vulkan만 보고, Metal 변환은 호스트 프로세스(krunkit/MoltenVK)가
  담당한다. 이는 AGENTS.md의 "Metal GPU cannot be passed through to Linux VMs" 서술과도
  모순되지 않는다 — 애초에 패스스루되는 것은 Metal이 아니라 Vulkan(Venus)이다.
- 그러나 **MLX는 Metal 전용 런타임**이고 Vulkan/Venus 경로에서 MLX 자체가 게스트 안에서
  동작한다는 증거는 없다 — issue #94 원문도 "Do not assume MLX itself runs inside the Linux
  guest"라고 명시한다(issue-94.md:134). 즉 krunkit 경로가 증명하는 것은 "게스트 컨테이너가
  Vulkan GPU 워크로드(llama.cpp/ggml-vulkan 등)를 돌릴 수 있다"이지 "MLX가 K8s 안에서
  돌아간다"가 아니다. 이 구분을 문서/README에 반영해야 한다는 issue #94 §Documentation Gate와
  일치.

### 1.3 K3s가 이 GPU를 스케줄 가능한 리소스로 취급할 수 있는가

- 업스트림 kiac 프로젝트(krunkit 기반 멀티노드 K8s, Apple GPU 워커)의 README: Kubernetes
  1.32–1.37 지원, "Kubernetes resource publication: `device-plugin`, or `dra` on Kubernetes
  1.36+", 스택은 "krunkit, vmnet-helper, virglrenderer, Mesa's Venus driver, and MoltenVK".
  "Only `-gpu-N` workers publish a schedulable GPU resource and mount `/dev/dri` into GPU
  pods" while "Venus is exposed to every VM in a GPU cluster." 상태는 명시적으로 **"(alpha)"**,
  "This is real Apple GPU access through virtio-gpu/Venus and Vulkan, not CUDA compatibility."
  [upstream-doc, https://github.com/saiyam1814/kiac, 접근 2026-09-28]
- 이것은 **Colima 자체의 공식 기능이 아니라 제3자 프로젝트의 증거**다 — "device-plugin 또는
  DRA로 Apple GPU를 K8s allocatable 리소스로 publish하는 경로가 upstream 어딘가에 실제로
  존재한다"는 사실 증명(existence proof)이지, KubeMetal의 K3s(단일 노드, Colima vz/krunkit)에서
  바로 재현된다는 보장은 아니다. `/dev/dri`를 마운트하고 device-plugin을 배포하는 구체적인
  매니페스트/버전 조합은 이 조사에서 **실행/재현하지 않았다** → `[unverified]`.
- 결론: issue #94 Phase B의 핵심 질문("Kubernetes가 GPU를 allocatable resource로 관리 가능한가")에
  대해 **"가능성이 upstream에 존재함이 문서로 확인됨(partial 증거)"**이지, KubeMetal 자체 실측은
  전무하다. `docs/04-architecture.md:182-184`가 이미 명시한 "krunkit 기반 K3s Pod GPU 사용,
  Kubernetes allocatable accelerator, DRA/Kueue 동작을 실측한 evidence가 없음"과 정합적이며,
  이번 조사도 그 상태를 뒤집지 못한다.

---

## 2. Issue #94 — Colima krunkit 체크박스 매핑

| 항목 | 상태 | 근거/다음 단계 |
|---|---|---|
| Colima 버전 감지, krunkit-capable 버전 요구 | **next-step** | `src-tauri/src/commands/colima.rs:121-149`의 `start_cluster`는 `--vm-type=vz`를 하드코딩(줄 143) — 버전 감지도, vm-type 분기도 없음. `colima version --json` 파싱 추가 필요(정확한 플래그는 `colima version --help`로 재확인 요) |
| Apple Silicon/macOS 전제조건 감지 | **done-now(수동)** | `[measured-local]` arm64 + macOS 27.0 확인. 자동 감지 코드는 리포에 없음 — `[unverified]`(코드 부재) |
| krunkit 설치/버전 감지 | **blocked** | krunkit 바이너리 미설치. brew 기본 탭에 없어 `slp/krunkit`(deprecated) 등 서드파티 tap 필요 — 설치는 이번 조사 범위 밖 |
| capability 스키마에 `krunkit` experimental 추가 | **next-step** | 리포 전체에 `GpuCapability`/`capability_schema` 유사 심볼 없음(grep 결과 없음) — 스키마 자체가 아직 존재하지 않음. `docs/04-architecture.md:54-79` 표가 유일한 설계 산문 |
| 격리 Colima 프로필로 `--vm-type krunkit` 기동 | **blocked(범위 밖)** | 이 조사는 colima 생명주기 조작 금지 — 실행 안 함 |
| 컨테이너 기본 라이프사이클 확인 | **blocked(범위 밖)** | 상동 |
| GPU 가속 컨테이너 워크로드 확인 | **blocked** | krunkit 미설치 + 실행 금지 |
| GPU 미사용 시 첫 오류 캡처 | **blocked** | 상동 |
| 기존 `vz` 프로필/호스트 MLX 경로 불변 확인 | **done-now** | `colima list`/`colima list --json` 결과 `default` 프로필은 `docker+k3s` 런타임으로 Running이지만 이 출력에는 vm-type 컬럼 자체가 없다(재확인). `colima status --json`은 이 머신에서 `level=fatal msg="error retrieving current runtime: empty value"`로 실패해 vm-type을 주지 않는다. 대신 실행 중인 `default` 프로필의 lima 구성 파일 `~/.colima/_lima/colima/lima.yaml:1`과 `~/.colima/default/colima.yaml:162`을 읽으면 둘 다 `vmType: vz`다 — 코드 하드코딩(`--vm-type=vz`, `colima.rs:143`)과 실제 실행 중 구성이 일치함을 확인. 이번 조사(읽기 전용)로 건드리지 않았음. `[measured-local]` |
| Phase A Acceptance 4개 | **unverified 전체** | GPU 실행 자체를 시도하지 않았으므로 `supported/partial/unavailable` 어느 것도 주장 불가 — 정직하게 `unverified` |
| Phase B 전체(K3s 스케줄링/isolation/accounting) | **unverified**(제3자 existence-proof만 있음) | §1.3의 kiac 증거는 "가능성"이지 KubeMetal 재현이 아님 |
| Phase C(워크로드/성능 매트릭스) | **not-started** | Phase A/B 선행 필요 |
| Phase D(ComputeBackend contract 확장) | **partial-design-only** | `docs/04-architecture.md:54-106`가 이미 `krunkit-container`를 표/mermaid에 포함 — 코드 구현은 없음(grep 결과 없음) |

**Recommended 최소 next slice (issue #94)**: `slp/krunkit`(또는 후속 tap) 설치 후
`colima start --vm-type krunkit --kubernetes` 를 **별도 격리 프로필**(`--profile krunkit-poc`
등, 기존 `default`/`nqa-node2` 불변)로 1회 기동해 `colima status --json`과 `vulkaninfo --summary`
(게스트 내부)만 실측하는 것 — Phase A의 처음 3개 체크박스만 커버하며, K3s/Pod 단계는 별도 슬라이스로
분리해야 한다(issue 자체가 Phase A/B를 분리해 놓은 이유이기도 함).

---

## 3. smolvm 업스트림 정체성 (#97)

GitHub에 동명 저장소가 다수 존재해 혼동 위험이 있다(`agustif/smolvm`, `LoganGrasby/smolvm`,
`mmlb/smol-machines--smolvm`, `yermakoffivan/smolvm`, `craftsland/smolvm`,
`CelestoAI/SmolVM` 등). 이슈 #97 본문의 설명("Hypervisor.framework/KVM/WHP", "OCI images",
"network-off-by-default", "libkrun-based isolation")과 별점(6.4k)·조직 구조가 정확히 일치하는
것은 다음 하나뿐이다:

- **정체성**: `smol-machines/smolvm` (Rust) — "An embeddable, portable, branchable virtual
  machine to safely run Agents locally." [upstream-doc, https://github.com/smol-machines/smolvm, 접근 2026-09-28]
- **조직 구성**: `smol-machines` org는 `smolvm`(코어), `smol`(로컬/원격/임베디드 라이프사이클
  관리), `smolvm-sdk`(언어별 SDK), `libkrun`/`libkrunfw`(각각 `libkrun/libkrun`,
  `libkrun/libkrunfw`에서 포크해 smolVM 전용으로 수정), `smol-mcp`(AI 에이전트 통합용 MCP
  서버), `smolbench`(성능 벤치마크)로 구성. [upstream-doc, https://github.com/smol-machines, 접근 2026-09-28]
- **격리**: README "How It Works" 절 원문(WebFetch로 2026-09-28 재확인): "Each workload runs in
  a hardware-virtualized VM with its own guest kernel on Hypervisor.framework (macOS), KVM
  (Linux), or the Windows Hypervisor Platform (WHP) (Windows). libkrun is the VMM and
  libkrunfw supplies the guest kernel." [upstream-doc, https://github.com/smol-machines/smolvm,
  접근 2026-09-28]
- **네트워크/마운트 기본값**: README "Safe" 절 원문: "Networking is off by default, egress can be
  limited to named hosts, and code can use a credential without ever reading it." README
  "Branchable" 절 원문: "Checkpoints capture RAM, CPU state and disks; branches are
  copy-on-write children of a live machine." [upstream-doc, https://github.com/smol-machines/smolvm,
  접근 2026-09-28]
- **MCP 연동 요청 존재**: `smol-machines/smolvm` issue #1176이 로컬 smolvm API용 공식 MCP
  서버를 요청 — Claude Code/OpenCode 등 MCP 호환 에이전트 대상. AGENTS.md의 AgentRuntime 분리
  원칙(issue #97 Related: #63)과 직접 관련. [upstream-doc, https://github.com/smol-machines/smolvm/issues/1176, 접근 2026-09-28]
- 이 리포에는 smolvm CLI가 설치돼 있지 않고(`command -v smolvm` 실패, §0) 홈브류 기본 탭에도
  없다 — 설치 경로(소스 빌드/릴리스 바이너리)는 `[unverified]`, 이번 조사에서 설치를 시도하지
  않았다.

### 3.1 Issue #97 체크박스 매핑

| 항목 | 상태 | 근거/다음 단계 |
|---|---|---|
| smolvm 설치/버전, 호스트 가상화 지원 감지 | **blocked** | smolvm 미설치, 설치는 범위 밖. Hypervisor.framework 자체는 macOS 표준 프레임워크라 존재 확인만 가능(별도 명령 없음) |
| Apple Silicon Hypervisor.framework 경로 확인 | **partial** | arm64+macOS 27.0 확인(`[measured-local]`)이나 smolvm이 실제로 그 경로를 타는지는 미설치로 미확인 |
| 고정 OCI 이미지를 ephemeral VM에서 실행 | **blocked(범위 밖 + 미설치)** | 이미지 pull 금지 규칙과도 겹침 |
| 기본 네트워크 비활성 확인 | **unverified(문서상으로만)** | upstream 문서는 "off by default"라고 명시하나 로컬 실측 없음 |
| 명시적 read-only/read-write 볼륨 동작 확인 | **not-started** | 코드/실행 없음 |
| 종료 코드/stdout/stderr 전파 확인 | **not-started** | 상동 |
| cold start/steady-state 오버헤드 측정 | **not-started** | 상동 |
| smolvm/libkrun/runtime 버전 기록 | **blocked** | 설치 전까지 불가 |
| smolvm vs 직접 krunkit/libkrun 통합(#94) 비교 | **design-only** | 이슈 본문 자체가 이미 "Primary question"으로 이 비교를 제기 — 실측 데이터 없이는 답변 불가. 방향성 메모: smolvm은 이미 `libkrun` 포크 + CLI/SDK/MCP를 묶어 제공하므로, KubeMetal이 krunkit 통합을 처음부터 구현하는 대신 smolvm의 `launch/observe/evidence` 유사 기능을 `ComputeBackend`(`docs/04-architecture.md:54-106`) 어댑터로 감싸는 편이 구현량을 줄일 가능성이 있음 — 그러나 이는 `[unverified]` 가설이며 Phase A 스파이크 없이 결정 불가 |
| Phase B(어댑터 `smolvm-microvm`) | **not-started** | Phase A 선행 필요, 코드베이스에 `smolvm` 심볼 없음(grep 결과 없음, §0에서 이미 확인) |
| Phase C(격리 정책: network=off 기본, mount=none 기본 등) | **design-only** | krunkit AI 문서가 이미 `--mount=none`으로 볼륨 완전 비활성화가 가능하다고 언급하나(§1.1) 이는 krunkit-container 경로이지 smolvm 경로가 아님 — 별도 검증 필요 |
| Phase D(가속기 연구: virtio-gpu/Venus `--gpu`, `--cuda`는 별도 평가) | **not-started** | §1의 krunkit GPU 지식을 참고용으로 재사용 가능하나 smolvm 자체의 `--gpu` 플래그 존재/동작은 미확인 |
| Phase E(통합: #17/#23/#24/#35/#63, 회귀 테스트) | **not-started** | 상위 이슈들 자체가 이번 조사 범위 밖 |
| Acceptance criteria(재현 가능 테스트, 커널 분리, default-deny 증명, evidence, cleanup, host-mlx 무변경) | **unverified 전체** | 실행 자체가 없었음 — 정직하게 unverified로 남김 |

---

## 4. Owner decisions needed

1. **krunkit 설치 방법 확정** — deprecated된 `slp/krunkit` 대신 사용할 후속 tap/버전을 오너가
   확정해야 한다(리서치에서 정확한 현재 tap 이름을 재검증 못함, `[unverified]`).
2. **Phase A를 "격리 프로필 1회 PoC"로 축소할지 여부** — 이슈 원문 그대로면 Phase A~D가 매우
   크다. 최소 슬라이스(§2 Recommended)로 쪼개는 데 대한 승인이 필요하다.
3. **smolvm vs krunkit-container 우선순위** — #97의 Primary question("smolvm이 #94의 krunkit
   구현량을 줄일 수 있는가")에 답하려면 두 스파이크가 순서대로 필요하다. 어느 쪽을 먼저 할지
   (krunkit 직접 통합 먼저 vs smolvm 먼저)는 리소스/시간 배분 결정이라 오너 판단이 필요하다.
4. **GPU 문서 표현 승인선** — README/UI에 "Vulkan(Venus) 게스트 GPU 실행 가능"까지는 upstream
  문서로 뒷받침되지만, "K3s가 이를 allocatable resource로 관리 가능"은 제3자(kiac, alpha) 증거뿐
  이므로 이 표현 수위를 오너가 승인해야 문서 게이트(issue #94 Documentation Gate)를 통과한다.

## 5. Recommended next slice

**하나만**: `docs/04-architecture.md`의 `krunkit-container` 상태 표(줄 73-79)에 이번 조사에서
확인한 kiac(alpha, device-plugin/DRA on K8s 1.36+) existence-proof를 각주로 추가하고, 동시에
`src-tauri/src/commands/colima.rs:121-149`에 `vm_type: String` 파라미터를 추가해
`--vm-type=vz`(현재 하드코딩, 줄 143)를 `--vm-type={vm_type}`으로 바꾸되 **기본값은 여전히
`"vz"`로 고정**하는 최소 PR을 낸다 — 이것만으로 Phase A "krunkit capability를 스키마에 추가"의
첫 전제(호출 경로에서 vm-type을 선택 가능하게 함)를 코드 변경 없이 실제 krunkit 기동 없이도
충족시키고, 기존 `vz` 기본 경로를 전혀 바꾸지 않는다. krunkit 실제 기동/GPU 실측은 별도 후속
이슈로 남긴다(오너 결정 1, 2 해결 후).
