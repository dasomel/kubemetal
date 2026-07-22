#!/usr/bin/env python3
"""KubeMetal MLflow REST 리포터(urllib, mlflow 패키지 미설치 가정) — 공용 모듈.

원래 `finetune_wrapper.py`에 인라인으로 있던 `MlflowReporter`를 여기로 옮겨
`host_runner.py`의 `evaluate_flow`(Phase 4b, experiment "kubemetal-eval")와
`finetune_wrapper.py`(experiment "kubemetal-finetune")가 함께 재사용한다.
experiment 이름과 "경고를 어떻게 내보낼지"는 호출자마다 다르므로(전자는 Prefect
로거, 후자는 러스트가 읽는 stdout JSON 이벤트) 둘 다 생성자 인자로 주입받는다.

MLflow 접근 실패는 호출자의 본 작업(학습/평가)을 막지 않고 warn 콜백만 호출한다.
"""
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Callable, Optional


class MlflowReporter:
    """MLflow REST(urllib) 클라이언트. 접근 실패는 warn 콜백만 호출하고 계속한다."""

    def __init__(
        self,
        base_uri: str,
        experiment_name: str,
        warn: Callable[[str], None] = lambda _message: None,
    ):
        self.base_uri = base_uri.rstrip("/")
        self.experiment_name = experiment_name
        self._warn_fn = warn
        self.run_id: Optional[str] = None
        self.enabled = True

    def _request(self, method: str, path: str, body: Optional[dict] = None,
                 query: Optional[str] = None):
        url = f"{self.base_uri}{path}"
        if query:
            url = f"{url}?{query}"
        import json

        data = json.dumps(body).encode("utf-8") if body is not None else None
        req = urllib.request.Request(
            url, data=data, headers={"Content-Type": "application/json"}, method=method
        )
        with urllib.request.urlopen(req, timeout=5) as resp:
            return json.loads(resp.read().decode("utf-8"))

    def _warn(self, message: str) -> None:
        self._warn_fn(f"MLflow: {message}")

    def get_or_create_experiment(self) -> Optional[str]:
        try:
            resp = self._request(
                "GET",
                "/api/2.0/mlflow/experiments/get-by-name",
                query=f"experiment_name={urllib.parse.quote(self.experiment_name)}",
            )
            return resp["experiment"]["experiment_id"]
        except urllib.error.HTTPError as e:
            if e.code != 404:
                self._warn(f"experiment 조회 실패({e})")
                self.enabled = False
                return None
        except Exception as e:  # noqa: BLE001 - 외부 서비스 실패는 작업을 막지 않는다
            self._warn(f"experiment 조회 실패({e})")
            self.enabled = False
            return None

        try:
            resp = self._request(
                "POST", "/api/2.0/mlflow/experiments/create", {"name": self.experiment_name}
            )
            return resp["experiment_id"]
        except Exception as e:  # noqa: BLE001
            self._warn(f"experiment 생성 실패({e})")
            self.enabled = False
            return None

    def start_run(self, experiment_id: Optional[str], params: dict) -> None:
        if not self.enabled or experiment_id is None:
            return
        try:
            resp = self._request(
                "POST",
                "/api/2.0/mlflow/runs/create",
                {"experiment_id": experiment_id, "start_time": int(time.time() * 1000)},
            )
            self.run_id = resp["run"]["info"]["run_id"]
            self._request(
                "POST",
                "/api/2.0/mlflow/runs/log-batch",
                {
                    "run_id": self.run_id,
                    "params": [{"key": k, "value": str(v)} for k, v in params.items()],
                },
            )
        except Exception as e:  # noqa: BLE001
            self._warn(f"run 생성 실패({e})")
            self.enabled = False

    def log_metric(self, key: str, value: float, step: int = 0) -> None:
        if not self.enabled or self.run_id is None:
            return
        try:
            self._request(
                "POST",
                "/api/2.0/mlflow/runs/log-batch",
                {
                    "run_id": self.run_id,
                    "metrics": [
                        {
                            "key": key,
                            "value": value,
                            "timestamp": int(time.time() * 1000),
                            "step": step,
                        }
                    ],
                },
            )
        except Exception as e:  # noqa: BLE001
            self._warn(f"메트릭 기록 실패({e})")

    def log_metrics(self, metrics: dict, step: int = 0) -> None:
        """`log_metric`을 여러 건 한 번의 log-batch 요청으로 보낸다(N회 왕복 대신 1회)."""
        if not self.enabled or self.run_id is None:
            return
        try:
            self._request(
                "POST",
                "/api/2.0/mlflow/runs/log-batch",
                {
                    "run_id": self.run_id,
                    "metrics": [
                        {
                            "key": key,
                            "value": value,
                            "timestamp": int(time.time() * 1000),
                            "step": step,
                        }
                        for key, value in metrics.items()
                    ],
                },
            )
        except Exception as e:  # noqa: BLE001
            self._warn(f"메트릭 기록 실패({e})")

    def end_run(self, status: str) -> None:
        if not self.enabled or self.run_id is None:
            return
        try:
            self._request(
                "POST",
                "/api/2.0/mlflow/runs/update",
                {"run_id": self.run_id, "status": status, "end_time": int(time.time() * 1000)},
            )
        except Exception as e:  # noqa: BLE001
            self._warn(f"run 종료 실패({e})")
