**Phase 1 MVP 구축을 위한 Tauri v2 + Rust 백엔드 및 React(TypeScript) 프론트엔드 통합 프로젝트의 폴더 구조와 핵심 코드 설계**입니다.

---

### 1. 프로젝트 전체 디렉터리 구조

```text
kubemetal/
├── src/                            # Frontend (React + TypeScript + Tailwind)
│   ├── assets/
│   ├── components/
│   │   ├── dashboard/              # 시스템 자원(RAM/GPU) 및 Colima 상태 뷰
│   │   ├── services/               # MLflow / MinIO 원클릭 프로비저닝 UI
│   │   └── common/                 # 공통 UI 컴포넌트
│   ├── hooks/                      # Tauri IPC 커스텀 훅 (useColima, useMetrics)
│   ├── types/                      # Rust 백엔드와 맞춘 TypeScript 타입 정의
│   ├── App.tsx
│   └── main.tsx
├── src-tauri/                      # Backend (Rust Native Control Agent)
│   ├── src/
│   │   ├── main.rs                 # Tauri 실행 엔트리포인트
      ├── lib.rs                  # 핸들러 및 모듈 등록
│   │   ├── commands/               # Frontend에서 호출하는 tauri::command 모음
│   │   │   ├── mod.rs
│   │   │   ├── colima.rs           # Colima/K3s 프로세스 제어
│   │   │   └── metrics.rs          # macOS System/Metal GPU 자원 측정
│   │   └── services/               # CLI 래퍼 및 시스템 인터페이스
│   │       ├── process.rs          # std::process 비동기 실행기
│   │       └── sysinfo.rs          # sysinfo 및 powermetrics 호출
│   ├── capabilities/               # Tauri v2 권한 설정 (default.json)
│   ├── tauri.conf.json             # Tauri v2 앱 설정
│   └── Cargo.toml                  # Rust 의존성 설정
├── scripts/                        # 로컬 인프라 스크립트 및 매니페스트
│   └── k8s/
│       ├── mlflow-deployment.yaml  # K8s 배포용 MLflow 서버
│       └── minio-deployment.yaml   # K8s 배포용 MinIO 스토리지
├── package.json
└── tsconfig.json

```

---

### 2. 백엔드 핵심 파일 설계 (Rust / Tauri v2)

#### `src-tauri/Cargo.toml`

시스템 메트릭 측정 및 Colima 제어를 위한 필수 크레이트 설정입니다.

```toml
[package]
name = "kubemetal"
version = "0.1.0"
edition = "2021"

[build-dependencies]
tauri-build = { version = "2.0.0", features = [] }

[dependencies]
tauri = { version = "2.0.0", features = [] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1.0", features = ["full"] }
sysinfo = "0.30" # macOS RAM/CPU 모니터링

```

#### `src-tauri/src/commands/colima.rs`

Colima 기반 K8s 클러스터 생성을 제어하는 Rust 명령 모듈입니다.

```rust
use std::process::Command;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct ColimaStatus {
    pub is_running: bool,
    pub vm_type: String,
    pub kubernetes_active: bool,
}

// Colima 클러스터 상태 확인
#[tauri::command]
pub async fn get_colima_status() -> Result<ColimaStatus, String> {
    let output = Command::new("colima")
        .arg("status")
        .output()
        .map_err(|e| format!("Colima 실행 실패: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let is_running = output.status.success() && stdout.contains("colima is running");

    Ok(ColimaStatus {
        is_running,
        vm_type: "vz".to_string(),
        kubernetes_active: stdout.contains("kubernetes: enabled"),
    })
}

// Apple vz 엔진 기반 Colima K8s 스타트
#[tauri::command]
pub async fn start_colima(cpu: u32, memory: u32) -> Result<String, String> {
    let output = Command::new("colima")
        .args([
            "start",
            "--cpu", &cpu.to_string(),
            "--memory", &memory.to_string(),
            "--vm-type=vz",
            "--mount-type=virtiofs",
            "--kubernetes"
        ])
        .output()
        .map_err(|e| format!("Colima 구동 명령어 실패: {}", e))?;

    if output.status.success() {
        Ok("Colima K8s 클러스터가 성공적으로 시작되었습니다.".into())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

```

#### `src-tauri/src/commands/metrics.rs`

macOS 유니파이드 메모리 점유율을 실시간 측정하는 모듈입니다.

```rust
use sysinfo::System;
use serde::Serialize;

#[derive(Serialize)]
pub struct SystemMetrics {
    pub total_memory_gb: f64,
    pub used_memory_gb: f64,
    pub memory_usage_percentage: f32,
}

#[tauri::command]
pub fn get_system_metrics() -> SystemMetrics {
    let mut sys = System::new_all();
    sys.refresh_memory();

    let total = sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let used = sys.used_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let percentage = (used / total * 100.0) as f32;

    SystemMetrics {
        total_memory_gb: (total * 100.0).round() / 100.0,
        used_memory_gb: (used * 100.0).round() / 100.0,
        memory_usage_percentage: (percentage * 10.0).round() / 10.0,
    }
}

```

---

### 3. 프론트엔드 핵심 파일 설계 (React / TypeScript)

#### `src/hooks/useColima.ts`

Tauri IPC를 통해 Rust 백엔드를 비동기 호출하는 React Custom Hook입니다.

```typescript
import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface ColimaStatus {
  is_running: boolean;
  vm_type: string;
  kubernetes_active: boolean;
}

export function useColima() {
  const [status, setStatus] = useState<ColimaStatus | null>(null);
  const [loading, setLoading] = useState<boolean>(false);

  const fetchStatus = async () => {
    try {
      const res = await invoke<ColimaStatus>('get_colima_status');
      setStatus(res);
    } catch (err) {
      console.error("Colima 상태 로드 오류:", err);
    }
  };

  const startCluster = async (cpu: number = 4, memory: number = 12) => {
    setLoading(true);
    try {
      await invoke('start_colima', { cpu, memory });
      await fetchStatus();
    } catch (err) {
      alert(`클러스터 구동 실패: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchStatus();
    const interval = setInterval(fetchStatus, 5000);
    return () => clearInterval(interval);
  }, []);

  return { status, loading, startCluster, refresh: fetchStatus };
}

```

#### `src/components/dashboard/ClusterControl.tsx`

Colima K8s 라이프사이클 및 리소스 상태를 제어하는 메인 컴포넌트 예시입니다.

```tsx
import React from 'react';
import { useColima } from '../../hooks/useColima';

export const ClusterControl: React.FC = () => {
  const { status, loading, startCluster } = useColima();

  return (
    <div className="p-6 bg-slate-900 text-white rounded-xl shadow-md border border-slate-800">
      <h2 className="text-xl font-bold mb-4 flex items-center gap-2">
        <span>⚙️</span> Colima K8s Control
      </h2>
      
      <div className="mb-6">
        <div className="flex items-center gap-2">
          <span className="text-sm text-slate-400">클러스터 상태:</span>
          <span className={`px-2 py-1 text-xs rounded-full font-semibold ${
            status?.is_running ? 'bg-green-500/20 text-green-400' : 'bg-red-500/20 text-red-400'
          }`}>
            {status?.is_running ? 'RUNNING (vz)' : 'STOPPED'}
          </span>
        </div>
      </div>

      <button
        onClick={() => startCluster(6, 12)}
        disabled={loading || status?.is_running}
        className="w-full py-2.5 bg-blue-600 hover:bg-blue-500 disabled:bg-slate-700 text-white font-medium rounded-lg transition-colors"
      >
        {loading ? '클러스터 구동 중...' : 'Apple vz 기반 K8s 스타트 (6 CPU / 12GB RAM)'}
      </button>
    </div>
  );
};

```

---
