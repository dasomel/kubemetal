# CuMetal Feasibility — Issue #84

이 문서는 #84(CuMetal CUDA Compatibility on Apple Silicon)의 Phase 1~4 acceptance
checkbox를 done-now / next-step / blocked로 매핑한다. 코드는 아직 하나도 작성되지
않았다 — 이 문서는 구현 전 feasibility·evidence 수집 결과다.

## 1. 업스트림 CuMetal이 실제로 무엇인가

- 프로젝트: **CuMetal**, 저장소 `Lulzx/cuda-metal`
  ([Lulzx/cuda-metal](https://github.com/Lulzx/cuda-metal)) [upstream-doc,
  https://github.com/Lulzx/cuda-metal, 접근 2026-09-28, README 원문 재확인]:
  README는 스스로를 "CuMetal is a CUDA compiler and runtime for Apple Silicon"으로 소개하며,
  "It compiles supported CUDA C++ and PTX into Metal kernels, so you can run existing CUDA
  code on your Mac's GPU without rewriting it in Metal"이라고 명시한다.
- 라이선스: **Apache-2.0** [upstream-doc, https://github.com/Lulzx/cuda-metal, 접근 2026-09-28 —
  `gh api repos/Lulzx/cuda-metal/license` 응답: `license.spdx_id: "Apache-2.0"`,
  `license.name: "Apache License 2.0"`, `path: "LICENSE"`] — 저장소 라이선스 필드 기준.
- 활동성 [upstream-doc, https://github.com/Lulzx/cuda-metal, 접근 2026-09-28,
  `gh api repos/Lulzx/cuda-metal`/`.../commits --paginate` 응답 재확인]: stargazers_count 192,
  forks_count 15, open_issues_count 5, 전체 커밋 650(페이지네이션 라인 수), 최신 릴리스 `v0.6.0`
  (published_at 2026-09-23T06:51:37Z) — 활발히 릴리스되는 프로젝트. 단일 유지보수자 의존(bus
  factor) 여부는 [unverified].
- 컴파일/번역 경로 [upstream-doc, https://github.com/Lulzx/cuda-metal, 접근 2026-09-28, README
  "How it works" 절 원문]: `CUDA C++ / PTX → CuMetal compiler → Metal Shading Language → Apple
  tools → metallib` (README 코드블록 그대로). "Direct CUDA C++ compilation uses typed CuMetal IR
  and embeds the compiled Metal library in the executable, with no first-launch PTX JIT."
- 지원 CUDA 서브셋:
  - v0.6.0 릴리스 노트 "Important limits" 절 원문: "Apple Silicon/macOS only. There is no SASS
    execution, multi-GPU transport, NVIDIA management telemetry or graphics interop. Library
    shims are tested subsets, and NVIDIA bitstream or numerical parity is not claimed."
    [upstream-doc, https://github.com/Lulzx/cuda-metal/releases/tag/v0.6.0, 접근 2026-09-28]
  - README "Limits" 절 원문: "SIMD/warp width is fixed at 32. Multi-GPU, peer access, and
    graphics-API interop are unsupported."; "Cooperative grids, dynamic launch, graphs, and
    textures have bounded or incomplete support."; "FP64 uses emulation with mode-dependent
    precision; it is not native Metal FP64." [upstream-doc, https://github.com/Lulzx/cuda-metal,
    접근 2026-09-28]
  - v0.6.0 릴리스 노트 원문: "Extended-precision carry chains (add.cc/addc, sub.cc/subc,
    mad.cc/madc) are unsupported on every PTX backend and refuse the kernel." [upstream-doc,
    https://github.com/Lulzx/cuda-metal/releases/tag/v0.6.0, 접근 2026-09-28]
  - v0.6.0 릴리스 노트 원문: "AMReX's CUDA backend runs on the Apple GPU with no AMReX patches. ...
    The HeatEquation tutorial agrees with a CPU build to 5.5e-09 relative after 200 steps." 같은
    릴리스 노트가 PhysX position-based dynamics에 대해: "The PhysX PBD scenes carry the four
    limits recorded in their README, including spring constraints that have no effect. Particle
    systems of 1024 or more stop integrating."라고 알려진 결함을 명시한다. [upstream-doc,
    https://github.com/Lulzx/cuda-metal/releases/tag/v0.6.0, 접근 2026-09-28]
  - 요구사항 [upstream-doc, https://github.com/Lulzx/cuda-metal, 접근 2026-09-28, README
    "Install" 절 원문]: "Requires Apple Silicon and macOS 14 or newer."
- 설치 경로 [upstream-doc, https://github.com/Lulzx/cuda-metal, 접근 2026-09-28, README "Install"
  절 원문 그대로]: `brew install lulzx/tap/cumetal`
  (커스텀 tap, Homebrew core 아님), 이어서 `cumetal doctor`,
  `cumetalc kernel.cu -o kernel`, `./kernel`.
- 이슈 본문이 요구하는 "CuMetal이 실제로 무엇인가"에 대한 답: 위 내용이 현재
  확인 가능한 전부다. 별도 백서/논문, 벤치마크 스위트 공식 문서, PyTorch CUDA
  바인딩 지원 여부에 대한 업스트림의 명시적 입장은 이번 조사에서 찾지 못했다 —
  이 세 가지는 [unverified]로 남긴다.

## 2. 로컬 설치/탐지 가능성 (read-only probe, 측정)

```
$ sw_vers
ProductName:    macOS
ProductVersion: 27.0
BuildVersion:   26A428

$ uname -m
arm64

$ xcrun --version
xcrun version 72.

$ xcrun -f metal
xcrun: error: unable to find utility "metal", not a developer tool or in PATH

$ clang --version
Apple clang version 21.0.0 (clang-2100.3.34.2)
Target: arm64-apple-darwin27.0.0
InstalledDir: /Library/Developer/CommandLineTools/usr/bin

$ command -v cumetal   # (no output)
$ command -v nvcc      # (no output)

$ brew info --json=v2 cumetal
Error: No available formula with the name "cumetal". Did you mean metals?
```

[measured-local, 2026-09-28]:
- macOS 27.0 (arm64) — 업스트림이 요구하는 "macOS 14+"는 여유 있게 충족.
- **`xcrun -f metal`이 실패한다** — Command Line Tools만 설치돼 있고 full Xcode의
  Metal 컴파일러 툴체인이 없다는 뜻. CuMetal의 `metallib` 생성 경로(Apple tools
  단계)가 이 Mac에서 그대로 동작할지는 Xcode 설치 없이는 [unverified].
- `cumetal`/`nvcc` 바이너리 모두 미설치 — 예상대로 `unavailable`.
- `brew info`는 기본 tap만 조회했고 커스텀 tap(`lulzx/tap`)을 추가하지 않았으므로
  "brew에 없다"는 이 프로브의 정상적인 결과이지 CuMetal의 배포 상태에 대한 증거가
  아니다 — tap 추가는 "설치"에 해당해 이 조사의 read-only 제약을 벗어난다.
- Clang은 Apple clang 21 (CLT) — CuMetal 빌드가 CLT만으로 충분한지 full Xcode를
  요구하는지는 [unverified] (README의 "compiler and Apple toolchain (see build
  documentation)" 문구가 build doc을 가리키지만 이번 조사에서 별도로 열람하지
  않았다).

## 3. 리포지토리 통합 지점 (코드 인용)

- `src-tauri/src/services/runtime_adapter.rs` — 현재 존재하는 어댑터 계층은
  `LocalInferenceRuntimeAdapter` trait (5-12행)과 두 구현체 `OmlxAdapter`(15-53행),
  `MlxLmAdapter`(56-90행)뿐이다. 이 trait은 로컬 **추론 서빙**(oMLX/mlx-lm)
  전용이고 `RuntimeAdapterDescriptor.capabilities`는
  `src-tauri/src/services/local_inference.rs`의 `RuntimeCapabilities` 구조체
  (openai_chat/embeddings/mcp 등, 21-30행 부근)를 쓴다 — CUDA
  compile/execute/parity 같은 필드가 없다. **CuMetal은 이 trait에 세 번째
  어댑터로 끼워 넣을 대상이 아니다**: 이슈가 정의한 `ComputeBackend` 계약
  (아래)과 스코프가 다르다.
- `docs/04-architecture.md` §1.1 "ComputeBackend 확장 모델" (54-79행 부근) —
  이미 `host-mlx` / `host-cumetal` / `krunkit-container` / `remote-kubernetes`
  4-way 분리를 **문서 수준에서 정의**해 두었고, `host-cumetal`을 "Experimental,
  핵심 검증: CUDA API/workload correctness·성능 (#84)"로 명시한다. 그러나
  `grep -rl "ComputeBackend" src-tauri/src`는 0건 — **이 계약은 Rust 코드로는
  아직 전혀 구현되지 않았다.**
- `src-tauri/src/services/process.rs` — `resolve_cli_path`(19-29행)와
  `external_command`(64-69행)가 모든 외부 CLI 스폰의 유일한 경로다.
  `cumetal doctor`/`cumetalc`를 스폰할 코드는 반드시 이 헬퍼를 거쳐야 하며
  (AGENTS.md "Spawn external CLIs through resolve_cli_path/external_command"),
  `SEARCH_PATHS`(9-16행)에 `/opt/homebrew/bin`이 포함돼 있어 Homebrew tap으로
  설치된 `cumetal`도 원칙적으로 탐지 가능하다 — 단, 커스텀 tap
  (`lulzx/tap/cumetal`)이라 Homebrew core formula 존재를 가정한 코드는 안 된다.
- `src-tauri/src/commands/metrics.rs` — D2 GPU 지표 수집(`ioreg -c
  IOAccelerator`, 160-169행 부근)이 이미 sudo-free 패턴의 선례다. CuMetal
  실행이 "실제 Apple GPU를 썼는가"(GPU provenance, Phase 1 항목)를 검증할 때
  동일한 `ioreg IOAccelerator` 경로로 실행 전후 GPU 활동을 교차 확인하는 것이
  가장 저위험 next-step이다 — 단 이것은 "이 프로세스가 GPU를 썼다"는 직접 증명이
  아니라 "실행 구간에 GPU 활동이 있었다"는 정황 증거임을 evidence에 명시해야
  한다.
- `research/README.md` — evidence 스키마는 OpenForge Research Evidence
  Collection Standard를 그대로 따르고, KubeMetal은 `environment` 라벨만 추가
  정의한다(`apple-silicon-dev-host-<ram-tier>` 등). 이슈 본문의
  `kubemetal.cumetal.v1` evidence JSON 스키마는 **이 표준과 별개의 자체 스키마**로
  제안돼 있다 — 두 스키마를 합칠지, `research/evidence/*.jsonl`에 어떻게
  얹을지는 결정이 안 됐다(§6 Owner decisions 참고).

## 4. 최소 vectorAdd/SAXPY/reduction smoke harness 설계 (구현 전 스케치)

현재 어떤 코드도 존재하지 않으므로 아래는 next-step 스케치이지 구현 보고가 아니다.

1. **Toolchain preflight** — `external_command("xcrun")` + `["-f", "metal"]`로
   Metal 컴파일러 존재를 먼저 확인. 이번 프로브에서 이미 이 Mac은 실패했으므로,
   harness가 배포 전 첫 게이트로 이 결과를 `unavailable`로 정직하게 표시해야
   Phase 1 "CuMetal 미설치/불일치 환경을 unavailable로 명확히 표시" 기준을
   만족한다.
2. **CuMetal 탐지** — `resolve_cli_path("cumetal")` / `resolve_cli_path("cumetalc")`.
   실패 시 optional-dependency 정책대로 기존 MLX/MPS/Metal 기능에 영향 없이
   `unavailable` 상태만 기록하고 조기 반환.
3. **`cumetal doctor` 정규화** — `external_command("cumetal")` + `["doctor"]`
   실행, stdout/stderr를 파싱해 version/toolchain 필드를 이슈의 evidence
   `runtime.version`/`install_source`에 매핑. 출력 포맷은 업스트림 문서에
   샘플이 없어 실제 설치 후 stdout을 보지 않고는 파서를 확정할 수 없다 —
   [unverified].
4. **워크로드 3종 소스** — `vectorAdd.cu`(elementwise), `saxpy.cu`(BLAS-1),
   `reduction.cu`(shared-mem + sync)를 리포지토리에 fixture로 추가하고
   `source_digest`(sha256)를 evidence에 기록.
5. **compile/run 분리** — `cumetalc <src>.cu -o <bin>` (compile 단계, timeout
   별도 측정) → `<bin>` 실행 (execution 단계, 별도 timeout). 두 실패를
   Phase 1 acceptance대로 "별도 상태"로 기록(컴파일 실패 ≠ 실행 실패).
6. **GPU provenance** — 실행 전/직후 `ioreg -c IOAccelerator` 스냅샷 diff
   (metrics.rs 기존 패턴 재사용)와, 가능하다면 `cumetal doctor`가 보고하는
   device identity를 함께 기록. 둘 다 없으면 provenance를 `unverified`로
   남기고 "GPU 실행 성공"을 주장하지 않는다.
7. **CPU reference 비교** — 동일 워크로드를 순수 C/Rust CPU 구현으로 재실행,
   dtype별 tolerance로 `max_abs_error`/`max_rel_error` 계산.
8. **manifest 저장** — `sw_vers`/`uname -m`/`xcrun --version`/`cumetal doctor`
   출력 전체를 offline 재실행용 manifest로 evidence와 함께 저장(이슈 Phase 1
   "offline 재실행을 위한 tool/runtime version manifest" 항목).

이 harness가 붙을 위치는 `runtime_adapter.rs`가 아니라 **새 모듈**
(예: `src-tauri/src/services/compute_backend/` 또는 `cumetal.rs`) — 기존
`LocalInferenceRuntimeAdapter`와 결합하면 "추론 서빙"과 "실험적 CUDA
호환성 측정"이라는 서로 다른 책임이 한 trait에 섞인다.

## 5. Phase별 acceptance 매핑

### Phase 1 — Host Probe / Installation / Minimal CUDA Execution

| Acceptance | 상태 | 근거 |
|---|---|---|
| M4/Mac mini에서 설치/탐지/doctor 결과 재현 가능 | **blocked** | CuMetal 미설치. Xcode Metal 컴파일러도 없음(§2 측정) — 설치 자체가 이 조사의 read-only 스코프 밖. next-step: `brew tap lulzx/tap && brew install lulzx/tap/cumetal` (설치 승인 필요) |
| `.cu` smoke workload 3종 자동 compile/run | **done on the JIT path (D51, 2026-11-06)** | `run_cumetal_smoke`(`services/cumetal/harness.rs`)가 `scripts/cumetal/smoke/*.cu` 3종을 clang 등록/JIT 경로로 compile -> link -> run하고 CPU 참조 parity와 `CUMETAL_PROVENANCE`를 판정. 이 Mac live 실행에서 3종 모두 `device=apple_gpu`로 통과. `cumetalc` AOT 경로는 Xcode Metal 툴체인이 없어 **여전히 blocked** |
| 미설치/불일치 환경을 `unavailable`로 명확 표시 | **done (D50, 2026-11-02)** | `get_cumetal_status`(`services/cumetal.rs`)가 metal/metallib/cumetal/cumetalc/doctor 프로브를 수행하고 첫 실패 사유와 함께 `unavailable`을 반환. 단위 테스트 14건(`cargo test cumetal`) 통과 — 모든 분기는 주입된 러너로, 실제 프로세스 일관성은 live 테스트로 확인. 이 Mac의 앱 수준 확인은 별도 |
| compile 성공/execution 성공을 별도 상태로 기록 | **done (D51, JIT path)** | compile/link/run/parity/provenance를 각각 별도 상태로 기록하고 단계별 타임아웃 적용. AOT 경로의 별도 상태는 Xcode 이후 |
| Mac model/SoC/GPU cores/RAM/macOS/Xcode/CuMetal version 연결 | **partial (D51 manifest)** | D51 manifest가 cumetal/clang++/macOS 버전, arch, Metal 툴체인 유무, 소스 sha256, 타임스탬프를 기록(offline 재실행용 버전 manifest 항목은 JIT 경로에서 충족). SoC/GPU 코어/RAM/Xcode 버전 연결은 **next-step**: `metrics.rs`의 `system_profiler`/`sysctl` 수집 경로 재사용 가능(§3), Xcode 버전은 `xcodebuild -version` 또는 `xcrun --version`으로 추가 |

### Phase 2 — CUDA API/Library Compatibility Matrix + Numerical Validation

전부 **blocked** — Phase 1 harness가 없으므로 API별 probe(`__global__`,
grid/block, shared memory, atomics, stream/event/memcpy, cuBLAS/cuFFT)를
얹을 기반이 없다. 업스트림 README가 "library shims are tested subsets;
NVIDIA numerical parity not claimed"라고 명시하므로, cuBLAS/cuFFT probe를
만들더라도 상위 커버리지 여부는 CuMetal 저장소의 개별 테스트 목록을 열람해야
확정된다 [unverified].

### Phase 3 — Real Workloads / Performance / PyTorch Feasibility Gate

전부 **blocked**, PyTorch Gate는 특히 그렇다. 업스트림에서 PyTorch CUDA
바인딩에 대한 언급을 이번 조사에서 찾지 못했다 — llm.c/llama.cpp CUDA
backend가 CuMetal 위에서 동작한다는 업스트림 주장도 찾지 못했다. 이슈
자체가 "PyTorch CUDA를 초기 지원 목표로 선언하지 않는다"고 명시하므로,
Phase 3 착수 전 별도 조사(업스트림 이슈 트래커, 커뮤니티 사례)가 선행
next-step이다.

### Phase 4 — Integration / UI / Regression / Evidence Catalog

전부 **blocked**. 단, 기반 설계는 이미 문서 수준에 존재한다: `docs/04-architecture.md`
§1.1의 `ComputeBackend` 4-way 모델이 `host-cumetal`을 Experimental/#84로
자리매김해 두었다. next-step은 그 문서 계약을 Rust trait/enum으로 옮기는 것
— 단 `grep -rl "ComputeBackend" src-tauri/src`가 0건이므로 #24(policy-aware
routing)가 이 trait의 소비자가 될 텐데, #24 자체도 이번 조사에서 코드
존재를 확인하지 않았다(스코프 밖).

## 6. optional-dependency / MLX 무영향 보장

- 이슈와 AGENTS.md 둘 다 "K8s never runs compute", "MLX/Metal work is host
  processes"를 하드 invariant로 못박는다. CuMetal은 `host-cumetal`로 이미
  이 경계 **안쪽**(macOS host)에 있고 K8s Pod에서 실행되지 않는다 — 이 점은
  설계 문서(§3) 기준으로 일관적이다.
- 무영향 보장의 실질적 메커니즘은 `runtime_adapter.rs`의 기존 패턴과 동일해야
  한다: `all_runtime_adapters()`(117-119행)가 CuMetal을 목록에 넣지 않는 한
  기존 두 어댑터(oMLX/mlx-lm)의 동작에는 코드 경로상 영향이 없다. **CuMetal
  모듈을 별도 파일/모듈로 분리하고 `Cargo.toml`에 별도 feature flag를 두지
  않는 이상**, 컴파일 시 CuMetal 관련 코드가 항상 바이너리에 포함된다는 점은
  주의할 부분이다 — "optional dependency"가 "런타임에 미설치 시 무해"를
  뜻하는지 "빌드 시 제외 가능"까지 뜻하는지 이슈 문구만으로는 불명확
  [unverified], §7에서 결정 필요 항목으로 남긴다.

## 7. 라이선스 호환성

- CuMetal 라이선스: **Apache-2.0** [upstream-doc, https://github.com/Lulzx/cuda-metal/blob/main/LICENSE,
  접근 2026-09-28 — `gh api repos/Lulzx/cuda-metal/license` 응답: `license.spdx_id: "Apache-2.0"`,
  `license.name: "Apache License 2.0"`, `path: "LICENSE"`].
- `scripts/release/check_licenses.sh`의 `ALLOWED` 집합(같은 파일 내 `ALLOWED = {...}`
  블록)에 `"Apache-2.0"`이 포함돼 있다 — **호환**.
- 다만 이 게이트는 스코프가 "바이너리에 컴파일되는 것"(`cargo metadata` +
  `pnpm licenses list --prod`)으로 명시적으로 한정된다(스크립트 상단 주석).
  CuMetal은 `cumetal`/`cumetalc` **외부 CLI**로 호출될 설계(§3, §4)이므로
  이 게이트를 애초에 통과할 필요가 없는 카테고리 — Colima/K3s와 같은 취급이다.
- 올바른 처리 경로는 `check_licenses.sh` 통과가 아니라 **`NOTICE` 파일에
  Colima/K3s 항목과 같은 형식으로 섹션 추가**: Source URL, License
  (Apache-2.0), Role in KubeMetal("host-cumetal 실험적 CUDA 호환성 백엔드,
  외부 CLI로 호출, 바이너리에 번들되지 않음"). 이 문서에서는 NOTICE를
  수정하지 않았다(이 작업의 스코프는 신규 `docs/20-*` 파일 한 개로 제한됨).

## Owner decisions needed

1. `kubemetal.cumetal.v1` evidence 스키마(이슈 본문)를 `research/README.md`가
   따르는 OpenForge Research Evidence Standard와 어떻게 합칠지 — 별도 스키마로
   병존시킬지, 표준 스키마의 `metadata` 필드 하위에 얹을지.
2. CuMetal 통합을 `LocalInferenceRuntimeAdapter`와 완전히 분리된 새 모듈
   (`ComputeBackend` trait)로 시작할지, 아니면 이 trait 자체를 이번 기회에
   먼저 `ComputeBackend`로 일반화(리팩터)할지 — 후자는 #24 라우팅 설계와
   순서 의존성이 생긴다.
3. "optional dependency" 요구사항이 런타임 무해(현재 패턴으로 충분)를
   뜻하는지, 빌드 시점 feature-flag 제외까지 요구하는지.
4. CuMetal 설치(`brew tap lulzx/tap && brew install lulzx/tap/cumetal`)를
   실제로 이 Mac에 수행할 권한 — 이번 조사는 read-only 제약으로 설치하지
   않았다.
5. NOTICE 파일에 CuMetal 섹션을 언제(Phase 1 착수 시 vs 실제 코드 병합 시)
   추가할지.

## Recommended next slice

**Phase 1의 toolchain preflight + CuMetal 탐지 두 단계만** 먼저 구현한다:
`src-tauri/src/services/compute_backend/cumetal.rs` 신설, `external_command`로
`xcrun -f metal`과 `resolve_cli_path("cumetal")`을 호출해 `Available /
Unavailable(reason)` 두 상태만 반환하는 순수 조회 함수 하나. 실행/컴파일/
evidence 스키마는 이 한 조각이 실제로 이 Mac에서 `Unavailable("xcrun -f
metal failed")`를 정직하게 반환하는 것을 테스트로 확인한 뒤 다음 슬라이스로
넘긴다 — Xcode 없는 이 개발 호스트에서는 그 결과 자체가 유효한 Phase 1
acceptance 증거("미설치/불일치 환경을 unavailable로 명확히 표시")가 된다.

## 2026-11-02 업데이트 — Phase 1 탐지 슬라이스 (D50)

- 업스트림 최신 릴리스 `v0.7.0` (published_at 2026-10-28T14:07:02Z)
  [upstream-doc, `gh api repos/Lulzx/cuda-metal/releases/latest`, 측정 2026-11-02].
  이 문서 §1의 `v0.6.0`(2026-09-28 기준)은 더 이상 최신이 아니다.
- Homebrew tap formula `Lulzx/homebrew-tap` `Formula/cumetal.rb` [upstream-doc, 측정 2026-11-02]:
  - `url`은 **v0.4.0**에 고정돼 있다 — 최신 릴리스(v0.7.0)와 어긋난다. 따라서 `brew install`로
    설치되는 버전은 업스트림 최신이 아니다(`cumetal version` 결과로 확인해야 하며 UI는 보고된 값만 표시).
  - `depends_on`: llvm, cmake, arm64, macOS Sonoma 이상.
  - `xcrun --find metal`과 `xcrun --find metallib` 중 하나라도 실패하면 `odie`한다 —
    D50 preflight가 같은 두 조회를 그대로 사용하는 이유.
  - `-DCUMETAL_ENABLE_BINARY_SHIM=OFF`로 빌드한다.
  - formula test는 `CUMETAL_TRACE_GPU=1`로 실행해 출력에 `device=apple_gpu`와
    `launch_success=true`를 기대한다 — 향후 "실제로 Apple GPU에서 실행됐는가"를 판정할
    GPU provenance 신호로 쓸 수 있다(이번 슬라이스는 실행하지 않으므로 미사용).
- 이 호스트: `xcrun -f metal`이 여전히 실패한다(Command Line Tools만 설치,
  `/Library/Developer/CommandLineTools`) [measured-local, 2026-11-02]. 따라서 이 Mac에서
  `get_cumetal_status`의 기대 결과는 `unavailable`(`xcrun --find metal failed ...`)이다.
- 구현: `src-tauri/src/services/cumetal.rs`, 커맨드 `get_cumetal_status`, 카드
  `src/components/mlx/CumetalCard.tsx`. 결정 근거는 D50(`docs/03-mvp-design.md` §4).
  모듈 위치는 §4 스케치의 `compute_backend/cumetal.rs`가 아니라 `services/cumetal.rs`다 —
  `ComputeBackend` trait이 아직 없어 디렉터리를 먼저 만들지 않았다.
- Phase 1 표 갱신: "미설치/불일치 환경을 `unavailable`로 명확 표시"만 done.
  나머지 행(설치/doctor 재현, `.cu` smoke 3종, compile/execution 별도 상태, 환경 연결 기록)은
  그대로 blocked/next-step이다. Owner decision 3(optional dependency)은 "런타임 무해, feature flag
  없음"으로 D50이 잠정 결정했다.

## 2026-11-06 업데이트 — 실제 설치·실행 (measured-local)

**결론: Metal 툴체인(Xcode) 없이도 clang CUDA → PTX 등록 → 런타임 MSL JIT 경로로 smoke 3종이
Apple GPU에서 실행되고 CPU 참조와 정확히 일치한다. 반면 `cumetalc` AOT 경로와 `cumetal doctor`는
Metal 툴체인 없이는 실패한다.**

- 설치 [measured-local, 2026-11-06]: tap 공식(이제 v0.7.1 핀)은 `xcrun --find metal`이 없으면 odie하므로
  사용하지 않고, `v0.7.1`(548e79a) 소스를 공식과 같은 옵션(`Release`, `-DCUMETAL_BUILD_TESTS=OFF`,
  `-DCUMETAL_ENABLE_BINARY_SHIM=OFF`, Homebrew LLVM 23.1.4)으로 빌드했다. prefix `~/.local/opt/cumetal`,
  `cumetal`/`cumetalc`만 `/opt/homebrew/bin`에 심볼릭 링크(`resolve_cli_path` 탐색 경로). 업스트림
  `docs/build.md`가 "Metal 도구 없이도 host 빌드와 runtime Metal compilation은 가능"이라고 명시한다.
  제거: 두 링크와 `~/.local/opt/cumetal` 삭제(`install_manifest.txt` 동봉).
- `cumetal version` → `cumetal 0.7.1`. `cumetal doctor` → exit 1, `[✗] Metal toolchain (metal/metallib not
  found)`, 나머지(Apple Silicon, macOS 27.0.1, CuMetal, CUDA-capable Clang) ✓.
- D50 프로브 실측: `state=unavailable`(사유: `xcrun --find metal` 실패), `cumetal_path`/`cumetalc_path`=
  `/opt/homebrew/bin/…`, `version=0.7.1`, `doctor.success=false, exit_code=1` — doctor 판정과 일치.
- `cumetalc <w>.cu -o <w>`(기본·`--mode experimental` 모두) → `failed to compile .metal to .air with xcrun
  metal`. AOT 경로는 Metal 툴체인 필수.
- 등록(JIT) 경로: 업스트림 `tests/cuda_projects/softmax/build.sh`와 같은 방식 —
  `clang++ -x cuda --cuda-gpu-arch=sm_80 -nocudainc -nocudalib -include cuda_runtime.h` (PATH 앞에
  `scripts/cuda_toolchain`) → `xcrun clang++ … -lcumetal -Wl,-rpath,<prefix>/lib`. 첫 실행 시 PTX를 MSL로
  낮춰 `newLibraryWithSource`로 컴파일한다(`CUMETAL_DEBUG_REGISTRATION=1`: `metallib='' ptx_size=1168`).

| workload (n=2^20) | source sha256 (앞 12자) | 결과 | GPU provenance (`CUMETAL_TRACE_GPU=1`) |
|---|---|---|---|
| vectorAdd | `9d220ac488df` | `max_abs_error=0` | `device=apple_gpu device_name="Apple M4 Pro" launch_success=true semantic_quality=exact` |
| SAXPY | `1857a9f1d794` | `max_rel_error=0` | `device=apple_gpu launch_success=true` |
| reduction (shared mem + `__syncthreads` + 64-bit `atomicAdd`) | `bbc671a3324d` | `got=36699887 expected=36699887` | `device=apple_gpu launch_success=true` |

  각 프로세스 wall time 0.05–0.07s(첫 실행 JIT 포함, 벤치마크 아님). provenance는 CuMetal 런타임의
  자기 보고다 — 독립 교차 확인(ioreg 활동 diff)은 아직 하지 않았다.
- **D50 판정과의 간극**: 지금 D50은 doctor를 따라 `unavailable`을 보이지만, 위처럼 JIT 경로로는 CUDA
  실행이 된다. "AOT 불가 / JIT 가능"을 구분하는 상태를 둘지는 오너 결정(Phase 1 harness 설계와 함께).

## 2026-11-06 구현 — Phase 1 smoke harness (D51, JIT 경로)

- 구현: `src-tauri/src/services/cumetal/harness.rs`, 커맨드 `run_cumetal_smoke`, 카드
  `src/components/mlx/CumetalSmokeCard.tsx`, fixture `scripts/cumetal/smoke/*.cu`(위 측정과 동일한 소스,
  sha256 앞 12자 `9d220ac488df`/`1857a9f1d794`/`bbc671a3324d`). 결정 근거는 D51(`docs/03-mvp-design.md` §4).
- §4 스케치와의 차이: AOT `cumetalc` 대신 JIT 경로를 쓰고(Metal 툴체인 불필요), GPU provenance는
  `ioreg` diff가 아니라 CuMetal의 `CUMETAL_PROVENANCE` 자기 보고를 쓴다 — **독립 교차 확인은 아직 없다**.
  CPU 참조 비교는 각 `.cu`가 스스로 수행해 종료 코드(0/1)와 metric 줄로 보고한다(별도 Rust 참조 구현 없음).
- 이 Mac live 실행(`cargo test live_smoke -- --nocapture`): 3종 모두 compile/link/run/parity 통과,
  provenance `device=apple_gpu device_name="Apple M4 Pro" launch_success=true semantic_quality=exact`.
- D50 `state`는 doctor 기준 그대로(`unavailable`)다. 카드가 "AOT 불가 / JIT 검증 가능" 안내를 덧붙인다.
- 여전히 blocked: `cumetalc` AOT 경로(Xcode Metal 툴체인 필요). 앱 UI 수준 관찰은 별도.

## #84 독립 ioreg 관측 추가 (D51 판정 유지)

- 위 11월 6일 결과는 당시 자기 보고 증거다. 현재 하네스는 D2와 공용인
  `services/gpu_observation.rs` 수집기·파서로 각 workload **실행 직전/실행 중/직후**를
  관측하고 기존 JSON의 `workloads[].gpu_observation`에 별도로 기록한다. compile/link 실패로
  실행하지 않으면 이 필드는 `null`이다. 신규 IPC나 샘플 기반 자동 승인은 없다.
- 각 ioreg는 1초 timeout, 실행 중에는 별도 Tokio task로 100ms 간격·최대 16회(전후 포함 최대 18회), 총 2초 상한이다.
  실행이 끝나면 샘플러를 즉시 abort해 진행 중인 probe와 kill_on_drop 자식을 정리하고 직후 샘플을 수집한다.
  실패/timeout에도 관측이 백그라운드에 남지 않고 기존 예약 guard가 해제된다.
- 샘플은 phase, 시작 Unix ms(시계 오류 시 null), 소요 ms, 사용률/메모리의 선택값,
  최대 4 KiB `raw_excerpt`(PerformanceStatistics 줄이 있으면 그 줄들, 없으면 stdout)와
  별도 `unavailable_reason`을 기록한다. 비정상 종료에도 stdout 발췌를 보존하고 오류는
  `unavailable_reason`에만 둔다. timeout은 자식을 종료하며 현재 runner에는 캡처된 출력이
  없어 `raw_excerpt`가 빈 문자열이고 사유에 이를 명시한다. spawn 실패도 발췌는 비어 있다.
  필드 누락·실행 실패·timeout은 0이 아니다.
- `activity_increase`는 실행 중 사용률이 직전보다 증가한 **시스템 전체 상관 증거**일 뿐,
  해당 프로세스의 GPU 사용 증명이 아니다. 다른 프로세스가 원인일 수 있고, 이미 바쁜 baseline에서는 증가 감지가 약하다. 판정에 사용하지 않는다. 직후의 증가나
  메모리 할당량 증가는 활동 증가로 판정하지 않는다. 짧은 커널 미포착·0·감소·필드 부재는
  `inconclusive`이며 CPU 실행으로 단정하지 않는다. 기존 CuMetal provenance/verdict는 독립이다.
- 이 변경의 GPU live 실행은 수행하지 않았다. 오케스트레이터가 idle Mac에서
  `cargo test --locked --manifest-path src-tauri/Cargo.toml live_smoke -- --nocapture`를 실행해
  실제 JSON 또는 명시적 `SKIPPED`를 확인해야 한다. 활동 미포착도 유효한 관측 결과다.

## 2026-11-08 업데이트 — Xcode 설치 후 AOT 경로 검증 (measured-local)

- Xcode 27.2(27B52) 설치. 이 버전은 Metal 툴체인을 별도 구성요소로 배포한다 — Xcode만 깔면 `xcrun --find metal`이
  경로를 돌려주지만 그것은 스텁이고(`cannot execute tool 'metal' due to missing Metal Toolchain`), `metallib`은 없다.
  필요한 순서(모두 실측): `sudo xcode-select -s /Applications/Xcode.app/Contents/Developer` →
  `sudo xcodebuild -license accept` → `sudo xcodebuild -runFirstLaunch`(없으면 CoreSimulator 플러그인 로드 실패로
  `-downloadComponent`도 실패) → `xcodebuild -downloadComponent MetalToolchain`(838.9 MB, sudo 불필요,
  Status: installed, 27B47). 툴체인은 cryptex 마운트(`/var/run/com.apple.security.cryptexd/mnt/…/Metal.xctoolchain`)에서
  실행된다. `metal --version` → `Apple metal version 32023.922`.
- `cumetal doctor` → exit 0, "No issues found!".
- `cumetalc <w>.cu -o <w>`(AOT) 3종 모두 compile exit 0, run exit 0, CPU 참조 일치(vectorAdd `max_abs_error=0`,
  SAXPY `max_rel_error=0`, reduction `got=expected=36699887`), provenance `device=apple_gpu launch_success=true
  semantic_quality=exact source=generic_nvvm`(JIT 경로는 `generic_ptx` — 두 경로가 실제로 다른 lowering을 탄다).
- D50 프로브 live: `state=available`, doctor success. D51 harness live: `all_passed=true`,
  `metal_toolchain_present=true`. 앱(dev 빌드)에서 D50 카드 "사용 가능", D51 smoke 3종 "통과" 렌더 확인.

## 2026-11-08 업데이트 — NVML shim 실측 (NVIDIA 지표 열 채울 수 있는가)

결론: **채울 수 없다.** CuMetal 0.7.1의 `lib/libnvidia-ml.dylib`는 dlopen으로 직접 호출해 유휴/부하(`saxpy` 57회
반복) 두 시점에서 측정했다. 원문 로그와 프로브 소스는 `.omc/logs/cumetal-nvml-2026-11-08.md`(로컬) 참조.

| 지표 | shim 반환 (measured) | 구현 (업스트림 `runtime/rt/nvml.cpp`, read) | NVIDIA 열에 사용 |
|---|---|---|---|
| GPU 사용률 | `NVML_ERROR_NOT_SUPPORTED`(rc=3), 유휴·부하 동일 | L164-169 무조건 NOT_SUPPORTED | 불가 |
| 온도 | rc=3 | L171-178 | 불가 |
| 전력(W) | rc=3 | L180-184 | 불가 |
| 클럭 | rc=3 (세 종류 모두) | L186-192 | 불가 |
| GPU 메모리 | rc=0, total=68719476736 | L142-162: total=`hw.memsize`, free=호스트 VM free+inactive 페이지 | **불가 — 머신 전체 RAM이지 GPU 할당이 아니다.** 같은 순간 `ioreg` "In use system memory"는 0.5–0.8 GB |
| 발열 압력 | API 없음 | — | 불가 |

- 장치 이름은 CPU 브랜드 문자열("Apple Silicon (Apple M4 Pro)"), UUID는 하드코딩된 합성 값 — 측정값이 아니다.
- `nvidia-smi` 류 도구는 동봉되지 않는다. `nvmlDeviceGetCudaComputeCapability`는 export되지 않는다.
- 따라서 NVML 기반 도구를 이 Mac에 붙이면 "GPU 메모리 사용량"이 호스트 RAM 사용량으로 보고된다 — KubeMetal은
  이 값을 GPU 지표로 쓰지 않는다(D22). GPU 지표의 출처는 계속 D2 개정의 `ioreg`다.

## 2026-11-08 업데이트 — Phase 2 호환성 매트릭스 (measured-local)

`scripts/cumetal/matrix/run_matrix.sh <out.json>`: 프로브 14종을 AOT(`cumetalc`)와 JIT(clang CUDA) **두 경로로 각각**
빌드해 `CUMETAL_TRACE_GPU=1`로 3회씩 실행하고, 이슈의 어휘(supported/partial/unavailable/failed/unverified)로
fail-closed 분류한다 — 컴파일 성공만으로는 supported가 되지 않고, 커널을 띄운 프로브는 provenance가
`device=apple_gpu launch_success=true`여야 한다. 결과 JSON(매니페스트·프로브 sha256 포함)은
`.omc/logs/cumetal-matrix-2026-11-08.json`(로컬, 약 75초). CuMetal 0.7.1, Apple metal 32023.922, M4 Pro.

| 프로브 | AOT | JIT | 비고 |
|---|---|---|---|
| launch 1D/2D/3D, shared+`__syncthreads`, atomics(int add·CAS·float add), warp shuffle, stream+event+`cudaMemcpyAsync`, managed memory, 수기 SGEMM(512², 오차 5.8e-9), NaN/Inf 검출 | supported | supported | 3회 비트 동일, 모든 launch `semantic_quality=exact` |
| FP64 | partial | partial | 오차 9.1e-15(허용 1e-9) 통과, 그러나 CuMetal이 `semantic_emulation`(AOT)·`reduced_precision_fp64`(JIT)로 보고 — 네이티브 FP64 아님 |
| cuBLAS `cublasSgemm` | partial | partial | 수치 통과, provenance 줄 없음 → GPU 실행 미증명 |
| cuFFT 1D C2C 왕복 | partial | partial | 수치 통과, provenance 없음. **AOT는 3회 중 순방향 출력 비트가 2종**(허용 오차 내, 두 번의 전체 실행에서 재현) — 비결정적 |
| cuRAND host API | partial | partial | 분포 검사 통과, provenance 없음 |

- 이번 실행에서 unavailable/failed는 0건 — 즉 분류기의 거부·실패 분기는 실측으로 아직 한 번도 타지 않았다.
- 미검증: cuSPARSE, cuSOLVER, cuDNN, NCCL, NVRTC, device-side cuRAND, FP16, 다른 하드웨어.

## 2026-11-09 수정 — Phase 2 회차별 provenance 판정 (#84)

분류기는 `scripts/cumetal/matrix/classify.py`로 분리했다. 커널·라이브러리 프로브 모두
**매 회차** parity PASS, exit 0 및 `kernel_launch` 증거가 있어야 `supported`가 된다.
각 회차의 모든 launch가 `device=apple_gpu`, `launch_success=true`,
`semantic_quality=exact`, source 존재, unsupported_reason 없음 조건을 만족해야 한다.
한 회차만 증거가 있거나 CPU·실패 launch·불완전 provenance가 섞이면 `partial`이다.
실행 timeout·수치 불일치·누락된 실행 결과는 `failed`이며, 명시적 unsupported API
실패는 기존대로 `unavailable`이다. 허용 오차 내 digest 차이는 판정을 강등하지 않는다.

JSON의 `runs[].provenance`는 회차별 증거, 경로의 `provenance`는 전체 회차 집계다.
집계는 판정에 사용하지 않는다. runner manifest는 분류기 sha256도 포함한다.
`python3 -B -m unittest discover -s tests/cumetal -v`가 합성 로그 회귀를 실행하며
`make test`/`make verify`에 포함된다. 위 11월 8일 실측 표는 과거 결과이며 이 수정으로
GPU 실행을 새로 검증한 것은 아니다. CuMetal 자기 보고의 독립 교차 확인은 여전히 미검증이다.

## 2026-11-12 추가 — cuSPARSE CSR SpMV 프로브 (#84, runtime unverified)

`cusparse_spmv.cu`를 기본 매트릭스의 15번째 프로브로 등록했다. 측정 범위는
FP32, 32-bit/0-based CSR, non-transpose `cusparseSpMV`의 기본 알고리즘 하나다.
4×5 행렬(7개 nonzero, 빈 행 포함)에 `y = 1.25 A x - 0.5 y`를 적용하고 모든
출력을 double CPU reference와 비교한다. 각 원소는 finite여야 하며
`abs(error) <= 1e-5 + 1e-5 * abs(reference)`를 만족해야 한다.
결과에는 finite 여부·최대 절대 오차·허용 오차·출력 FNV digest를 남기며,
CUDA/cuSPARSE 실패는 호출 위치·반환 코드와 함께 비정상 종료한다.
runner는 기존대로 AOT/JIT 각각 3회의 종료 코드·출력·provenance를 보존한다.
수치 PASS만으로 `supported`가 되지 않으며 회차별 GPU provenance 검사를 그대로 적용한다.
이 한 subset의 결과를 cuSPARSE 전체 지원으로 확대하지 않는다. cuFFT/cuRAND는 변경하지 않았다.

CPU fake API fault injection은 실제 프로브의 정상·허용 오차·mismatch·NaN/Inf·API 실패와
workspace 유무 분기를 검사한다. 합성 classifier fixture는 두 경로의 라이브러리 등록,
provenance 누락, API 실패, 수치 실패 및 회차별 digest 차이를 검사한다.
이는 실제 CuMetal 빌드·GPU 실행 증거가 아니다. 헤더·dylib 존재도 실행 지원 증거가 아니다.
위 11월 8일 실측 14종 표는 그대로 과거 결과다.

실측은 오케스트레이터가 학습·서빙 등 GPU 작업이 없는 유휴 상태를 확인한 뒤 실행한다.
독립 shell matrix는 D40 예약을 획득하지 않는다. 설치·업그레이드 없이 실패도 JSON/Markdown
증거로 보존한다. 이번 구현 레인에서는 새 SpMV matrix를 실행하지 않았다:

```bash
bash scripts/cumetal/matrix/run_matrix.sh <evidence-out.json> cusparse_spmv:cusparse
```

**실측 (measured-local, 2026-11-12, M4 Pro, CuMetal 0.7.1, 유휴 상태 확인 후 오케스트레이터 실행):**
위 명령을 실행한 결과 AOT·JIT 모두 컴파일 성공, 3회 모두 `status=PASS`
(`rows=4 cols=5 nnz=7 finite=true max_abs_err=0.000e+00`, digest `ff8d415ec1f5d36a` 동일)였다.
CuMetal은 이 라이브러리 호출에 `CUMETAL_PROVENANCE` 줄을 하나도 내지 않았으므로(`events={}`)
분류는 두 경로 모두 **`partial`** — "numeric parity only; GPU execution not independently proven".
cuBLAS/cuFFT/cuRAND와 같은 결론이며, 이 subset이 GPU에서 실행됐다는 증거는 없다.

## 2026-11-12 ioreg live 관측 (measured-local, orchestrator-run)

오케스트레이터가 이 Mac에서 실행한 결과: smoke workload 3종 모두 통과했지만
`gpu_observation.status=inconclusive`, 사유는 `No utilization increase captured`였다.
실행 전 `Device Utilization`은 다른 앱 부하로 이미 **49%**였다. 바쁜 baseline은
증가 감지를 약화시키므로 이 관측은 시스템 전체 상관 증거로만 남기며 D51 verdict에는
사용하지 않는다. 이번 수정 작업에서는 live smoke를 재실행하지 않았다.

동시 sampler 적용 후 두 번째 오케스트레이터 실행(measured-local, 2026-11-12, 사용자 제공):
3개 workload 모두 통과했고 `gpu_observation`은 1개에서 `activity_increase`, 2개에서
`inconclusive`였다. live test wall time은 4.5–6.1초였다. 관측은 실행마다 달라지며
시스템 전체 상관 증거로만 남는다. 이 작업자는 live test를 재실행하지 않았다.

## #84 기존 수치 프로브 finite 방어 (runtime unverified)

수기/cuBLAS SGEMM, FP64, cuFFT 순방향·역방향은 `parity.h`로 전체 출력의
NaN/±Inf를 거부한다. SGEMM CPU reference 샘플 간격, strict tolerance
(`<1e-5`), FP64 fast48 정책(`<1e-9`), FFT(`<1e-3`)은 유지한다.
결과의 `finite`, `failed_index`(0-based, FFT는 complex 원소), `max_error`는
실패 위치와 검사 결과를 기록한다. 0/비유한 분모는 정의되지 않은 비율로 실패한다.
`nan_inf.cu`와 cuSPARSE는 변경하지 않았다. CPU helper 주입 테스트는 실제
CuMetal corruption 재현이 아니다. 기존 `fmax(0, NaN)` CPU 재현은
`legacy_nan max_error=0 verdict=PASS`(회귀 검사 exit 1)였다.
finite scan을 임시 제거한 음성 대조에서도 실제 helper 테스트가 SIGABRT로 실패했고
(`Ran 2 tests ... FAILED (errors=1)`), 복원 후 전체 15개가 통과했다.

유휴 Mac에서 오케스트레이터가 실행할 정상 회귀 명령(D40 예약 없음, 학습/서빙과 병행 금지):

```bash
bash scripts/cumetal/matrix/run_matrix.sh /tmp/cumetal-parity.json sgemm_handwritten cublas_sgemm:cublas fp64 cufft_c2c:cufft
```

`make test`/`make verify`의 기존 `tests/cumetal` discovery가 host C++ helper와
MISMATCH classifier 전파 테스트를 실행한다. 이번 레인은 GPU 실행을 하지 않았다.

**실측 (measured-local, 2026-11-12, M4 Pro, CuMetal 0.7.1, 오케스트레이터 실행):** NaN/Inf 방어를 넣은 뒤
`run_matrix.sh` 전체 15종을 다시 돌렸다. 4개 수치 프로브(SGEMM·cuBLAS SGEMM·FP64·cuFFT)는 모든 회차에서
`finite=true failed_index=-1`로 통과했고 분류는 이전과 같다(경로별 supported 20 / partial 10).
즉 이 Mac에서는 실제 NaN이 숨어 있지 않았다 — 고친 것은 "NaN이 나와도 PASS가 될 수 있던" 판정 결함이다.

**설치 주의 (measured-local, 2026-11-12):** `cumetalc`는 FP64 지원 셰이더
(`compiler/metal/support/cumetal_fp64_inline_support.metal`)를 **빌드에 쓴 소스 트리의 절대 경로**에서 읽는다.
11-06에 `/tmp/cumetal-src`에서 빌드했더니 macOS의 `/tmp` 정리로 그 파일이 지워져, 같은 날 `fp64` AOT가
`compile refused: textual include input must be an existing .metal file`로 `unavailable`이 됐다(main에서도 재현,
코드 변경과 무관). 소스를 `~/.local/src/cuda-metal`(v0.7.1, 548e79a)에 두고 같은 옵션으로 재빌드·재설치해
해소했다 — 소스 트리는 설치 후에도 지우면 안 된다.

## #84 SGEMM opt-in 시간 분리 (runtime unverified)

인자 없는 `sgemm_handwritten`은 기존 단일 dispatch matrix 프로브다.
`--benchmark --warmup W --iterations I`로만 성능 측정을 켠다(W=1..20, I=1..100).
같은 프로세스에서 **첫 dispatch 1회 → warm-up W회 → 측정 I회**를 실행한다.
매 dispatch는 오류 검사와 `cudaDeviceSynchronize`를 마친다. steady clock의 ns 구간은
launch·동기화를 포함하며 GPU-event 시간이 아니다. 첫 dispatch에는 JIT가 포함될 수 있다.
모든 iteration에서 복사·전체 finite 검사·기존 샘플 CPU reference를 타이머 종료 뒤 수행한다.
초기 입력 복사도 측정 밖이다. 반복 parity가 실행 간격·thermal에 영향을 줄 수 있으므로
이 수치를 GPU 포화 throughput으로 해석하지 않는다.

`benchmark/run_benchmark.py`는 AOT 1단계 또는 JIT compile/link 2단계의 외부 wall time을
각각 ns로 보존하고 합계를 별도 기록한다. 독립 프로세스 3회 compatibility 표본과 무관하다.
첫 dispatch, warm-up 제외 raw steady samples, sample count, median(짝수는 중앙 두 값 평균),
nearest-rank p95(`ceil(.95*n)`번째 값)를 기록하며 Python이 raw와 요약을 대조한다.
raw·요약·단위가 없거나 맞지 않으면 실패한다. timeout은 process group을 종료한다.
API·동기화·parity·GPU provenance·도구 버전 수집 실패도 성능 성공으로 보고하지 않는다.
매 dispatch의 exact apple_gpu provenance 개수가 예상 횟수와 같아야 한다.

Foundation `thermalState` 전용 Swift 도구는 미리 빌드하며 그 준비 시간은 compile/link 및
kernel 시간에서 제외한다. 측정 전후 nominal/fair/serious/critical 원문과 도구 결과를 보존한다.
준비·조회 실패는 `unavailable`과 사유로 기록한다. thermal 미수집만으로 수치 시간을 버리지는
않지만 thermal 조건을 검증했다고 주장할 수 없다. 도구 버전·소스/helper/classifier SHA256,
호스트·prefix·명령·실행 출력·종료 코드도 JSON에 남긴다. 새 IPC/D-registry 변경은 없다.

CPU 테스트는 실제 `.cu`의 host timing 루프에 fake clock/dispatch/validation을 주입해
첫 dispatch 분리, warm-up 제외, ns·분위수, 횟수 제한, 동기화/검증/시계 실패를 검사한다.
runner는 fake tools로 compile/link·timeout·parity·provenance·thermal 실패를 검사한다.
기존 `make test`/`make verify`의 `tests/cumetal` discovery가 모두 실행한다.
이번 레인은 benchmark CLI 또는 GPU workload를 실행하지 않았다. 유휴 Mac에서 오케스트레이터가
아래 두 명령을 실행하고 raw samples와 median/p95를 대조해야 한다. D40 예약이 없으므로
학습·서빙과 병행하지 않는다. PyTorch/MLX 비교·범용 accelerator schema는 범위 밖이다.

```bash
python3 -B scripts/cumetal/benchmark/run_benchmark.py --mode aot --warmup 2 --iterations 5 --output /tmp/cumetal-sgemm-aot.json
python3 -B scripts/cumetal/benchmark/run_benchmark.py --mode jit --warmup 2 --iterations 5 --output /tmp/cumetal-sgemm-jit.json
```

**실측 (measured-local, 2026-11-12, M4 Pro, CuMetal 0.7.1, thermal nominal, 오케스트레이터가 위 두 명령 실행):**
SGEMM 512², warm-up 2 + 측정 5회, 한 프로세스. 두 경로 모두 parity·provenance 통과(`supported`, kernel launch 8회 = 1+2+5).

| 경로 | compile+link wall | 첫 dispatch | steady-state median | p95 |
|---|---|---|---|---|
| AOT (`cumetalc`) | 7.25 s | 8.19 ms | 1.708 ms | 1.722 ms |
| JIT (clang CUDA) | 1.12 s | 55.33 ms | 1.744 ms | 1.800 ms |

읽는 법: AOT는 컴파일 비용을 빌드 때 내고 첫 실행이 빠르다. JIT는 빌드가 빠른 대신 첫 dispatch에 런타임 컴파일이 섞인다.
정상 구간은 두 경로가 사실상 같다. 표본 5개·단일 실행·다른 앱이 떠 있는 데스크톱이라 **경향만** 보여 주며,
네이티브 MLX/Metal 대비 비교나 재현성 주장은 아니다. 실패한 실행의 수치는 `timing`이 아니라 `timing_unverified`에만 남는다.

## #84 matrix 원시 증거 보존 및 오프라인 재분류

명시한 `<out.json>` 옆에 `<out.json>.evidence.<attempt>`를 생성한다. JSON의
`evidence.directory`는 결과 파일 기준 상대 경로이며 `evidence.files`는 각 파일의
상대 경로와 SHA256이다. 결과와 증거 디렉터리를 함께 전달한다. 전체 doctor 로그와
종료 코드, AOT/JIT 컴파일 진단·rc, 성공 stdout을 포함한 매 회차 출력·rc·원시
provenance, manifest, 모든 `*.h` 헤더·프로브 및 runner/classifier 소스와 해시를 보존한다.
회귀 테스트는 `.cu`의 `#include "..."`를 스캔해 복사·해시 목록의 헤더 누락을 거부한다.
실행 바이너리와 object는 별도 임시 디렉터리에만 생성하고 종료 시 삭제한다.
다운로드·앱 파일 접근·TTL·증거 자동 삭제는 없다. 수신자가 보관 용량을 관리한다.

실패한 컴파일·timeout·실행 실패도 정상적으로 수집되면 해당 실패 판정을 내보낸다.
반면 보존 실패·중단·중복 probe 등 부분 실행은 성공 메시지 없이 비정상 종료하며,
남은 증거는 `complete=false`로 표시한다. 기존 JSON/Markdown 목적지는 덮어쓰지 않는다.
재분류는 저장 목록과 모든 SHA256을 먼저 검사하며 변조·누락·추가 파일, symlink,
부분 실행, 반복 횟수 불일치를 거부한다. 재분류에는 `--result <원본.json>`이 필수이며,
원본 결과의 디렉터리·index 경로·파일별 SHA256·index SHA256과 대조한다. 증거 디렉터리나
index와 sources가 모두 사라져도 검증을 생략하지 않는다. index 없는 옛 fixture도 CLI에서 거부한다.
`--initial`은 runner가 새로 봉인한 증거의 최초 분류용이며 원본 결과에 대한 재검증이 아니다.
D44 범위: 이 결속은 결과 JSON에 대한 증거의 독립적/우발적 변경(로그 수정 후 index 재봉인 포함)을
탐지한다. 서명이 아니므로 결과 JSON과 증거를 함께 다시 쓰는 공격자는 범위 밖이다.

```bash
# GPU 유휴 상태에서 오케스트레이터만 실행 (이 구현 레인은 실행하지 않음)
bash scripts/cumetal/matrix/run_matrix.sh /tmp/matrix-84.json launch_1d
# 이후 CuMetal/clang/Metal 없이도 실행 가능. 실제 생성된 디렉터리 이름을 지정한다.
/usr/bin/python3 -I -B scripts/cumetal/matrix/classify.py /tmp/matrix-84.json.evidence.XXXXXX /tmp/reclassified-84.json 3 --result /tmp/matrix-84.json
```

`tests/cumetal/test_matrix_evidence.py`는 실제 shell/classifier에 CPU fake tool을
연결해 성공·컴파일 거부/timeout·실행 실패/timeout·보존 실패·목적지 충돌·부분 실행,
재분류 판정 동일성과 로그 변조 거부를 검사한다. 기존 `make test`의
`python3 -B -m unittest discover -s tests/cumetal -v`를 통해 `make verify`에도 포함된다.
이는 GPU 실측 증거가 아니다.

## #84 matrix 호스트 식별 manifest (smoke 범위 제외)

`host_manifest.py`는 고정된 읽기 전용 명령을 각각 최대 5초, `LC_ALL=C`와 고정 PATH로
shell 없이 실행한다. 자식 환경은 PATH/LC_ALL만 허용하며 환경 변수 덤프를 저장하지 않는다.
`doctor.log`는 CuMetal doctor의 OS/아키텍처·설치 구성요소 경로·도구 가용성 진단만 담는다.
doctor에도 HOME/PATH/LC_ALL만 전달한다. 홈 디렉터리 경로는 포함될 수 있다.
`manifest/host.json` 원문·종료 코드·argv·수집/파싱 오류를 원시 증거 해시 목록에 포함하고,
분류 결과의 `manifest.host`에 연결한다. 수집 필드는 다음과 같다.

| 필드 | 원본 |
|---|---|
| `mac_model` | `sysctl -n hw.model` (Mac model identifier) |
| `soc` | `sysctl -n machdep.cpu.brand_string` |
| `ram_bytes` | `sysctl -n hw.memsize` (양의 정수 bytes) |
| `gpu_cores` | `system_profiler SPDisplaysDataType -detailLevel mini -json`의 단일 GPU `sppci_cores` |
| `macos`, `product_version`, `build` | `sw_vers` 원문과 파싱한 ProductVersion/BuildVersion |
| `xcode_version` | `xcodebuild -version`의 Xcode 버전과 build |
| `cumetal_version`, `llvm_clang`, `xcrun_clang`, `metal_version` | 해당 tool의 전체 원시 `--version` 출력과 첫 줄 식별값 |

mini displays 조회만 사용하며 하드웨어 serial/UUID 조회는 하지 않는다. displays JSON의
serial/UUID 키는 중첩 위치에서도 제거하며, JSON 파싱 실패 시 원문 대신 명시적 오류를 기록한다.
누락·비정상 종료·timeout·빈 출력·잘못된 RAM/GPU 정수·복수 GPU는 해당 값을 `null`로 두고
`errors`에 이유를 기록한다. timeout은 종료 코드를 추정하지 않고 `returncode=null`로 두며
가능한 부분 stdout/stderr를 보존한다. 수집기가 JSON을 썼다는 것은 장비/toolchain 정상
판정이 아니다. CLT-only 환경의 clang 버전을 Xcode 버전으로 대체하지 않는다.
기존 `manifest.hw_model`은 호환을 위해 **SoC brand 별칭**으로 유지하며
`hw_model_meaning`으로 의미를 명시한다. 실제 model identifier는 `manifest.host.mac_model`이다.
기존 버전 별칭과 `sw_vers`도 유지한다. 옛 결과에 없는 하드웨어 값을 채워 넣지 않는다.

수집기만 실행할 때는 GPU workload가 없다 (기존 목적지는 거부한다):

```bash
/usr/bin/python3 -I -B scripts/cumetal/matrix/host_manifest.py /tmp/matrix-host-84.json \
  --cumetal /opt/homebrew/bin/cumetal --clang /opt/homebrew/opt/llvm/bin/clang++
```

**수집기 전용 실측 (measured-local, 2026-11-12)**: 이 Mac에서 `mac_model=Mac16,11`,
`soc=Apple M4 Pro`, `ram_bytes=68719476736`, `gpu_cores=20`,
`Xcode 27.2 / Build version 27B52`, macOS 27.0.1 / 26A434,
CuMetal 0.7.1, Homebrew clang 23.1.5, Apple clang 21.0.0,
Metal 32023.922를 수집했으며 `errors={}`였다. sysctl 3종·system_profiler·xcodebuild를
독립 재조회하여 원문과 종료 코드 일치를 확인했다. GPU 실행 증거는 아니다.
`tests/cumetal/test_host_manifest.py`의 형식·CLT-only·누락/timeout·malformed·Unicode·별칭
회귀는 기존 Python discover를 통해 `make test`/`make verify`에 포함된다.
이 변경은 **matrix만** 보강한다. D51 smoke manifest와 그 하드웨어 AC는 미완료로 남긴다.

#84 리뷰 수정의 검증은 CPU fixture/오프라인 재분류에 한정한다. 이 변경에서 GPU 실측을 추가하지 않았다.
