# 보존된 CuMetal matrix 오프라인 회귀 비교 (#84)

```sh
python3 -B scripts/cumetal/regression/compare.py before.json after.json comparison.json
```

두 원본 `cumetal-matrix/1` JSON과 각각의 `evidence.directory` 전체를 보존한다.
상대 경로 관계를 유지하면 다른 머신에서도 실행할 수 있다. Python 표준 라이브러리만
사용하며 compiler, CuMetal, GPU, 네트워크를 호출하지 않는다. 출력 JSON 및 같은 이름의
Markdown은 새 파일이어야 한다. 원본과 증거는 수정하지 않는다.

먼저 기존 `matrix/classify.py` replay를 사용한다. replay의 `evidence.verify`는 complete,
파일 inventory와 digest를 검사하고 원본 JSON의 index digest까지 대조한다. 재분류된
manifest/results가 원본과 다르면 거부한다. 독립적인 로그 재봉인도 원본 digest와
맞지 않아 거부한다. 원본 JSON과 증거를 함께 위조하는 공격을 인증하는 서명은 아니다.

비교 조건:

- host 식별 성공과 Mac model, SoC, RAM, GPU cores, macOS version/build 동일성.
  개인 serial/UUID를 수집하지 않으므로 물리적으로 같은 기기인지는 증명하지 않는다.
- 동일한 비어 있지 않은 probe 집합, 중복 없는 행, probe별 AOT/JIT 상태와 library.
- 보존 source 전체의 실제 digest와 `probe_sha256` 일치 및 실행 간 동일성.
  header, runner, classifier 변경도 비교 조건 변경으로 거부한다.
- repeats 및 manifest의 실행 조건(prefix, doctor 성공, 추가 필드) 동일성.
  실행 시각과 CuMetal/LLVM/Xcode/Metal 버전은 달라도 된다. 버전 차이는 JSON에 기록한다.
  버전 변경을 회귀의 자동 면책 사유로 사용하지 않는다.

기존 runner는 timeout 환경변수 override, background load, thermal 상태를 보존하지 않는다.
이 도구는 **보존된 조건만** 비교한다. 운영자는 기록되지 않은 조건도 동일하게 유지해야
한다. 이 비교는 성능 gate나 물리적 host 인증이 아니다.

각 probe의 AOT/JIT를 독립적으로 비교한다. supported → failed/unavailable은 regression,
다른 검증된 상태 → supported는 improvement다. supported → partial 및 어느 쪽이든
unverified인 경우 incomparable이다. 나머지 상태 변경은 changed, 동일 상태는 unchanged로
표시한다. JSON은 양쪽 전체 분류 증거를 보존하고 Markdown은 candidate 증거를 표시하므로
numeric mismatch, parity line(max_error/finite 등), compile 오류와 caveat를 잃지 않는다.
기존 실패가 그대로인 경우 no_regression은 수치 검증 성공을 뜻하지 않는다.

종료 코드: `0` = no_regression, `1` = regression, `2` = incomparable 또는 입력/출력 오류.
regression과 incomparable이 함께 있으면 regression이 우선하며 모든 행은 그대로 남는다.
누락/변조/조건 불일치에서는 비교 결과를 생성하지 않고 stderr에 오류를 출력한다.

검증(CPU fixture):

```sh
python3 -B -m unittest discover -s tests/cumetal -p 'test_regression_compare.py' -v
```

기존 `make test`가 `tests/cumetal` 전체를 discover하므로 `make verify`에도 자동 포함된다.
이 slice에 필요한 GPU live 실행은 없다. 운영자가 이미 보존한 업그레이드 전후 실행을
위 명령으로 비교하면 된다.
