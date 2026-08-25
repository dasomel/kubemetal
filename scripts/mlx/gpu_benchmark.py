#!/usr/bin/env python3
"""KubeMetal Apple Silicon GPU matmul 벤치마크 (이슈 #11 축소 스코프).

`mlx.core.matmul`로 고정 크기 float32 정방행렬 두 개를 반복 곱해 실측 GFLOPS를 낸다.
storage/cache I/O 벤치마크나 범용 accelerator evidence 스키마는 스코프 밖이다 — 여기서는
MLX matmul 처리량 한 가지만 측정한다.

측정 함정(MLX는 lazy evaluation, 실측): `mx.matmul()` 호출은 즉시 계산하지 않고 연산
그래프만 쌓는다. 루프 안에서 `mx.eval()`로 매번 강제 동기화하지 않으면 파이썬 쪽
`time.perf_counter()`는 그래프 구축 시간만 재고, 실제 GPU 연산은 다음 접근(예: 마지막
결과를 읽는 시점) 때 한꺼번에 일어나 버린다 — 그러면 "측정한 시간"과 "실제 계산이
일어난 시간"이 어긋난다. 그래서 워밍업 단계와 매 반복 모두 `mx.eval()`로 동기화한다.

출력: stdout에 JSON 한 줄
    {"gflops": ..., "elapsed_seconds": ..., "matrix_dim": ..., "iterations": ...}
실패(예: mlx import 실패)는 stderr에 메시지를 남기고 0이 아닌 코드로 종료한다 — GFLOPS
0이나 가짜 값을 stdout에 내지 않는다(D22, 이 저장소는 조작된 메트릭으로 여러 번 데였다).
"""
import json
import sys
import time

MATRIX_DIM = 2048
ITERATIONS = 20


def main() -> int:
    try:
        import mlx.core as mx
    except ImportError as e:
        print(f"mlx import failed: {e}", file=sys.stderr)
        return 1

    a = mx.random.normal((MATRIX_DIM, MATRIX_DIM), dtype=mx.float32)
    b = mx.random.normal((MATRIX_DIM, MATRIX_DIM), dtype=mx.float32)
    # 입력 생성 자체도 lazy이므로, 측정 구간 밖에서 미리 강제 평가해 워밍업한다.
    mx.eval(a, b)

    start = time.perf_counter()
    for _ in range(ITERATIONS):
        c = mx.matmul(a, b)
        mx.eval(c)  # 매 반복 강제 동기화 — 위 함정 설명 참고.
    elapsed = time.perf_counter() - start

    if elapsed <= 0:
        print(f"non-positive elapsed time measured: {elapsed}", file=sys.stderr)
        return 1

    # GFLOPS 계산식(감사 가능하도록 명시): M x K 행렬과 K x N 행렬의 곱은 각 출력 원소마다
    # K번의 곱셈 + K번의 덧셈, 즉 원소당 2*K FLOPs. 여기서는 정방행렬이라 M=N=K=MATRIX_DIM이므로
    # 행렬곱 1회당 2 * MATRIX_DIM^3 FLOPs. 이를 반복 횟수만큼 곱하고 경과 시간(초)으로 나눈 뒤
    # 10^9로 나누면 GFLOPS다.
    flops_per_matmul = 2 * (MATRIX_DIM ** 3)
    total_flops = flops_per_matmul * ITERATIONS
    gflops = total_flops / elapsed / 1e9

    print(
        json.dumps(
            {
                "gflops": gflops,
                "elapsed_seconds": elapsed,
                "matrix_dim": MATRIX_DIM,
                "iterations": ITERATIONS,
            }
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
