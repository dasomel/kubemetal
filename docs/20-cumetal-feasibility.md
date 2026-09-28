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
| `.cu` smoke workload 3종 자동 compile/run | **next-step** | 코드 미작성. next-step: §4의 5-8 단계 구현, `src-tauri/src/services/compute_backend/cumetal.rs` 신설 |
| 미설치/불일치 환경을 `unavailable`로 명확 표시 | **next-step** | `resolve_cli_path` 실패 시 조기 반환 패턴은 기존 코드에 선례 있음(`runtime_adapter.rs` install_hint 패턴 참고), CuMetal 전용 상태 enum만 추가하면 됨 |
| compile 성공/execution 성공을 별도 상태로 기록 | **next-step** | §4-5 설계 |
| Mac model/SoC/GPU cores/RAM/macOS/Xcode/CuMetal version 연결 | **next-step** | `metrics.rs`의 `system_profiler`/`sysctl` 수집 경로 재사용 가능(§3), Xcode 버전은 `xcodebuild -version` 또는 `xcrun --version`으로 추가 |

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
