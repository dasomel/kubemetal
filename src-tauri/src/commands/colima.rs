use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::services::process::{external_command, resolve_bundled_resource};

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
    pub artifact_store_wired: bool,
}

#[tauri::command]
pub async fn get_cluster_status() -> Result<ClusterStatus, String> {
    let output = external_command("colima")?
        .args(["status", "--json"])
        .output()
        .await
        .map_err(|e| format!("colima execution failed: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
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
            artifact_store_wired: false,
        });
    };

    let is_running = true; // exit 0 + JSON 출력 = 기동 중 (미기동은 위에서 조기 반환)
    let kubernetes_active = raw.kubernetes;

    let (mlflow_ready, seaweedfs_ready, artifact_store_wired) = if kubernetes_active {
        // 이 함수는 colima 수명주기 상태(`colima status`)를 다루므로 컨텍스트도 colima가
        // 맞다. 다만 네임스페이스는 활성 대상을 따른다 — colima를 쓰면서 전용 ns에
        // 배포한 경우까지 default로 조회하면 스택이 없다고 오판한다.
        let (_, namespace) = crate::services::deploy_target::active_context();
        let deploy_out = external_command("kubectl")?
            .args(["--context", "colima", "get", "deploy", "-n", &namespace, "-o", "json"])
            .output()
            .await
            .map_err(|e| format!("kubectl get deploy failed: {e}"))?;
        let json: serde_json::Value =
            serde_json::from_slice(&deploy_out.stdout).unwrap_or(serde_json::json!({"items": []}));
        let items = json["items"].as_array().cloned().unwrap_or_default();
        let is_ready = |name: &str| {
            items.iter().any(|d| {
                d["metadata"]["name"].as_str() == Some(name)
                    && d["status"]["availableReplicas"].as_u64().unwrap_or(0) > 0
            })
        };
        // mlflow Deployment 컨테이너 env에 MLFLOW_S3_ENDPOINT_URL이 있으면
        // SeaweedFS(S3) 아티팩트 스토어가 연동된 것으로 판정 — 추가 kubectl 호출 없이
        // 위에서 이미 가져온 deploy JSON을 재사용한다.
        let artifact_store_wired = items.iter().any(|d| {
            d["metadata"]["name"].as_str() == Some("mlflow")
                && d["spec"]["template"]["spec"]["containers"]
                    .as_array()
                    .map(|containers| {
                        containers.iter().any(|c| {
                            c["env"]
                                .as_array()
                                .map(|envs| {
                                    envs.iter().any(|e| {
                                        e["name"].as_str() == Some("MLFLOW_S3_ENDPOINT_URL")
                                    })
                                })
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
        });
        (is_ready("mlflow"), is_ready("seaweedfs"), artifact_store_wired)
    } else {
        (false, false, false)
    };

    Ok(ClusterStatus {
        is_running,
        kubernetes_active,
        mlflow_ready,
        seaweedfs_ready,
        artifact_store_wired,
    })
}

#[tauri::command]
pub async fn start_cluster(cpu: u32, memory: u32) -> Result<String, String> {
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

    let output = external_command("colima")?
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
        .map_err(|e| format!("colima start execution failed: {e}"))?;

    if output.status.success() {
        Ok("Colima K8s cluster started.".into())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

#[tauri::command]
pub async fn stop_cluster() -> Result<String, String> {
    let output = external_command("colima")?
        .arg("stop")
        .output()
        .await
        .map_err(|e| format!("colima stop execution failed: {e}"))?;

    if output.status.success() {
        Ok("Colima cluster stopped.".into())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// kubeconfig에 실제로 등록된 컨텍스트만 반환한다. 조회 실패는 에러로 올린다 —
/// 존재하지 않는 컨텍스트를 폴백으로 지어내면 UI가 없는 클러스터를 있는 것처럼 표시한다.
#[tauri::command]
pub async fn list_kubeconfig_contexts() -> Result<Vec<String>, String> {
    let output = external_command("kubectl")?
        .args(["config", "get-contexts", "-o", "name"])
        .output()
        .await
        .map_err(|e| format!("kubectl config get-contexts failed: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "kubectl config get-contexts failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

#[derive(Debug, Serialize)]
pub struct AirgapAssetItem {
    pub category: String,
    pub name: String,
    pub version: String,
    pub file_name: String,
    pub exists: bool,
    pub size_mb: f64,
    /// 손상 파일은 MB로 반올림하면 실제 크기가 0으로 뭉개진다 — 정확한 바이트를 함께 준다.
    pub size_bytes: u64,
    /// 파일은 있는데 유효한 산출물로 보기엔 너무 작은 경우. 실기기에서 발견(2026-07-25):
    /// 구버전 다운로더의 `curl ... || true`가 남긴 9바이트 `Not Found` 본문이
    /// `binaries/kubescape`로 저장돼 UI가 "보유 (0 MB)"로 보고하고 있었다.
    pub corrupt: bool,
}

/// 이보다 작은 파일은 바이너리·차트(.tgz)·이미지 아카이브(.tar.gz) 어느 쪽으로도 유효할 수
/// 없다. 자산별 실제 크기를 추정하는 대신, 어떤 산출물에도 적용되는 하한만 둔다.
const MIN_VALID_ASSET_BYTES: u64 = 1024;

#[derive(Debug, Serialize)]
pub struct AirgapStatusReport {
    pub airgap_dir: String,
    pub total_assets_count: usize,
    pub downloaded_count: usize,
    pub total_size_mb: f64,
    pub assets: Vec<AirgapAssetItem>,
}

/// 매니페스트에 선언되지 않는 자산 — Helm 차트가 배포하는 이미지, 바이너리, 차트 자체.
/// 형식: (category, 표시 이름, 버전, 번들 내 상대경로)
const STATIC_AIRGAP_TARGETS: [(&str, &str, &str, &str); 11] = [
    ("Binary", "K3s Kubernetes Engine", "v1.28.2 (arm64)", "binaries/k3s"),
    ("Binary", "Kubescape Security CLI", "v3.0.0", "binaries/kubescape"),
    // CRD 차트는 본 차트의 선행 조건이다(D33 개정 2) — 이것 없이는 폐쇄망 최초 설치가
    // helm 렌더 단계에서 죽는다. 번들에 빠져 있으면 상태 화면이 "완비"라고 말하게 된다.
    ("Helm Chart", "kagent CRD Helm Chart", "0.9.12", "charts/kagent-crds-0.9.12.tgz"),
    ("Helm Chart", "kagent Helm Chart", "0.9.12", "charts/kagent-0.9.12.tgz"),
    ("Container Image", "kagent Controller Image", "0.9.12", "images/cr.kagent.dev_kagent-dev_kagent_controller_0.9.12.tar.gz"),
    ("Container Image", "kagent Declarative App Image", "0.9.12", "images/cr.kagent.dev_kagent-dev_kagent_app_0.9.12.tar.gz"),
    ("Container Image", "kagent UI Dashboard Image", "0.9.12", "images/cr.kagent.dev_kagent-dev_kagent_ui_0.9.12.tar.gz"),
    ("Container Image", "kagent Tools Server Image", "0.2.1", "images/ghcr.io_kagent-dev_kagent_tools_0.2.1.tar.gz"),
    ("Container Image", "kmcp Controller Image", "0.3.0", "images/ghcr.io_kagent-dev_kmcp_controller_0.3.0.tar.gz"),
    // kagent이 요구하는 Postgres. 다운로더는 받아왔지만 이 목록에 없어 상태 화면이 존재를
    // 검사하지 않던 자산이다 — static_airgap_targets_match_images_helm_txt가 잡아냈다.
    ("Container Image", "kagent Postgres Database", "18.3-alpine", "images/postgres_18.3-alpine.tar.gz"),
    // 태그를 고정한다(이슈 #5) — `latest`는 수집 시점마다 다른 내용을 받아오므로
    // "번들은 불변"이라는 전제가 성립하지 않는다. 저장소 전체에서 유일한 `latest`였다.
    ("Container Image", "Trivy Vulnerability Scanner", "0.69.3", "images/aquasec_trivy_0.69.3.tar.gz"),
];

/// `docker save`가 만든 파일명 규칙 — 다운로더의 `tr '/:' '_'`와 동일해야 한다.
fn image_archive_name(image: &str) -> String {
    let safe: String = image
        .chars()
        .map(|c| if c == '/' || c == ':' { '_' } else { c })
        .collect();
    format!("images/{safe}.tar.gz")
}

/// K8s 매니페스트의 `image:` 라인에서 (이미지 전체, 태그)를 뽑는다. 번들에 무엇이 있어야
/// 하는지의 **단일 출처는 매니페스트**다 — Rust와 셸에 목록을 따로 적어두면 매니페스트가
/// 올라갈 때 조용히 어긋난다(실측 2026-07-25: mlflow/seaweedfs가 구버전으로 굳고
/// prefect/curl은 누락돼 폐쇄망 설치가 ImagePullBackOff로 깨지는 상태였다).
fn images_from_manifests(manifest_dir: &std::path::Path) -> Vec<String> {
    let mut images = Vec::new();
    let Ok(entries) = std::fs::read_dir(manifest_dir) else {
        return images;
    };
    let mut files: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("yaml"))
        .collect();
    files.sort();

    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in text.lines() {
            let trimmed = line.trim_start();
            // 주석과 `imagePullPolicy:` 같은 유사 키를 배제하기 위해 정확히 `image:`만 본다.
            if trimmed.starts_with('#') {
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("image:") {
                let image = rest.trim().trim_matches('"').trim_matches('\'');
                if !image.is_empty() && !images.iter().any(|i| i == image) {
                    images.push(image.to_string());
                }
            }
        }
    }
    images
}

#[tauri::command]
pub async fn get_airgap_status(app: tauri::AppHandle) -> Result<AirgapStatusReport, String> {
    let home_str = std::env::var("HOME").map_err(|_| "HOME environment variable not found.".to_string())?;
    let airgap_dir = std::path::PathBuf::from(home_str).join(".kubemetal").join("airgap");

    let mut targets: Vec<(String, String, String, String)> = STATIC_AIRGAP_TARGETS
        .iter()
        .map(|(c, n, v, p)| (c.to_string(), n.to_string(), v.to_string(), p.to_string()))
        .collect();

    // 매니페스트 유래 이미지를 덧붙인다. 번들에 동봉된 매니페스트를 먼저 보고(설치 대상과
    // 정확히 일치), 없으면 앱에 동봉된 리소스를 본다.
    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let bundled_manifests = airgap_dir.join("manifests");
    let manifest_dir = if bundled_manifests.is_dir() {
        bundled_manifests
    } else {
        resolve_bundled_resource(&resource_dir, "scripts/k8s")
    };

    for image in images_from_manifests(&manifest_dir) {
        let version = image.rsplit(':').next().unwrap_or("latest").to_string();
        let name = image.rsplit('/').next().unwrap_or(&image).to_string();
        let rel_path = image_archive_name(&image);
        if targets.iter().any(|(_, _, _, p)| *p == rel_path) {
            continue;
        }
        targets.push(("Container Image".into(), name, version, rel_path));
    }

    let mut assets = Vec::new();
    let mut downloaded_count = 0;
    let mut total_size_mb = 0.0;

    for (cat, name, ver, rel_path) in &targets {
        let full_path = airgap_dir.join(rel_path);
        // .tar.gz 우선 확인, 없으면 비압축 .tar 폴백 확인
        let target_file = if full_path.exists() {
            Some(full_path)
        } else {
            let fallback_path = airgap_dir.join(rel_path.trim_end_matches(".gz"));
            if fallback_path.exists() {
                Some(fallback_path)
            } else {
                None
            }
        };

        // 파일 존재만으로 "보유"로 세지 않는다 — 크기 하한을 못 넘기면 손상으로 분류하고
        // 보유 수·총 용량에서도 제외한다(부분 수신 파일을 완료로 보고하지 않기 위함).
        let (exists, corrupt, size_mb, size_bytes) = match target_file {
            Some(p) => {
                let bytes = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                let mb = (bytes as f64) / 1024.0 / 1024.0;
                if bytes >= MIN_VALID_ASSET_BYTES {
                    downloaded_count += 1;
                    total_size_mb += mb;
                    (true, false, (mb * 100.0).round() / 100.0, bytes)
                } else {
                    (false, true, (mb * 100.0).round() / 100.0, bytes)
                }
            }
            None => (false, false, 0.0, 0),
        };

        assets.push(AirgapAssetItem {
            category: cat.clone(),
            name: name.clone(),
            version: ver.clone(),
            file_name: rel_path.clone(),
            exists,
            size_mb,
            size_bytes,
            corrupt,
        });
    }

    let total_assets_count = assets.len();

    Ok(AirgapStatusReport {
        airgap_dir: airgap_dir.to_string_lossy().to_string(),
        total_assets_count,
        downloaded_count,
        total_size_mb: (total_size_mb * 100.0).round() / 100.0,
        assets,
    })
}

/// Air-Gap 스크립트는 `bundle.resources`로 동봉된다. 상대경로(`scripts/airgap/...`)는
/// `.app` 실행 시 CWD가 프로젝트 루트가 아니므로 항상 실패한다 — resource_dir 기준으로
/// 해석해야 dev/번들 양쪽에서 동작한다(D5·`resolve_bundled_resource`와 동일 규약).
async fn run_airgap_script(
    app: &tauri::AppHandle,
    relative: &str,
    label: &str,
) -> Result<String, String> {
    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let script_path = resolve_bundled_resource(&resource_dir, relative);
    if !script_path.is_file() {
        return Err(format!(
            "{label} script not found: {}",
            script_path.display()
        ));
    }

    let output = external_command("bash")?
        .arg(&script_path)
        .output()
        .await
        .map_err(|e| format!("{label} script execution failed: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "{label} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    // 스크립트가 실제로 출력한 마지막 줄을 그대로 돌려준다(성공 문구를 지어내지 않는다).
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("(no output)")
        .trim()
        .to_string())
}

#[tauri::command]
pub async fn trigger_airgap_download(app: tauri::AppHandle) -> Result<String, String> {
    run_airgap_script(
        &app,
        "scripts/airgap/download_airgap_bundle.sh",
        "Air-Gap bundle download",
    )
    .await
}

#[tauri::command]
pub async fn trigger_airgap_install(app: tauri::AppHandle) -> Result<String, String> {
    run_airgap_script(
        &app,
        "scripts/airgap/install_from_airgap.sh",
        "Air-Gap offline install",
    )
    .await
}

#[derive(Debug, Serialize)]
pub struct AirgapLatestVersionReport {
    pub name: String,
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
}

#[tauri::command]
pub async fn check_latest_airgap_versions() -> Result<Vec<AirgapLatestVersionReport>, String> {
    /// `v1.28.2+k3s1` → `1.28.2`. 업스트림 태그의 접두 v와 빌드 메타데이터를 걷어내
    /// 보유 버전과 같은 축으로 맞춘다(이 정규화 없이는 k3s가 상시 "업데이트 있음"이 된다).
    fn normalize(tag: &str) -> String {
        tag.trim()
            .trim_start_matches('v')
            .split('+')
            .next()
            .unwrap_or("")
            .to_string()
    }

    // 조회에 실패하면 "최신"이라고 단정하지 않고 실패 사실을 그대로 표시한다.
    const UNKNOWN: &str = "lookup failed";

    let check_repo = |owner: &'static str, repo: &'static str, cur_ver: &'static str| async move {
        let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
        if let Ok(mut cmd) = external_command("curl") {
            cmd.args(["-sS", "-m", "6", "-H", "User-Agent: KubeMetal", &url]);
            if let Ok(out) = cmd.output().await {
                if out.status.success() {
                    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                        if let Some(tag) = json["tag_name"].as_str() {
                            let latest = normalize(tag);
                            let has_update = !latest.is_empty() && latest != normalize(cur_ver);
                            return (latest, has_update);
                        }
                    }
                }
            }
        }
        (UNKNOWN.to_string(), false)
    };

    let with_v = |ver: String| {
        if ver == UNKNOWN {
            ver
        } else {
            format!("v{ver}")
        }
    };

    let (kagent_latest, kagent_up) = check_repo("kagent-dev", "kagent", "0.9.12").await;
    let (k3s_latest, k3s_up) = check_repo("k3s-io", "k3s", "1.28.2").await;
    let (kubescape_latest, ks_up) = check_repo("kubescape", "kubescape", "3.0.0").await;

    Ok(vec![
        AirgapLatestVersionReport {
            name: "kagent Helm & Controller".into(),
            current_version: "0.9.12".into(),
            latest_version: kagent_latest,
            has_update: kagent_up,
        },
        AirgapLatestVersionReport {
            name: "K3s Kubernetes Engine".into(),
            current_version: "v1.28.2".into(),
            latest_version: with_v(k3s_latest),
            has_update: k3s_up,
        },
        AirgapLatestVersionReport {
            name: "Kubescape Security CLI".into(),
            current_version: "v3.0.0".into(),
            latest_version: with_v(kubescape_latest),
            has_update: ks_up,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_k8s_dir() -> std::path::PathBuf {
        // 테스트 cwd는 src-tauri/ — 매니페스트는 리포 루트 아래에 있다.
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .join("scripts/k8s")
    }

    #[test]
    fn image_archive_name_matches_downloader_convention() {
        // 다운로더의 `echo "$img" | tr '/:' '_'`와 동일해야 한다.
        assert_eq!(
            image_archive_name("ghcr.io/mlflow/mlflow:v3.14.0"),
            "images/ghcr.io_mlflow_mlflow_v3.14.0.tar.gz"
        );
        assert_eq!(image_archive_name("nginx:alpine"), "images/nginx_alpine.tar.gz");
    }

    #[test]
    fn images_from_manifests_reads_real_manifests() {
        let images = images_from_manifests(&repo_k8s_dir());
        assert!(
            !images.is_empty(),
            "read zero images from scripts/k8s — parsing is broken"
        );
        // 태그 없는 항목은 폐쇄망에서 latest를 끌어오므로 있으면 안 된다.
        for image in &images {
            assert!(
                image.contains(':'),
                "{image}: no tag — cannot reproduce in an air-gapped environment"
            );
        }
    }

    fn repo_images_helm_txt() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .join("scripts/airgap/images-helm.txt")
    }

    /// `scripts/airgap/images-helm.txt`의 파싱 규칙(줄 끝 `#` 주석 제거 + 모든 공백 제거)은
    /// `scripts/airgap/lib.sh`의 `read_image_list`와 같아야 한다.
    fn images_helm_txt_entries() -> Vec<String> {
        let text = std::fs::read_to_string(repo_images_helm_txt()).expect("read images-helm.txt");
        text.lines()
            .map(|line| {
                let mut s = line.split('#').next().unwrap_or("").to_string();
                s.retain(|c| !c.is_whitespace());
                s
            })
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// `images-helm.txt`는 폐쇄망 번들의 비매니페스트 이미지에 대한 단일 출처지만, 그 파일을
    /// 읽는 것은 셸 스크립트 둘(수집·검증)뿐이고 여기 `STATIC_AIRGAP_TARGETS`는 같은 사실을
    /// 따로 들고 있다. 상태 조회를 런타임 파일 존재에 묶지 않으려는 의도적 중복이므로,
    /// CLAUDE.md가 요구하는 "파생할 수 없으면 어긋남에 실패하는 테스트"를 대신 둔다.
    /// 이 테스트가 깨지면 한쪽만 버전을 올렸다는 뜻이다(D23이 mlflow·seaweedfs로 겪은 그 실패).
    #[test]
    fn static_airgap_targets_match_images_helm_txt() {
        let mut expected: Vec<String> = images_helm_txt_entries()
            .iter()
            .map(|image| image_archive_name(image))
            .collect();
        expected.sort();

        let mut actual: Vec<String> = STATIC_AIRGAP_TARGETS
            .iter()
            .filter(|(category, ..)| *category == "Container Image")
            .map(|(.., path)| (*path).to_string())
            .collect();
        actual.sort();

        assert_eq!(
            expected, actual,
            "images-helm.txt and STATIC_AIRGAP_TARGETS diverged — if only one side is \
             updated, the bundle gets the new version while the Air-Gap status screen \
             looks for the old one"
        );
    }

    /// 이 테스트가 깨지면 `imagePullPolicy` 같은 유사 키를 이미지로 잘못 읽고 있다는 뜻이다.
    #[test]
    fn images_from_manifests_ignores_pull_policy_lines() {
        let images = images_from_manifests(&repo_k8s_dir());
        for image in &images {
            assert!(
                !image.contains("Always") && !image.contains("IfNotPresent"),
                "{image}: read an imagePullPolicy value as an image"
            );
        }
    }
}
