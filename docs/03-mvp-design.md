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
| D1 | (2026-07-25 개정) 포트: MLflow 5001(호스트 포워딩, AirPlay가 5000 점유), SeaweedFS Filer UI 8888, S3 API 8333, 서빙 8080(`suggest_serving_port`가 8080~8099를 순회), Prefect 서버 4200(D19), **kagent UI 8090**. kagent UI를 8080으로 포워딩하면 모델 서빙과 정면 충돌한다 — `make forward`는 `8090:8080`으로 매핑하고 UI 링크(`KagentOpsView`, 파이프라인 스테이지)도 `http://127.0.0.1:8090`을 쓴다(서빙 URL과 마찬가지로 `localhost` 금지, mistakes-log 2026-07-21). |
| D2 | (2026-07-24 개정) Phase 1 메트릭 범위는 sysinfo 기반 RAM/CPU. `powermetrics`(root 필요) 기반 GPU 측정은 계속 금지한다 — 대신 GPU 사용률/메모리는 sudo-free `ioreg -l -d 1 -r -c IOAccelerator`(D16과 동일 원칙, 일반 유저 권한으로 조회 가능) 출력 파싱으로 Phase 4에서 `get_system_metrics`에 `gpu_usage_percentage`/`gpu_memory_used_gb` 필드로 추가했다. `metrics.rs::get_metal_gpu_metrics`는 `external_command("ioreg")`(D5 확장, `/usr/sbin` 탐색)로 스폰한다 — bare `Command::new("ioreg")`는 GUI 번들 앱의 빈 PATH에서 항상 실패해 GPU 값이 0으로 고정되는 회귀가 있었다(2026-07-24 실기기 확인: `env -i PATH=/usr/bin:/bin /usr/sbin/ioreg ...`는 절대경로로는 정상 동작, PATH 탐색 의존 호출만 실패). |
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
| D14 | 모델 허브는 CLI 래퍼 패턴(curl)로 HF API를 호출하고 로컬 저장 경로는 ~/.kubemetal/models/{repo_id 슬러그}이다. 업로드 대상은 SeaweedFS S3 버킷 models. |
| D15 | MLX 학습은 앱 전용 venv(~/.kubemetal/venv)의 mlx-lm으로 실행하고, 번들 파이썬 래퍼가 stdout JSON 라인(진행)과 MLflow REST(메트릭)를 담당한다. |
| D16 | 가드레일(FR-05.2/05.3) 판정 소스는 실기기 실측(2026-07-21, uid 501, sudo 불필요)으로 확정: memory pressure는 `sysctl -n kern.memorystatus_vm_pressure_level` 출력이 `1`/`2`/`4`(각각 normal/warn/critical)이며, 전원 상태는 `pmset -g batt` 출력에 `'Battery Power'`(배터리 구동) 또는 `'AC Power'`(AC 전원) 문자열이 포함되는지로 판별한다. 두 CLI 모두 `/usr/sbin`, `/usr/bin`에 위치해 `resolve_cli_path`(D5) 탐색 경로에 `/usr/sbin`을 추가했다. |
| D17 | `run_mlx_finetune`이 기동하는 파인튜닝 래퍼(`finetune_wrapper.py`)는 내부에서 `subprocess.Popen`으로 실제 학습을 수행하는 `mlx_lm` 자식 프로세스를 띄운다. 실기기 실측(2026-07-21)으로 확인: 래퍼 단일 pid에만 SIGSTOP을 보내면 래퍼만 멈추고 `mlx_lm` 자식은 GPU 연산을 계속해 학습을 끝까지 완주해버려 가드레일이 완전히 무력화된다(SIGKILL/SIGTERM도 동일하게 래퍼만 죽이고 자식은 orphan으로 남아 계속 실행됨). 해결: 래퍼를 `tokio::process::Command::process_group(0)`으로 기동해 자신이 리더인 새 프로세스 그룹을 만들고(자식은 그룹을 상속), 가드레일 SIGSTOP/SIGCONT 및 `kill_mlx_process`의 SIGTERM/SIGKILL을 그룹 전체(`-pid`)로 전송한다. 그룹 대상 실측: 그룹 SIGSTOP 후 래퍼·자식 모두 `T` 상태로 멈추고 CPU 시간이 정지, SIGCONT 후 자식이 재개되어 정상적으로 61개 프로그레스 라인 전부와 `done` 이벤트를 방출하며 완료됨을 확인했다. 서빙 프로세스(`start_model_serving`)는 자식이 없는 단일 프로세스라 새 그룹 없이 기존 단일 pid 시그널을 유지한다. |
| D19 | 파이프라인 오케스트레이션은 Prefect 3을 채택한다(docs/05-mlops-research.md Q1). 서버는 K3s 파드 1개(`prefecthq/prefect:3.7.8-python3.12` — 2026-07-23 Docker Hub API 실존 확인, 설치된 venv `prefect` 3.7.8과 동일 버전)로 `prefect server start --host 0.0.0.0`를 실행하며 SQLite 백엔드(`PREFECT_HOME=/data`)를 1Gi PVC(k3s 기본 `local-path` StorageClass)에 영속화하고 requests 256Mi/limits 1Gi로 D4 VM 예산 안에 든다. 워크풀은 사용하지 않는다 — GPU 연산은 K8s에서 실행할 수 없다는 아키텍처 불변식(제어면=K8s, 연산=macOS 호스트)에 따라, `finetune`/`evaluate` 두 flow를 호스트 venv python(`scripts/prefect/host_runner.py`)이 `flow.to_deployment()` + `serve()`로 직접 폴링·실행하는 "Process Worker" 패턴을 쓴다(실기기 확인, 2026-07-23: `prefect==3.7.8`의 `serve(*deployments)`/`Flow.to_deployment(name, ...) -> RunnerDeployment` 시그니처를 venv에서 `inspect.signature`로 직접 확인). REST 경로(`POST /flow_runs/filter`, `GET /deployments/name/{flow}/{deployment}`, `POST /deployments/{id}/create_flow_run`, `GET /api/health`)와 flow run 상태 스키마(최상위 `state_type`/`state_name` 필드)는 실기기에 배포한 서버에 직접 curl로 확인했으며, 5-iter 스모크 파인튜닝을 이 경로로 트리거해 flow run이 COMPLETED로 완주함을 확인했다. `host_runner.py`는 Rust가 `process_group(0)`으로 기동(D17과 동일 원칙)하므로, 내부에서 `subprocess.Popen`으로 띄우는 `finetune_wrapper.py`와 그 `mlx_lm` 학습 자식까지 프로세스 그룹을 상속받아 정지 시그널이 트리 전체에 전파된다. |
| D18 | `resource_dir()`가 가리키는 `.app` 번들의 `Contents/Resources`에는 `tauri.conf.json`의 `../scripts/...` 리소스가 `_up_/scripts/...`로 평탄화되어 담긴다(실측, §5 전제 #3). `services/process.rs::resolve_bundled_resource()`가 `_up_/<relative>`를 우선 시도하고 없으면 평탄화 없는 경로로 폴백하며, `provision.rs`/`mlx.rs`가 이를 통해 매니페스트·파인튜닝 래퍼 경로를 해석한다. 같은 실측 과정에서 `provision.rs`의 `MANIFESTS`에 `scripts/k8s/seaweedfs-s3-credentials.yaml` Secret이 누락돼 있던 것도 발견해 추가했다. |
| D21 | (2026-07-24, 감사 수정) 수집 파이프라인(Phase 5a, `run_data_ingest`/`ingest_host.py`)과 RAG DVC 커밋(Phase 4c, `dvc_commit_dataset`/`rag_host.py`)에 두 가지 하드닝을 적용한다. **(1) SSRF 가드**: `source_type`이 web/rss일 때 대상 URL은 scheme allowlist(http/https만)와 사설/루프백 호스트 거부(loopback 127.0.0.0/8, RFC1918 10.0.0.0/8·172.16.0.0/12·192.168.0.0/16, link-local 169.254.0.0/16, `localhost`/`*.internal`/`*.local`, 대응 IPv6 범위)를 통과해야 한다. Rust(`data_ingest.rs::validate_ingest_url`)와 Python(`ingest_host.py::_validate_url`) 양쪽에 동일 규칙을 이중 구현했다 — 스크립트가 Rust 스폰 없이 단독 실행되는 경로(수동 검증, 향후 CLI 진입점)도 방어선 밖에 남지 않도록 함이다. 실기기 확인(2026-07-24): `file:///etc/passwd`(스킴 거부), `http://127.0.0.1:4200`(루프백 거부) 모두 `status: "error"`로 즉시 거부됨을 `ingest_host.py` 단독 실행으로 확인했다. **(2) 크리덴셜 env 주입**: SeaweedFS S3 자격증명(`access-key-id`/`secret-access-key`)을 CLI 인자로 넘기던 기존 방식(`ps`로 프로세스 인자가 노출됨, 게다가 `seaweedfsadmin`이라는 `seaweedfs-s3-credentials.yaml`(D13)의 실제 더미값(`kubemetal`/`kubemetal-local`)과도 불일치하는 하드코딩값이었다)을 제거했다. `access.rs::resolve_s3_credentials()`가 실제 K8s Secret을 조회하고(실패 시 Secret 매니페스트와 동일한 기본값으로 폴백) `KUBEMETAL_S3_ACCESS_KEY`/`KUBEMETAL_S3_SECRET_KEY` 환경변수로 자식 프로세스에 주입하며, 파이썬 스크립트는 `os.environ`에서 읽는다. 실기기 확인(2026-07-24): 로컬 소스 4-노드(extract/clean_chunk/lancedb_index/dvc_backup) 전체가 이 경로로 `completed` 상태에 도달했고(SeaweedFS 볼륨 용량 부족은 이 태스크와 무관한 기존 클러스터 상태였으며 버킷 생성 후 자격증명 자체는 정상 인증됨을 `NoSuchBucket`→(버킷 생성 후) `PutObject` 단계까지 도달로 확인), `subprocess` 모듈 미임포트로 DVC 백업 노드가 NameError로 항상 실패하던 회귀도 함께 수정했다(`import subprocess` 누락, except 절이 NameError 문자열을 그대로 삼켜 원인 파악이 어려웠음). `fetch_url_bytes`의 미검증 SSL 폴백(`ssl._create_unverified_context()`)도 제거해 TLS 검증이 항상 강제되도록 했다. |
| D20 | 평가 스택은 lm-evaluation-harness(`local-completions` 모델 타입, `pip install "lm-eval[api]"` — `api` extra 없이는 `tenacity`/`tiktoken` 미설치로 즉시 실패, 실기기 실측 2026-07-23) + MLflow experiment `kubemetal-eval`을 채택한다(docs/05-mlops-research.md Q2). `mlx_lm.server`는 `/v1/completions`·`/v1/chat/completions`를 모두 지원하므로(서버 소스 `do_POST` 라우팅 실측) `base_url={serving_url}/completions`로 무수정 연결한다. `host_runner.py::evaluate_flow`가 venv `python -m lm_eval run --model local-completions --model_args base_url=...,model=...,num_concurrent=1,tokenized_requests=False --tasks ... --limit ... --output_path <tmpdir>`를 서브프로세스로 실행하고 `output_path`에 쓰인 `results_*.json`(`results[task]`에 `"metric,filter"` 키 → float, `alias`/`name`/`sample_len`은 메트릭이 아닌 메타데이터 — 실측)을 파싱, `"task/metric/filter"`(MLflow 메트릭 키는 콤마를 허용하지 않아 슬래시로 치환)로 평탄화해 MLflow REST(`runs/create`+`log-batch`+`runs/update`)로 기록한다. MLflow REST 클라이언트는 `finetune_wrapper.py`와 공유하는 `scripts/mlx/mlflow_reporter.py::MlflowReporter`로 통합했다(experiment 이름·경고 콜백만 호출자별로 주입). 실기기 E2E(2026-07-23): 8081에 mlx_lm.server(Qwen2.5-0.5B-Instruct-4bit) 기동 → Prefect REST로 evaluate deployment 트리거(tasks=gsm8k, limit=4) → flow run COMPLETED → MLflow `kubemetal-eval` run에 `gsm8k/exact_match/strict-match=0.25` 등 4개 메트릭과 `serving_url`/`model`/`tasks`/`limit` 파라미터가 기록됨을 `runs/search` 원문으로 확인. LLM-as-a-judge 스코어러(MLflow 3.x GenAI evaluate)는 후속 범위로 미룬다. |

| D22 | (2026-07-25, 감사 수정) **"모르면 비운다" 규약 — 조회 실패를 그럴듯한 값으로 채우지 않는다.** Phase 5b 인수 검토에서 조작된 값이 다수 발견돼 다음을 불변식으로 승격한다. (1) `get_kagent_diagnostics`는 kubectl 실측에서만 파생한다 — `kagent_ready`는 kagent ns 파드가 1개 이상이면서 전부 Ready condition=True일 때만 true이고(phase만 보면 CrashLoopBackOff 파드도 Running으로 통과했다), `active_agents`는 Ready 파드의 라벨에서만 수집하며(비어 있을 때 `["k8s-agent","helm-agent"]`를 주입하던 코드 제거), 원인 문자열은 클러스터가 준 `waiting.reason`/`message`만 인용한다(모든 waiting 사유를 "이미지 태그를 찾을 수 없음"으로 단정하던 코드 제거). 조회 실패는 Err이며 프론트도 "정상"으로 폴백하지 않는다. (2) `list_kubeconfig_contexts`는 실패 시 Err — 하드코딩 컨텍스트(`colima`/`narwhal-idp`)를 지어내지 않는다. (3) `toggle_kagent_agent`는 kubectl stdout만 인용하고 "파드가 1/1 Running" 같은 미확인 상태를 주장하지 않으며, delete 실패를 Ok로 삼키지 않는다. (4) `get_hardware_spec`은 `external_command`(D5, 비블로킹 — 기존 구현은 async 커맨드 안에서 블로킹 `std::process::Command`를 4회 호출해 D7 위반이자 GUI 번들의 빈 PATH에서 실패하는 D2 회귀 패턴이었다)로 sysctl을 조회하고, 실패 시 `Apple M4 Pro`/14/20/64GB를 채우는 대신 Err 또는 `gpu_cores: None`을 돌려준다(다른 기기에서 허위 스펙이 표시됐다). (5) UI 레이어에도 동일 규약 적용 — App 하단 Dock의 고정 로그 3줄, 사이드바 하드웨어 폴백, `SpecializedModelGuideCard`(kagent Ops 탭과 중복 구현이라 삭제)의 허위 폴백 리포트를 모두 제거했다. |
| D23 | (2026-07-25) Air-Gap 오프라인 번들은 `~/.kubemetal/airgap`(binaries/charts/images/manifests)에 `.tar.gz`로 보관하고, 수집·설치 스크립트는 `scripts/airgap/`에 두며 `tauri.conf.json`의 `bundle.resources`에 `../scripts/airgap/*`를 등록해 `resolve_bundled_resource`(D18)로 해석한다 — 상대경로 `scripts/airgap/...`는 `.app` 실행 시 CWD가 프로젝트 루트가 아니라 항상 실패한다. 스크립트는 실패를 `\|\| true`로 삼키지 않는다: 부분 수신 파일은 `.part`로 받아 성공 시에만 최종 이름으로 옮기고(0바이트/HTML 에러 페이지가 남으면 `get_airgap_status`가 그것을 "보유 완료"로 보고한다), 실패 항목을 모아 마지막에 출력하며 0이 아닌 코드로 종료한다. 업스트림 버전 비교는 접두 `v`와 빌드 메타데이터(`+k3s1`)를 제거한 뒤 수행하고, 조회 실패는 "최신"이 아니라 "조회 실패"로 표시한다. **파일 존재 ≠ 보유**: `get_airgap_status`는 `MIN_VALID_ASSET_BYTES`(1KiB — 바이너리·`.tgz`·`.tar.gz` 어느 쪽으로도 유효할 수 없는 하한) 미만인 파일을 `corrupt`로 분류해 보유 수와 총 용량에서 제외하고, UI는 정확한 바이트(`size_bytes`, MB 반올림 시 0으로 뭉개짐)와 함께 "손상 — 재다운로드"로 표시한다. 다운로더도 같은 하한으로 "이미 보유"를 판정해 손상 파일을 건너뛰지 않고 폐기 후 재수집한다. 실기기 확인(2026-07-25): 구버전 다운로더가 남긴 9바이트 `Not Found` 본문이 `binaries/kubescape`에 있었고 UI가 "보유 (0 MB)"·13/13(100%)로 보고하던 것을, 수정 후 "손상 (9B)"·12/13(92%)로 정정함을 실행 중인 앱에서 확인했다. |
| D24 | (2026-07-25) `scripts/e2e/` 검증 스위트는 **하지 않은 일을 성공으로 보고하지 않는다**. 이전 구현은 서빙 미가동 시 목 응답으로 학습 데이터를 채우고(01), 더미 LoRA 가중치와 조작된 메트릭(`train_loss=0.241`)을 출력했으며(02), kagent 진단 문장을 코드에 적어 두고 출력했고(03), 러너는 파이썬이 실패해도 항상 `🎉 SUCCESS`를 찍었다. 개정 규약: 01은 서빙 엔드포인트 실패 시 종료 코드 1(모의 데이터 생성 금지), 02는 학습을 흉내 내지 않고 MLflow(:5001)·SeaweedFS(:8333) 도달성만 검증하며 범위를 명시(실제 학습은 MLX 스튜디오), 03은 kubectl이 보고한 `reason`/`message`만 출력, 04는 client/server dry-run 후 실제 적용하고 Ready까지 확인한다(GitOps PR 시뮬레이션 제거). 러너는 첫 실패에서 멈추고 실패 단계를 명시하며 종료 코드를 전파한다. |

**원본(`arch.md`) 대비 변경 요약**: `commands/provision.rs` 신설(D3), `lib.rs`의 `System` State 등록·`tauri-plugin-dialog` 플러그인 등록·들여쓰기 정정, `services/process.rs`에 `resolve_cli_path` 추가, `colima.rs`를 `get_cluster_status`/`start_cluster`/`stop_cluster`로 개명하며 JSON 파싱과 `tokio::process`로 전환, `metrics.rs`에 `State<Mutex<System>>` 도입, 프론트에 `useMetrics`·`recommendVmResources` 신설 및 `useColima`의 `alert()`를 dialog 플러그인으로 교체, `capabilities/default.json`에 `dialog:default` 권한 추가, `scripts/k8s/mac-gpu-bridge.yaml` 신설, `commands/port_forward.rs` 신설(D3) 및 `stop_cluster`/앱 종료 시 정리 로직 추가, `start_cluster`의 백엔드 clamp 도입, `get_cluster_status`에 `mlflow_ready`/`seaweedfs_ready` 확장.

---

## 5. 미검증 전제 (실기기 검증 필요)

본 설계는 다음 항목들을 전제로 하며, 실제 Colima/vz 환경에서의 검증이 완료되지 않았다.

1. ~~`host.lima.internal` DNS 해석~~ **(2026-07-20 실측 검증 완료)**: 파드 내부에서 CoreDNS(10.43.0.10)가 `host.lima.internal` → `192.168.5.2`(Lima 호스트 게이트웨이)로 정상 해석하며, `mac-gpu-service.default.svc.cluster.local`의 ExternalName CNAME 체인도 동일 주소로 해석됨을 busybox 파드 nslookup으로 확인했다.
2. ~~`colima status --json`의 필드 스키마~~ **(2026-07-20 실측 검증 완료, colima 0.10.3)**: 초안 가정(`status: "Running"` 문자열 + `kubernetes.enabled` 중첩)은 틀렸다. 실제 출력은 기동 중일 때만 exit 0 + stdout에 평면 JSON(`{"kubernetes":true,"cpu":6,"memory":...,...}`)이며 `status` 필드는 없고, 미기동이면 exit 1 + stdout 없음(stderr에 logrus fatal). `ColimaStatusRaw`는 `kubernetes: bool` 평면 매핑으로 수정됐고, 기동 여부는 "exit 0 + JSON 파싱 성공" 자체로 판별한다. 버전별 스키마 변동 가능성은 여전히 있으므로 colima 업그레이드 시 재확인.
3. ~~`resource_dir()`의 dev/번들 경로 차이~~ **(2026-07-21 실측 검증 완료, tauri 2.11.5)**: `pnpm tauri build`로 생성한 `.app` 번들을 `find`로 실측한 결과, `resource_dir()`은 항상 `Contents/Resources`를 가리키지만 `tauri.conf.json`의 `bundle.resources`에 등록한 `../scripts/k8s/*`, `../scripts/mlx/*`처럼 `src-tauri/` 상위를 참조하는 리소스는 번들 내부에 `Contents/Resources/_up_/scripts/k8s/...`, `Contents/Resources/_up_/scripts/mlx/...`로 `../` 세그먼트가 `_up_/`로 평탄화되어 담긴다. 기존 `provision.rs`/`mlx.rs`는 `resource_dir.join("scripts/k8s/...")`처럼 `_up_` 없이 경로를 조합하고 있어 번들 실행 시 매니페스트/래퍼 스크립트를 찾지 못하는 버그였다. `services/process.rs`에 `resolve_bundled_resource()`를 추가해 `_up_/<relative>` 경로를 우선 시도하고 없으면(예: `tauri dev`) 평탄화 없는 경로로 폴백하도록 수정했고, 재빌드한 번들에서 매니페스트 4종(Secret 포함) + `finetune_wrapper.py` 5개 파일 모두 `_up_/scripts/...` 경로로 정상 해석됨을 확인했다. 또한 `provision.rs`의 `MANIFESTS`에 `scripts/k8s/seaweedfs-s3-credentials.yaml` Secret이 누락되어 있던 것도 함께 발견해 추가했다(D13 SeaweedFS S3 크리덴셜 와이어링에 필수 — mlflow-deployment.yaml이 이 Secret을 `secretKeyRef`로 참조한다).
4. **포트포워드 프로세스의 생존성**: `kubectl port-forward` 자식 프로세스가 macOS 슬립/네트워크 단절 후에도 자동 재연결되는지, 혹은 좀비 상태로 남아 `stop_port_forward`가 무의미해지는지 검증이 필요하다.
5. ~~`chrislusf/seaweedfs:3.73` 이미지 태그 실존 및 S3 게이트웨이 동작~~ **(2026-07-20 실측 검증 완료)**: 태그 풀·기동 정상(파드 1/1 Ready), 포트포워딩 후 S3 API(8333)가 `ListAllMyBucketsResult` XML을, Filer UI(8888)와 MLflow(5001)가 HTTP 200을 반환함을 curl로 확인했다.
