# 03. MVP 설계 — 폴더 구조 및 핵심 코드

**Phase 1 MVP 구축을 위한 Tauri v2 + Rust 백엔드 및 React(TypeScript) 프론트엔드 통합 프로젝트의 폴더 구조와 핵심 코드 설계**입니다. macOS `.app` 번들 실행 환경, `colima` CLI의 출력 특성, wry(WebView) 제약을 반영해 `arch.md` 초안의 결함을 수정했습니다.

---

## 1. 프로젝트 전체 디렉터리 구조

```text
kubemetal/
├── src/                              # Frontend (React + TypeScript + Tailwind)
│   ├── assets/
│   ├── components/
│   │   ├── dashboard/                # 시스템 자원(RAM/CPU) 및 Colima 상태 뷰
│   │   ├── services/                 # MLflow / SeaweedFS 원클릭 프로비저닝 UI
│   │   └── common/                   # 공통 UI 컴포넌트
│   ├── hooks/                        # Tauri IPC 커스텀 훅 (useColima, useMetrics)
│   ├── lib/                          # 순수 로직 (VM 리소스 추천 등)
│   ├── types/                        # Rust 백엔드와 맞춘 TypeScript 타입 정의
│   ├── App.tsx
│   └── main.tsx
├── src-tauri/                        # Backend (Rust Native Control Agent)
│   ├── src/
│   │   ├── main.rs                   # Tauri 실행 엔트리포인트
│   │   ├── lib.rs                    # 핸들러 및 모듈 등록
│   │   ├── commands/                 # Frontend에서 호출하는 tauri::command 모음
│   │   │   ├── mod.rs
│   │   │   ├── colima.rs             # Colima/K3s 클러스터 상태·시작·정지
│   │   │   ├── metrics.rs            # macOS 시스템 자원(RAM/CPU) 측정
│   │   │   └── provision.rs          # MLflow/SeaweedFS/GPU 브리지 매니페스트 적용
│   │   └── services/                 # CLI 래퍼 및 시스템 인터페이스
│   │       ├── process.rs            # CLI 절대경로 탐색 + tokio 비동기 실행기
│   │       └── sysinfo.rs            # sysinfo State 초기화 헬퍼
│   ├── capabilities/
│   │   └── default.json              # Tauri v2 권한 설정 (dialog 포함)
│   ├── tauri.conf.json               # Tauri v2 앱 설정
│   └── Cargo.toml                    # Rust 의존성 설정
├── scripts/                          # 로컬 인프라 스크립트 및 매니페스트
│   └── k8s/
│       ├── mlflow-deployment.yaml    # K8s 배포용 MLflow 서버
│       ├── seaweedfs-deployment.yaml # K8s 배포용 SeaweedFS 스토리지
│       └── mac-gpu-bridge.yaml       # 호스트 MLX 서빙으로의 ExternalName 별칭
├── package.json
└── tsconfig.json
```

---

## 2. 백엔드 핵심 파일 설계 (Rust / Tauri v2)

### `src-tauri/Cargo.toml`

```toml
[package]
name = "kubemetal"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2.0.0", features = [] }

[dependencies]
tauri = { version = "2.0.0", features = [] }
tauri-plugin-dialog = "2.0.0"       # alert() 대체용 (D8)
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1", features = ["full"] }
sysinfo = "0.30"                     # macOS RAM/CPU 모니터링
```
> Tauri v2 표준 구조를 따라 `src/lib.rs`에 `run()`을 두고 `src/main.rs`는 `fn main() { kubemetal_lib::run(); }`만 호출하는 얇은 바이너리 엔트리포인트로 유지한다.

### `src-tauri/tauri.conf.json`

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "kubemetal",
  "identifier": "com.dasomel.kubemetal",
  "build": {
    "beforeDevCommand": "pnpm dev",
    "beforeBuildCommand": "pnpm build",
    "frontendDist": "../dist",
    "devUrl": "http://localhost:5173"
  },
  "app": {
    "windows": [{ "title": "KubeMetal", "width": 1200, "height": 800 }]
  },
  "bundle": {
    "active": true,
    "targets": ["app", "dmg"],
    "resources": ["scripts/k8s/*"]
  },
  "plugins": {}
}
```
> `bundle.resources`에 `scripts/k8s/*`를 등록해야 `provision_mlops_stack`의 `resource_dir()` 조회가 dev/번들 양쪽에서 매니페스트를 찾을 수 있다.

### `src-tauri/src/services/process.rs`

```rust
use std::path::PathBuf;

/// macOS .app 번들은 로그인 셸의 PATH를 상속하지 않는다.
/// Homebrew/시스템 설치 경로를 직접 탐색해 실행 파일의 절대경로를 찾는다. (D5)
const SEARCH_PATHS: [&str; 3] = [
    "/opt/homebrew/bin", // Apple Silicon Homebrew
    "/usr/local/bin",    // Intel Homebrew / 수동 설치
    "/usr/bin",
];

pub fn resolve_cli_path(bin: &str) -> Result<PathBuf, String> {
    for dir in SEARCH_PATHS {
        let candidate = PathBuf::from(dir).join(bin);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "'{bin}' 실행 파일을 찾을 수 없습니다. Homebrew로 설치되어 있는지 확인하세요."
    ))
}
```

### `src-tauri/src/commands/colima.rs`

```rust
use serde::{Deserialize, Serialize};
use crate::services::process::resolve_cli_path;

/// colima 0.10.x `status --json` 실측 스키마: 기동 중일 때만 exit 0 + stdout에
/// 평면 JSON({"kubernetes":true,...})을 출력하고, 미기동이면 exit 1 + stdout 없음.
/// "status" 필드는 존재하지 않는다 — 기동 여부는 파싱 성공 자체로 판별한다.
#[derive(Debug, Deserialize)]
struct ColimaStatusRaw {
    #[serde(default)]
    kubernetes: bool,
}

#[derive(Debug, Serialize)]
pub struct ClusterStatus {
    pub is_running: bool,
    pub kubernetes_active: bool,
    pub mlflow_ready: bool,
    pub seaweedfs_ready: bool,
}

#[tauri::command]
pub async fn get_cluster_status() -> Result<ClusterStatus, String> {
    let bin = resolve_cli_path("colima")?;
    // colima는 logrus 로그를 stderr로, 상태 정보는 stdout에 JSON으로 출력한다.
    // 문자열 매칭 대신 --json + serde_json 파싱을 사용한다. (D6)
    let output = tokio::process::Command::new(&bin)
        .args(["status", "--json"])
        .output()
        .await
        .map_err(|e| format!("colima 실행 실패: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    // 미기동이면 exit 1 + stdout 없음 → STOPPED로 조기 반환 (실측: colima 0.10.3)
    let raw: Option<ColimaStatusRaw> = if output.status.success() {
        serde_json::from_str(&stdout).ok()
    } else {
        None
    };

    let Some(raw) = raw else {
        return Ok(ClusterStatus {
            is_running: false,
            kubernetes_active: false,
            mlflow_ready: false,
            seaweedfs_ready: false,
        });
    };

    let is_running = true; // exit 0 + JSON 출력 = 기동 중
    let kubernetes_active = raw.kubernetes;

    // kubernetes_active일 때만 배포 상태를 조회한다 (D3 정합: get_cluster_status가
    // MLflow/SeaweedFS 파드 헬스체크까지 리턴). kubectl get deploy -n default -o json을
    // 파싱해 availableReplicas > 0 여부로 준비 상태를 판정하는 간단 스케치.
    let (mlflow_ready, seaweedfs_ready) = if kubernetes_active {
        let kubectl = resolve_cli_path("kubectl")?;
        let deploy_out = tokio::process::Command::new(&kubectl)
            .args(["--context", "colima", "get", "deploy", "-n", "default", "-o", "json"])
            .output()
            .await
            .map_err(|e| format!("kubectl get deploy 실패: {e}"))?;
        let json: serde_json::Value =
            serde_json::from_slice(&deploy_out.stdout).unwrap_or(serde_json::json!({"items": []}));
        let items = json["items"].as_array().cloned().unwrap_or_default();
        let is_ready = |name: &str| {
            items.iter().any(|d| {
                d["metadata"]["name"].as_str() == Some(name)
                    && d["status"]["availableReplicas"].as_u64().unwrap_or(0) > 0
            })
        };
        (is_ready("mlflow"), is_ready("seaweedfs"))
    } else {
        (false, false)
    };

    Ok(ClusterStatus { is_running, kubernetes_active, mlflow_ready, seaweedfs_ready })
}

#[tauri::command]
pub async fn start_cluster(cpu: u32, memory: u32) -> Result<String, String> {
    let bin = resolve_cli_path("colima")?;

    // 프론트 입력을 신뢰하지 않는다: 감지된 호스트 RAM 기준 D4 상한으로 memory를,
    // 호스트 코어 수 기준으로 cpu를 clamp한다 (조작된/오래된 프론트 값 방어).
    let mut sys = sysinfo::System::new_all();
    sys.refresh_memory();
    sys.refresh_cpu_usage();
    let host_ram_gb = sys.total_memory() / 1024 / 1024 / 1024;
    let host_cores = sys.cpus().len().max(1) as u32;
    let max_memory_gb: u64 = match host_ram_gb {
        0..=23 => 4,
        24..=55 => 8,
        _ => 12,
    };
    let memory = memory.min(max_memory_gb as u32).max(1);
    let cpu = cpu.clamp(1, host_cores);

    // colima start는 VM 최초 부팅 시 수 분이 걸릴 수 있어, 블로킹 std::process 대신
    // tokio::process를 사용해 Tokio 워커 스레드를 점유하지 않는다. (D7)
    let output = tokio::process::Command::new(bin)
        .args([
            "start",
            "--cpu", &cpu.to_string(),
            "--memory", &memory.to_string(),
            "--vm-type=vz",
            "--mount-type=virtiofs",
            "--kubernetes",
        ])
        .output()
        .await
        .map_err(|e| format!("colima start 실행 실패: {e}"))?;

    if output.status.success() {
        Ok("Colima K8s 클러스터가 시작되었습니다.".into())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

#[tauri::command]
pub async fn stop_cluster() -> Result<String, String> {
    let bin = resolve_cli_path("colima")?;
    let output = tokio::process::Command::new(bin)
        .arg("stop")
        .output()
        .await
        .map_err(|e| format!("colima stop 실행 실패: {e}"))?;

    if output.status.success() {
        Ok("Colima 클러스터가 정지되었습니다.".into())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}
```

### 2.1 포트포워드 매니저 (`src-tauri/src/commands/port_forward.rs`)

FR-02.2의 MLflow(5001→5000)·SeaweedFS(8333/8888) 포트포워딩은 `kubectl port-forward`를 앱이 관리하는 자식 프로세스로 spawn하고, `Child` 핸들을 `tauri::State`로 추적해 `stop_cluster`나 앱 종료 시 반드시 정리한다.

```rust
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::process::Child;
use tauri::State;
use crate::services::process::resolve_cli_path;

#[derive(Default)]
pub struct PortForwardState(pub Mutex<HashMap<&'static str, Child>>);

#[tauri::command]
pub async fn start_port_forward(state: State<'_, PortForwardState>) -> Result<String, String> {
    let kubectl = resolve_cli_path("kubectl")?;
    let jobs: [(&str, &str, &str); 3] = [
        ("mlflow", "svc/mlflow", "5001:5000"),
        ("seaweedfs-s3", "svc/seaweedfs", "8333:8333"),
        ("seaweedfs-filer", "svc/seaweedfs", "8888:8888"),
    ];
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    for (key, svc, ports) in jobs {
        let child = tokio::process::Command::new(&kubectl)
            .args(["--context", "colima", "port-forward", "-n", "default", svc, ports])
            .spawn()
            .map_err(|e| format!("port-forward({key}) 실행 실패: {e}"))?;
        guard.insert(key, child);
    }
    Ok("포트포워딩이 시작되었습니다.".into())
}

#[tauri::command]
pub async fn stop_port_forward(state: State<'_, PortForwardState>) -> Result<String, String> {
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    for (_, mut child) in guard.drain() {
        let _ = child.kill().await;
    }
    Ok("포트포워딩이 정지되었습니다.".into())
}
```

`stop_cluster`는 colima 정지 직전에 `stop_port_forward`와 동일한 정리 로직을 호출하도록 확장하고, `lib.rs`에서는 `tauri::Window`의 `on_window_event`(또는 `RunEvent::ExitRequested`)에서도 같은 정리를 수행해 앱 종료 시 고아 프로세스가 남지 않도록 한다.

### `src-tauri/src/commands/metrics.rs`

```rust
use std::sync::Mutex;
use serde::Serialize;
use sysinfo::System;
use tauri::State;

#[derive(Serialize)]
pub struct SystemMetrics {
    pub total_memory_gb: f64,
    pub used_memory_gb: f64,
    pub memory_usage_percentage: f32,
    pub cpu_usage_percentage: f32,
}

#[tauri::command]
pub fn get_system_metrics(state: State<'_, Mutex<System>>) -> Result<SystemMetrics, String> {
    // 매 호출마다 System::new_all()을 생성하면 초기 스냅샷 비용이 반복된다.
    // 앱 시작 시 1회 생성한 State를 재사용하고 refresh만 수행한다. (D9)
    let mut sys = state.lock().map_err(|e| e.to_string())?;
    sys.refresh_memory();
    sys.refresh_cpu_usage();

    let total = sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let used = sys.used_memory() as f64 / 1024.0 / 1024.0 / 1024.0;

    Ok(SystemMetrics {
        total_memory_gb: (total * 100.0).round() / 100.0,
        used_memory_gb: (used * 100.0).round() / 100.0,
        memory_usage_percentage: ((used / total * 100.0) as f32 * 10.0).round() / 10.0,
        // sysinfo 0.30 API 기준 전역 CPU 사용률 조회는 global_cpu_info().cpu_usage()를 사용한다.
        cpu_usage_percentage: (sys.global_cpu_info().cpu_usage() * 10.0).round() / 10.0,
    })
}
```
> Phase 1은 sysinfo 기반 RAM/CPU만 측정한다. Metal GPU 사용률(`powermetrics`)은 root 권한이 필요해 Phase 3의 선택 기능으로 분리하며, 본 문서의 구현 대상이 아니다. (D2)

### `src-tauri/src/commands/provision.rs`

```rust
use tauri::Manager;
use crate::services::process::resolve_cli_path;

const MANIFESTS: [&str; 3] = [
    "scripts/k8s/mlflow-deployment.yaml",
    "scripts/k8s/seaweedfs-deployment.yaml",
    "scripts/k8s/mac-gpu-bridge.yaml",
];

#[tauri::command]
pub async fn provision_mlops_stack(app: tauri::AppHandle) -> Result<String, String> {
    let kubectl = resolve_cli_path("kubectl")?;
    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;

    for manifest in MANIFESTS {
        let path = resource_dir.join(manifest);
        // 사용자가 다른 kubectl 컨텍스트를 기본으로 설정해 두었을 경우 매니페스트가
        // 엉뚱한 클러스터에 적용되는 것을 막기 위해 --context colima를 항상 명시한다.
        let output = tokio::process::Command::new(&kubectl)
            .args(["--context", "colima", "apply", "-f"])
            .arg(&path)
            .output()
            .await
            .map_err(|e| format!("kubectl apply 실패({manifest}): {e}"))?;

        if !output.status.success() {
            return Err(format!(
                "{manifest} 적용 실패: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }

    Ok("MLflow / SeaweedFS / GPU 브리지 매니페스트가 적용되었습니다.".into())
}
```
> `run_mlx_finetune`, `kill_mlx_process`는 명령 이름과 시그니처만 예약하고, 상세 구현은 Phase 이후 범위로 둔다. (D3)

**`provision_mlops_stack` 실행 전제**
1. `tauri.conf.json`의 `bundle > resources`에 `scripts/k8s/`를 등록해야 한다 — dev(`tauri dev`)와 번들(`.app`) 양쪽에서 `resource_dir()`이 매니페스트 경로를 해석할 수 있어야 하기 때문이다 (§2 `tauri.conf.json` 스니펫 참조).
2. 모든 kubectl 호출에는 `--context colima`를 명시한다 — 사용자의 기본 kubectl 컨텍스트가 다른 클러스터를 가리키고 있을 때 매니페스트가 잘못 적용되는 사고를 방지한다.
3. 매니페스트는 별도 네임스페이스를 생성하지 않고 `default` 네임스페이스를 사용한다.

### `src-tauri/src/lib.rs`

```rust
mod commands;
mod services;

use std::sync::Mutex;
use sysinfo::System;

use commands::colima::{get_cluster_status, start_cluster, stop_cluster};
use commands::metrics::get_system_metrics;
use commands::port_forward::{start_port_forward, stop_port_forward, PortForwardState};
use commands::provision::provision_mlops_stack;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(System::new_all()))
        .manage(PortForwardState::default())
        .invoke_handler(tauri::generate_handler![
            get_system_metrics,
            get_cluster_status,
            start_cluster,
            stop_cluster,
            provision_mlops_stack,
            start_port_forward,
            stop_port_forward,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

### `src-tauri/capabilities/default.json`

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "dialog:default"
  ]
}
```

### `scripts/k8s/mac-gpu-bridge.yaml`

```yaml
apiVersion: v1
kind: Service
metadata:
  name: mac-gpu-service
  namespace: default
spec:
  type: ExternalName
  externalName: host.lima.internal
  # ports 필드는 의도적으로 생략한다: ExternalName은 DNS CNAME 별칭일 뿐이며
  # 포트 프록시/포워딩을 수행하지 않는다. 클라이언트는 대상 포트(예: :8080)를
  # 직접 지정해 접근해야 한다. (D10)
  # Colima/vz는 Lima 기반이므로 Docker Desktop 전용 호스트 별칭이 아닌
  # host.lima.internal을 사용한다.
```

---

## 3. 프론트엔드 핵심 파일 설계 (React / TypeScript)

### `src/hooks/useColima.ts`

```typescript
import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';

export interface ClusterStatus {
  is_running: boolean;
  kubernetes_active: boolean;
  mlflow_ready: boolean;
  seaweedfs_ready: boolean;
}

export function useColima() {
  const [status, setStatus] = useState<ClusterStatus | null>(null);
  const [loading, setLoading] = useState(false);

  const fetchStatus = useCallback(async () => {
    try {
      setStatus(await invoke<ClusterStatus>('get_cluster_status'));
    } catch (err) {
      console.error('클러스터 상태 로드 오류:', err);
    }
  }, []);

  const startCluster = useCallback(async (cpu: number, memory: number) => {
    setLoading(true);
    try {
      await invoke('start_cluster', { cpu, memory });
      await fetchStatus();
    } catch (err) {
      // wry(WebView)는 브라우저 alert()을 지원하지 않으므로
      // Tauri dialog 플러그인의 message()로 대체한다. (D8)
      await message(`클러스터 구동 실패: ${err}`, { title: 'KubeMetal', kind: 'error' });
    } finally {
      setLoading(false);
    }
  }, [fetchStatus]);

  useEffect(() => {
    fetchStatus();
    // FR-01.3은 갱신 주기를 고정하지 않는다 — 클러스터 상태는 초 단위로 급변하지
    // 않으므로 5s 폴링을 채택한다 (메트릭은 useMetrics에서 별도로 1s 주기 사용).
    const interval = setInterval(fetchStatus, 5000);
    return () => clearInterval(interval);
  }, [fetchStatus]);

  return { status, loading, startCluster, refresh: fetchStatus };
}
```

### `src/hooks/useMetrics.ts`

```typescript
import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

export interface SystemMetrics {
  total_memory_gb: number;
  used_memory_gb: number;
  memory_usage_percentage: number;
  cpu_usage_percentage: number;
}

export function useMetrics(intervalMs = 1000) { // NFR-02: UI 메트릭 갱신 주기 1000ms
  const [metrics, setMetrics] = useState<SystemMetrics | null>(null);

  useEffect(() => {
    let alive = true;
    const tick = async () => {
      try {
        const res = await invoke<SystemMetrics>('get_system_metrics');
        if (alive) setMetrics(res);
      } catch (err) {
        console.error('시스템 메트릭 로드 오류:', err);
      }
    };
    tick();
    const id = setInterval(tick, intervalMs);
    return () => { alive = false; clearInterval(id); };
  }, [intervalMs]);

  return metrics;
}
```

### `src/lib/recommendVmResources.ts`

```typescript
export interface VmResources {
  cpu: number;
  memoryGb: number;
}

// 감지된 전체 RAM(total_memory_gb) 기준 VM 기본 사양 자동 산정.
// 16GB→VM 4GB/2CPU, 32~48GB→8GB/4CPU, 64GB+→12GB/6CPU (D4)
export function recommendVmResources(totalMemoryGb: number): VmResources {
  if (totalMemoryGb >= 64) return { cpu: 6, memoryGb: 12 };
  if (totalMemoryGb >= 32) return { cpu: 4, memoryGb: 8 };
  return { cpu: 2, memoryGb: 4 }; // 16GB급 기본 구성
}
```

### `src/components/dashboard/ClusterControl.tsx`

```tsx
import React from 'react';
import { useColima } from '../../hooks/useColima';
import { useMetrics } from '../../hooks/useMetrics';
import { recommendVmResources } from '../../lib/recommendVmResources';

export const ClusterControl: React.FC = () => {
  const { status, loading, startCluster } = useColima();
  const metrics = useMetrics();
  const { cpu, memoryGb } = metrics
    ? recommendVmResources(metrics.total_memory_gb)
    : { cpu: 2, memoryGb: 4 };

  return (
    <div className="p-6 bg-slate-900 text-white rounded-xl shadow-md border border-slate-800">
      <h2 className="text-xl font-bold mb-4 flex items-center gap-2">
        <span>⚙️</span> Colima K8s Control
      </h2>

      <div className="mb-6 flex items-center gap-2">
        <span className="text-sm text-slate-400">클러스터 상태:</span>
        <span className={`px-2 py-1 text-xs rounded-full font-semibold ${
          status?.is_running ? 'bg-green-500/20 text-green-400' : 'bg-red-500/20 text-red-400'
        }`}>
          {status?.is_running ? 'RUNNING (vz)' : 'STOPPED'}
        </span>
      </div>

      <button
        onClick={() => startCluster(cpu, memoryGb)}
        disabled={loading || status?.is_running || !metrics}
        className="w-full py-2.5 bg-blue-600 hover:bg-blue-500 disabled:bg-slate-700 text-white font-medium rounded-lg transition-colors"
      >
        {loading
          ? '클러스터 구동 중...'
          : `Apple vz 기반 K8s 스타트 (${cpu} CPU / ${memoryGb}GB, 전체 ${metrics?.total_memory_gb ?? '-'}GB 기준 자동 산정)`}
      </button>
    </div>
  );
};
```

---

## 4. 설계 결정 및 주의사항

| ID | 결정 |
|----|------|
| D1 | 포트: MLflow 5001(호스트 포워딩, AirPlay가 5000 점유), SeaweedFS Filer UI 8888, S3 API 8333, 서빙 8080. |
| D2 | Phase 1 메트릭 범위는 sysinfo 기반 RAM/CPU만. Metal GPU/`powermetrics`는 root 권한이 필요해 Phase 3 선택 기능이며 본 문서 구현 대상이 아니다. |
| D3 | IPC 커맨드명 통일: `get_system_metrics`, `get_cluster_status`, `start_cluster{cpu,memory}`, `stop_cluster`, `provision_mlops_stack`, `start_port_forward`, `stop_port_forward`, `run_mlx_finetune`, `kill_mlx_process`(뒤 2개는 이름만 예약). |
| D4 | UI의 CPU/메모리 하드코딩("6 CPU / 12GB") 제거 → `get_system_metrics`로 감지한 전체 RAM 기반 자동 산정(16GB→VM 4GB/2CPU, 32~48GB→8GB/4CPU, 64GB+→12GB/6CPU). |
| D5 | macOS `.app`은 로그인 셸 PATH를 상속하지 않아 `Command::new("colima")`가 실패한다 → `resolve_cli_path`가 `/opt/homebrew/bin`, `/usr/local/bin` 등을 탐색해 절대경로로 실행. |
| D6 | colima 상태 출력(logrus)은 stderr로 나가 stdout 문자열 매칭은 항상 실패한다 → `colima status --json`을 serde로 파싱, `vm_type` 하드코딩 제거. |
| D7 | async `tauri::command` 내부의 `std::process::Command::output()` 블로킹 호출 금지(특히 수 분 걸리는 `colima start`) → `tokio::process::Command` 사용. |
| D8 | wry(WebView)는 JS `alert()`를 지원하지 않는다 → 프론트 훅에서 `@tauri-apps/plugin-dialog`의 `message`/`ask` 사용. |
| D9 | 매 호출 `System::new_all()` 생성 대신 `tauri::State<Mutex<System>>`로 앱 시작 시 1회 생성 후 `refresh_*`만 수행. |
| D10 | `mac-gpu-bridge.yaml`은 `type: ExternalName`이며 `ports` 필드를 선언하지 않는다 — ExternalName은 CNAME 별칭일 뿐 포트 프록시를 수행하지 않으므로, 클라이언트가 대상 포트를 직접 지정해야 한다. |
| D11 | OOM 가드(FR-05.2)는 "가용 RAM 비율" 기준이 아니라 macOS memory pressure 레벨(warn/critical) 기반으로 트리거한다(파일 캐시로 RAM은 상시 높게 점유). Phase 3 범위. |
| D12 | 서빙 도구 표기는 mlx_lm.server(mlx-lm 패키지) 또는 llama-server — "mlx-serve"라는 도구는 존재하지 않는다. |
| D13 | MLflow 아티팩트 스토어는 SeaweedFS S3(endpoint http://seaweedfs:8333, 버킷 mlflow, 더미 크리덴셜 환경변수)로 자동 구성한다 — "설치"가 아니라 "연동"이 목표(FR-06). |

**원본(`arch.md`) 대비 변경 요약**: `commands/provision.rs` 신설(D3), `lib.rs`의 `System` State 등록·`tauri-plugin-dialog` 플러그인 등록·들여쓰기 정정, `services/process.rs`에 `resolve_cli_path` 추가, `colima.rs`를 `get_cluster_status`/`start_cluster`/`stop_cluster`로 개명하며 JSON 파싱과 `tokio::process`로 전환, `metrics.rs`에 `State<Mutex<System>>` 도입, 프론트에 `useMetrics`·`recommendVmResources` 신설 및 `useColima`의 `alert()`를 dialog 플러그인으로 교체, `capabilities/default.json`에 `dialog:default` 권한 추가, `scripts/k8s/mac-gpu-bridge.yaml` 신설, `commands/port_forward.rs` 신설(D3) 및 `stop_cluster`/앱 종료 시 정리 로직 추가, `start_cluster`의 백엔드 clamp 도입, `get_cluster_status`에 `mlflow_ready`/`seaweedfs_ready` 확장.

---

## 5. 미검증 전제 (실기기 검증 필요)

본 설계는 다음 항목들을 전제로 하며, 실제 Colima/vz 환경에서의 검증이 완료되지 않았다.

1. ~~`host.lima.internal` DNS 해석~~ **(2026-07-20 실측 검증 완료)**: 파드 내부에서 CoreDNS(10.43.0.10)가 `host.lima.internal` → `192.168.5.2`(Lima 호스트 게이트웨이)로 정상 해석하며, `mac-gpu-service.default.svc.cluster.local`의 ExternalName CNAME 체인도 동일 주소로 해석됨을 busybox 파드 nslookup으로 확인했다.
2. ~~`colima status --json`의 필드 스키마~~ **(2026-07-20 실측 검증 완료, colima 0.10.3)**: 초안 가정(`status: "Running"` 문자열 + `kubernetes.enabled` 중첩)은 틀렸다. 실제 출력은 기동 중일 때만 exit 0 + stdout에 평면 JSON(`{"kubernetes":true,"cpu":6,"memory":...,...}`)이며 `status` 필드는 없고, 미기동이면 exit 1 + stdout 없음(stderr에 logrus fatal). `ColimaStatusRaw`는 `kubernetes: bool` 평면 매핑으로 수정됐고, 기동 여부는 "exit 0 + JSON 파싱 성공" 자체로 판별한다. 버전별 스키마 변동 가능성은 여전히 있으므로 colima 업그레이드 시 재확인.
3. **`resource_dir()`의 dev/번들 경로 차이**: `tauri dev` 실행 시와 `.app` 번들 실행 시 `resource_dir()`이 반환하는 실제 경로가 다를 수 있어, `scripts/k8s/` 매니페스트 탐색이 두 모드 모두에서 동작하는지 확인이 필요하다.
4. **포트포워드 프로세스의 생존성**: `kubectl port-forward` 자식 프로세스가 macOS 슬립/네트워크 단절 후에도 자동 재연결되는지, 혹은 좀비 상태로 남아 `stop_port_forward`가 무의미해지는지 검증이 필요하다.
5. ~~`chrislusf/seaweedfs:3.73` 이미지 태그 실존 및 S3 게이트웨이 동작~~ **(2026-07-20 실측 검증 완료)**: 태그 풀·기동 정상(파드 1/1 Ready), 포트포워딩 후 S3 API(8333)가 `ListAllMyBucketsResult` XML을, Filer UI(8888)와 MLflow(5001)가 HTTP 200을 반환함을 curl로 확인했다.
