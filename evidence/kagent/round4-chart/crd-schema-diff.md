# CRD property/type comparison

[measured — host-probe/schema-diff.json] 아래 수는 전체 flattened schema의 경로 수이며 앱 출력 필드 수가 아니다. 비교 쌍은 `kagent.dev/v1alpha2/<Kind> -> api.kagent.dev/v1alpha3/<Kind>`이다.

| Kind (v1alpha2 → v1alpha3) | added | removed | type_changed |
|---|---:|---:|---:|
| Agent | 78 | 1798 | 0 |
| ModelConfig | 29 | 8 | 1 |
| RemoteMCPServer | 1 | 4 | 0 |

[measured — host-probe/0-kagent-crds-0.9.12-paths.json; host-probe/1-kagent-crds-1.0.0-alpha17-paths.json] 앱이 출력·설정하는 필드별 old/new 타입과 literal 후보는 [code-compatibility-matrix.md](code-compatibility-matrix.md)에 기록했다. 컨테이너·array item·map value·schema alternative도 경로 수에 포함된다.

[unverified] 수치나 후보 경로만으로 requiredness, defaults, CEL, admission, conversion webhooks, 의미 동등성, migration 또는 runtime 호환성을 증명하지 않는다.
