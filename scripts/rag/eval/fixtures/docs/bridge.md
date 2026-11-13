# Mac bridge / 맥 브릿지
D10 브릿지는 ExternalName mac-gpu-service를 host.lima.internal로 연결한다.
ML computation runs on the macOS host. The Linux VM cannot use the Metal GPU.
클러스터는 제어하고 맥 호스트는 학습한다. ExternalName에는 IP 대신 DNS 이름을 쓴다.
