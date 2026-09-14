# ADR-0003: Kubernetes 보안 베이스라인 (OpenForge 프로필 `standard`)

- 상태: 채택됨 (Accepted)
- 날짜: 2026-09-10

## 배경 (Context)

이슈 #74는 KubeMetal이 스택의 Linux/Kubernetes 부분에만 OpenForge Kubernetes 보안
베이스라인(프로필 `standard`)을 채택할 것을 요구한다. 이 저장소가 운영되는 하드
불변조건(`AGENTS.md` "Architecture Invariants")은 K8s가 절대 연산을 실행하지 않는다는
것이다 — Colima(`vz`+`virtiofs`) k3s VM은 컨트롤 플레인 서비스(MLflow, SeaweedFS,
Prefect)만 호스팅하고, 모든 MLX/Metal 워크로드는 Rust 백엔드가 직접 스폰하는 macOS
호스트 프로세스다. 범용 Kubernetes 클러스터용으로 작성된 보안 베이스라인은 이 분리에
맞춰 재조정하지 않으면, 연산이 전혀 없는 표면을 강화하면서 실제 연산이 있는 표면은
놓치거나, 이 토폴로지가 실제로 제공할 수 없는 경계를 주장하게 된다.

아래 모든 내용은 2026-09-09/10 실제 가동 중인 colima 클러스터에서 실측했다
(`kubectl config current-context` = `colima`, k3s v1.35.0+k3s1, Ubuntu 24.04.4,
`docker://29.5.2`, `k3s server`는 추가 플래그 없이 구동) — 가정하지 않았다. 이
저장소의 `docs/mistakes-log.md`는 대부분 그 반대(D22–D25)의 사례로 채워져 있다.

## 결정 (Decision)

프로필 `standard`를 k3s 노드 범위로 채택한다. 모든 통제 항목의 판정을 정직하게
기록하며, 토폴로지상 의미가 없는 통제는 있는 척하지 않고 그렇게 기록한다.

### 1. 토폴로지 인벤토리 — Linux 노드 통제 vs macOS 호스트 책임

| 표면 | 소유자 | 보안 책임 |
|---|---|---|
| k3s 노드 (Ubuntu 24.04, colima VM) | Linux/Kubernetes | 파드 수준 하드닝, NetworkPolicy, LSM 강제 — 이 ADR의 범위 |
| MLflow / SeaweedFS / Prefect 파드 | Linux/Kubernetes | 범위 내 — 아래에서 강화 |
| MLX 추론/학습 프로세스 | macOS 호스트 (Rust가 스폰) | 이 ADR의 범위 밖 — 파드로 절대 실행되지 않음. `resolve_cli_path`/`external_command` 샌드박싱(`AGENTS.md` "What bites here")이 담당하며 Kubernetes RBAC/PSA가 아님 |
| 파드→호스트 브리지(`mac-gpu-service`, D10) | 양쪽 (ExternalName → `host.lima.internal`) | 유일한 경계 통과점. 아래 egress 정책은 이를 의도적으로 제한하지 않는다 — 이 스택의 파드(mlflow/seaweedfs/prefect)는 그것을 절대 호출하지 않고, `kagent`만 호출한다(이 작업의 파일 범위 밖) |
| kagent CRD / 에이전트 온리 통합 (D30 L1) | Linux/Kubernetes, 외부 클러스터 | 범위 밖: `security-agent.yaml`은 `scripts/k8s/kustomization.yaml`에서 의도적으로 제외돼 있고 이 베이스라인의 대상이 아니다 |

### 2. 통제 판정

| 통제 | 판정 | 근거 |
|---|---|---|
| 비루트 워크로드 아이덴티티 | 적용됨 | mlflow/seaweedfs/prefect(전부 기본 root 이미지)에 `runAsNonRoot: true`, `runAsUser/runAsGroup/fsGroup: 65532`. 실측: `docker run --user 65532:65532`가 셋 다 성공하고, 실제 파드도 해당 신원으로 Running. |
| Seccomp 기본값 | 적용됨 | 세 배포 모두 파드 수준 `seccompProfile: {type: RuntimeDefault}`. |
| `allowPrivilegeEscalation: false` / `capabilities: drop: [ALL]` | 적용됨 | 컨테이너 4개(mlflow, mlflow의 `ensure-artifact-bucket` init 컨테이너, seaweedfs, prefect) 전부. |
| `readOnlyRootFilesystem` | 적용됨 | 컨테이너 4개 각각 개별 실측(가정 없음): curl init 컨테이너는 쓰기 경로가 필요 없음; mlflow는 `/tmp`가 쓰기 가능해야 함(이미 마운트됨, `HOME=/tmp PYTHONUSERBASE=/tmp/pylibs pip install --user`); seaweedfs는 추가로 gRPC 유닉스 소켓용 `/tmp`가 필요(새 emptyDir 추가); prefect는 `PREFECT_UI_STATIC_DIRECTORY`를 쓰기 가능한 `/tmp` 경로로 돌려야 함 — 이미지 기본 UI 정적 파일 위치가 root 소유 `site-packages` 안이라 rootfs가 쓰기 가능해도 비루트 UID로는 못 쓴다(리드온리 여부와 무관). |
| 네이티브 LSM(AppArmor/SELinux) 보존 | 해당함, 추가 조치 불필요 | k3s 노드의 `/sys/kernel/security/lsm`은 `lockdown,capability,landlock,yama,apparmor`를 보고한다; SELinux 도구는 없음(`getenforce`: not found). 실행 중인 컨테이너에는 **명시적 파드 애너테이션 없이도** Docker 기본 `docker-default` AppArmor 프로파일이 적용된다(`docker inspect --format '{{.AppArmorProfile}}'` → `docker-default`). `appArmorProfile: Unconfined`나 다른 오버라이드를 설정하지 않는다 — 노드의 네이티브 강제를 그대로 둔 것이 여기서 말하는 "보존"이다. |
| Kubernetes 워크로드용 네임스페이스/NetworkPolicy | 해당함 (N/A 아님) | k3s는 임베디드 kube-router netpol 컨트롤러를 탑재한다 — "flannel은 NetworkPolicy를 강제 안 한다"는 통념과 배치된다. 실측 allow/deny 근거는 §5. |
| 내부/외부 노출 분리 | 정직하게 서술함, 하드 경계로 주장하지 않음 | §3 참고 — 이 토폴로지에서 가장 과장되기 쉬운 통제라 있는 그대로 서술한다. |
| 통제된 egress | 가능한 선에서 적용, 한계 1건 명시 | §4 참고. k3s의 L3/L4 전용 kube-router netpol로는 PyPI/CDN egress를 특정 IP로 좁힐 수 없다(FQDN 인식 egress가 없음 — Cilium/Calico가 필요). 대신 mlflow의 클러스터 내부 lateral movement는 전부 막고 외부 HTTPS만 허용한다. "이 정도면 됐다"고 조용히 넘기지 않고 실제 한계를 그대로 기록했다. |
| 아웃바운드 의존성 문서화 | 적용됨 | §4 참고. |
| 연결성/보안 회귀 근거 | 적용됨 | §5 참고; 실제 스택을 실시간으로 재배포해 매 변경 후 세 서비스 모두 `kubectl port-forward`로 접근 가능함을 확인. |

### 3. 노출 모델 — 배포 모드별 정직한 서술

이 저장소의 통합 등급(D30)이 이미 여기서 필요한 경계를 그어 놓았다:

- **이 앱 자신의 k3s(colima) — 이 ADR의 실제 범위.** "내부"와 "외부"를 나눌 네트워크
  경계 자체가 없다 — 접근 경로는 (a) Mac 호스트에서의 `kubectl port-forward`(MLflow/
  SeaweedFS/Prefect UI와 MLflow를 호출하는 MLX 호스트 프로세스가 사용)와 (b) 단일
  노드 클러스터 내부의 파드 간 트래픽 둘뿐이다. 실측(§5) 결과 `kubectl port-forward`와
  kubelet readiness probe 둘 다 kube-router netpol 강제 경로를 완전히 우회한다(API
  서버 → kubelet 스트림 경로를 타며 CNI iptables 체인을 전혀 거치지 않는다) — 그래서
  여기서는 NetworkPolicy로 "내부 대 외부"를 나누는 것 자체가 의미가 없다. 실제로
  의미 있고 실제로 구축한 것은 파드 간 lateral movement 통제다: mlflow는 오직
  seaweedfs:8333만 도달 가능하고, seaweedfs는 mlflow에서만 도달 가능하며, prefect는
  내부의 아무것도 호출하지 않는다. 그 위에 "외부 노출 경계"를 주장하는 것은 토폴로지가
  실제로 제공하는 것을 왜곡하는 일이다 — 이 스택은 인그레스 컨트롤러도, LoadBalancer도,
  노출된 NodePort도 없으므로 "외부" 트래픽이라는 것 자체가 이 베이스라인이 막아야 할
  대상이 아니다.
- **L1 에이전트 온리 외부 클러스터(D30 기본값).** 이 베이스라인의 대상인 풀스택 파드가
  애초에 그곳에 배포되지 않는다 — 나눌 것이 없다.
- **L2 옵트인 풀스택 외부 배포(D30, D26 `render.sh`).** 노출 모델은 대상 클러스터마다
  재검증이 필요하다(여기서 실측한 kube-router netpol 동작은 k3s 임베디드 컨트롤러에
  특화된 것이고, Calico/Cilium 클러스터는 포트포워드를 같은 방식으로 예외 처리하지
  않을 수 있다). 이 ADR의 범위 밖 — L2에 대해서는 검증되지 않았다고 명시하며, 위
  colima 결과가 그대로 성립한다고 가정하지 않는다.

### 4. 아웃바운드 의존성

| 의존성 | 호출자 | 이유 | Egress 통제 |
|---|---|---|---|
| PyPI / files.pythonhosted.org (HTTPS) | mlflow 컨테이너, 매 기동 시 | `ghcr.io/mlflow/mlflow:v3.14.0`에는 `boto3`가 없다; S3 아티팩트 스토어에 필요하다(`mlflow-deployment.yaml` 기존 주석) | 허용: 443 포트에 한해 `ipBlock 0.0.0.0/0 except {10.42.0.0/16(파드 CIDR), 10.43.0.0/16(서비스 CIDR)}`. kube-router의 L3/L4 전용 netpol(§2)이 주는 실질적 한계다 — PyPI의 실제 IP(Fastly CDN, 고정 대역 아님)로 더 좁히려면 이 클러스터에 없는 FQDN 인식 CNI가 필요하다. |
| DNS (kube-dns, `kube-system`) | mlflow, prefect | 서비스명 해석(`seaweedfs` 등) | 허용, 53 UDP/TCP, 네임스페이스+파드 셀렉터로 `kube-dns` 파드에 한정. |
| Prefect 익명 텔레메트리 | prefect 컨테이너 | Prefect 내장 `prefect.server.services.telemetry` 백그라운드 서비스 | **의도적으로 차단** — prefect의 egress 정책은 DNS만 허용한다. 실측: 실행 중인 파드 로그에 `prefect.server.services.telemetry - Failed to send telemetry: All connection attempts failed`가 찍히고, 서버 자체는 정상(`Ready` 영향 없음). 이 통신이 성공해야 하는 기능적 의존성은 없다. |
| `mac-gpu-service` → `host.lima.internal` | 이 스택 아님 | `kagent` 파드만 호출한다; mlflow/seaweedfs/prefect는 절대 호출하지 않는다(`scripts/`와 `src-tauri/` 전체에서 `grep -rn mac-gpu-service`로 확인) | 이 베이스라인의 NetworkPolicy가 다루지 않음 — 파일 범위 밖(kagent 매니페스트는 `scripts/k8s/kustomization.yaml`에서 제외됨) |

### 5. NetworkPolicy 설계와 실측 근거

네임스페이스 전체가 아니라 앱별 기본 차단: colima의 `default` 네임스페이스에는 이
작업의 파일 범위가 `kustomization.yaml`에서 명시적으로 제외한 워크로드
(`e2e-remediated-nginx.yaml`, `remote-reader/`)가 이미 함께 돌고 있다
(`e2e-broken-nginx`, `remote-reader-tools`). 네임스페이스 전체 `podSelector: {}` 기본
차단을 걸었다면 이들을 조용히 끊었을 것이다 — 이 작업이 허가받은 변경 반경 밖이다.
`mlflow-netpol` / `seaweedfs-netpol` / `prefect-netpol`(`scripts/k8s/network-policies.yaml`)
각각은 대신 자기 `app` 라벨만 선택해, 네임스페이스의 다른 파드는 그대로 둔다.

실제 colima 클러스터(시뮬레이션 아님)에 대한 실측 근거, 2026-09-09/10:

- **차단, 무관한 파드 → mlflow(어떤 ingress 정책도 허용하지 않음):** 일회용
  `curlimages/curl` 파드의 `wget http://mlflow:5000/` → connection refused(차단 확인).
- **차단, 무관한 파드 → seaweedfs(`app=mlflow`만 허용):** 같은 일회용 파드 →
  `wget http://seaweedfs:8333/` → connection refused.
- **허용, mlflow → seaweedfs:8333(이 스택에 필요한 유일한 클러스터 내부 경로):**
  실행 중인 mlflow 파드에 `kubectl exec`,
  `python3 -c "urllib.request.urlopen('http://seaweedfs:8333/')"` → HTTP 200.
- **차단, mlflow → prefect:4200(mlflow의 egress 허용 목록에 없음):** 같은 exec →
  `Connection refused`.
- **`kubectl port-forward`는 NetworkPolicy를 완전히 우회한다:** mlflow 파드에 임시로
  `ingress: []` 정책을 걸어도 `kubectl port-forward deploy/mlflow 15000:5000`이 여전히
  HTTP 200을 반환 — 이 클러스터 netpol 구현의 특성(API 서버가 중계하는 스트림이지
  CNI 경로 트래픽이 아님)임을 확인했다, 정책의 구멍이 아니라. 실제(임시가 아닌)
  정책으로 재확인: 전체 스택 재배포 후 mlflow/seaweedfs/prefect 모두 `kubectl
  port-forward`로 도달 가능했다(§6).
- **kubelet readiness probe도 NetworkPolicy를 우회한다:** `/api/health`에
  readinessProbe가 있는 prefect 파드에 임시로 `ingress: []`를 걸어도 20초 관찰
  구간 내내 `Ready: True`를 유지했다.

### 6. 전체 스택 재배포 근거

실제 colima 클러스터에 `kubectl apply -k scripts/k8s/`로 적용했다(사전에
`kubectl apply --dry-run=server -k scripts/k8s/` 통과 확인). mlflow와 seaweedfs는
깔끔하게 롤아웃됐다. Prefect는 처음에 크래시루프에 빠졌다:
`sqlite3.OperationalError: attempt to write a readonly database` — 근본 원인은
`readOnlyRootFilesystem`이 **아니라**, RWO `prefect-data` PVC의 기존 내용
(`prefect.db`, 이 베이스라인 이전부터 `root:root` 소유)과 쿠버네티스 kubelet의 동작
방식이 겹친 것이었다: `fsGroup`은 파드 하나가 볼륨을 새로 마운트할 때만 (재)적용되고,
기본 `RollingUpdate`가 같은 RWO 클레임에 대해 잠깐 파드 두 개를 동시에 띄우면 기존
파일의 소유권을 소급 재적용하지 않는다(실측된 kubelet 경고: `AlreadyMountedVolume ...
GID 0`). 다음 두 가지로 해결했다: (a) prefect Deployment를
`strategy: {type: Recreate}`로 바꿔 옛 파드가 PVC를 완전히 반납한 뒤 새 파드가
마운트하게 함(단일 레플리카·배타적 PVC 조합이라 가용성 손실 없음), (b) 노드의 PV
hostPath 디렉터리 기존 내용을 `chown -R 65532:65532`로 일회성 조정 — 이 내용이 이
베이스라인 이전부터 있었고, 이미 채워진 볼륨의 소유권을 소급 정정해 주는 자동
리컨실리에이션은 없기 때문에 필요했다. 두 조치 후 `kubectl rollout status`가 세
배포 모두 성공했고, 세 UI/API 모두 `kubectl port-forward`로 HTTP 200을 반환했다.
`make verify`도 통과한다(Rust 테스트 119/119, clippy, tsc, design lint, web build) —
매니페스트 목록을 하드코딩하고 있던
`src-tauri/src/commands/provision.rs`의
`kustomization_keeps_d13_secret_first_order` 테스트가 새 `network-policies.yaml`
항목을 몰랐던 것을 갱신한 뒤다.

## 결과 (Consequences)

- 이 스택 안에서 파드 하나가 뚫려도 더 이상 같은 네임스페이스의 다른 파드로 손쉽게
  옆으로 이동하거나, setuid/capability로 권한을 상승시키거나, 각 서비스가 실제로
  필요하다고 실측된 특정 쓰기 경로 밖의 read-only 컨테이너 파일시스템을 변경할 수
  없다.
- 의도적으로 받아들인 한계 하나는 PyPI egress가 FQDN 기반이 아니라 IP 대역 기반이라는
  점이다 — k3s의 임베디드 netpol 컨트롤러에 FQDN 인식이 없기 때문이다. 더 좁히려면
  Cilium이나 Calico 도입이 필요한데, 이는 이 작업 범위 밖이며 이미 해결된 것처럼
  조용히 넘길 사안이 아니다.
- 기존 root 소유 내용이 있는 PVC에 `fsGroup`을 소급 적용하려면 노드에서 수동 일회성
  `chown`이 필요했다 — 앞으로 이 베이스라인 하에 kustomization에 추가되는 상태 저장
  워크로드는 PVC를 처음 마운트할 때부터 하드닝된 `securityContext`로 시작해 이 반복을
  피해야 한다.
- L2(옵트인 풀스택 외부 배포)의 노출/netpol 동작은 이 ADR이 명시적으로 검증하지
  않았으며, 위 colima 결과와 같다고 가정해서는 안 된다.

## 관련 문서

- D38(`docs/03-mvp-design.md` §4)이 프로필 선택과 이 ADR에 대한 포인터를 기록한다.
- `AGENTS.md` "Architecture Invariants"에 이 베이스라인을 언급하는 항목 1개가
  추가된다.
