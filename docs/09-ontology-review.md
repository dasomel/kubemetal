# 09. 온톨로지 검토 — theaxlabs 블로그 + Microsoft Ontology Playground

> 2026-07-24. 근거: [블로그 원문](https://theaxlabs.com/blog/우리-회사는-같은-단어를-같은-뜻으로-쓰고-있는가),
> [microsoft/Ontology-Playground](https://github.com/microsoft/Ontology-Playground) (실체 직접 검증).

## 1. 블로그 논지 평가

- **동의**: "에이전트 도입의 최대 장벽은 모델 성능이 아니라 용어 불일치"는 이 프로젝트의
  경험과 정확히 일치한다 — 우리도 D-레지스트리(D1~D21)·mistakes-log라는 "기계가 지키는
  정의의 소스 오브 트루스"를 운영하며 같은 문제를 풀어왔다(블로그의 온톨로지 vs LLM Wiki
  보완론에서 우리는 이미 Wiki 쪽 절반을 하고 있는 셈).
- **실용적 결론 수용**: "명사 6개+화살표 7개, 1단계(용어 정의)만으로 가치의 대부분" —
  풀 스케일 RDF/추론이 아니라 경량 용어집이 적정.

## 2. Microsoft Ontology Playground 실체 (검증됨)

| 항목 | 확인 결과 |
|------|-----------|
| 실체 | Microsoft 공식, 2.2k★, 활발(390+ 커밋), MIT |
| 형태 | 완전 정적 웹앱(설치·로그인 불요), React/TS |
| 기능 | 시각적 온톨로지 에디터(Cytoscape), 6개 도메인 예제, 학습 코스, NL2Ontology 프리뷰 |
| 입출력 | RDF/XML·OWL 입력, RDF/XML·JSON 내보내기 |
| 한계 | **데이터에 연결되지 않음**(모델링·교육 전용); 실데이터 결합은 유료 Fabric IQ 영역 |

평가: 도구 자체는 벤더 중립 RDF를 다루므로 Fabric 미사용자도 "용어 모델링+시각화+RDF
보관" 용도로 부담 없이 쓸 수 있다. 단 우리 규모에선 필수가 아니라 선택(시각화 보조).

## 3. KubeMetal 적용 — 우리 제품에 이미 있는 용어 충돌

| 단어 | 실제로 가리키는 서로 다른 것들 |
|------|-------------------------------|
| "모델" | ① HF 저장소(검색 결과) ② 로컬 다운로드 디렉터리 ③ MLflow Registered Model ④ 서빙 중 인스턴스 ⑤ (혼용) 어댑터 |
| "데이터셋" | ① 학습용 jsonl 디렉터리 ② DVC 버전 스냅샷 ③ RAG 문서 컬렉션 ④ 수집 이력 항목 |
| "파이프라인" | ① 파이프라인 탭(상태 뷰) ② Prefect flow ③ 수집 DAG |

UI 라벨·IPC 이름·문서에서 이 혼용이 이미 사용자 혼란("서빙 아래 채팅 vs 시맨틱 검색
차이" 질문 등)의 원인 중 하나다.

## 4. 권고 (경량 채택 — 블로그 1단계 수준)

1. **`docs/10-glossary.md` 용어 온톨로지(-라이트) 작성**: 엔티티 8개(BaseModel,
   Adapter, RegisteredModel, ServingInstance, TrainingDataset, DatasetVersion,
   DocumentCollection, FlowRun) + 관계 화살표(파인튜닝은 BaseModel+TrainingDataset→
   Adapter, 서빙은 BaseModel[+Adapter]→ServingInstance …) + 각 엔티티의 UI 표기·IPC
   타입 매핑. D-레지스트리와 상호 링크.
2. **정합 감사**: 용어집 확정 후 UI 라벨/i18n 키/문서를 1회 정렬(예: 서빙 카드의
   "베이스 모델" vs 모델 허브의 "모델" 구분 표기).
3. **RAG 연계는 보류**: 컬렉션 메타에 엔티티 태깅 같은 확장은 지금 과함 — 블로그도
   1단계 우선을 권고.
4. Playground는 위 용어집을 시각화·RDF로 남기고 싶을 때 선택 사용(산출물 RDF를
   `docs/ontology/`에 보관 가능).

**Fabric IQ는 불채택**(유료·MS 데이터 플랫폼 종속 — 로컬-퍼스트 원칙과 불일치).

## 5. 비교 검증 추가: Obsidian 로컬 온톨로지 vs MS Ontology Playground

Obsidian으로 로컬 온톨로지를 구축하는 실무 사례가 실제로 많다 — frontmatter 속성 +
위키링크를 관계로 쓰고, [typed link/그래프 뷰](https://volodymyrpavlyshyn.medium.com/personal-knowledge-graphs-in-obsidian-528a0f4584b9),
Dataview 질의, Breadcrumbs/ExcaliBrain 플러그인으로 유형화하며,
[RDF 내보내기 경로](https://volodymyrpavlyshyn.medium.com/how-to-export-your-obsidian-vault-to-rdf-00fb2539ed18)와
[다국어 제품 온톨로지 사례](https://juliadiez.substack.com/p/building-a-multilingual-product-ontology),
[LLM 엔티티 추출로 그래프를 만드는 플러그인](https://github.com/junhewk/simple-graph-builder)까지 존재한다.

| 축 | Obsidian 방식 | MS Ontology Playground |
|----|---------------|------------------------|
| 형식성 | 자유 md+frontmatter, 링크는 기본 무유형(플러그인으로 유형화) | 엔티티/속성/관계/카디널리티 정식 모델, RDF/OWL 네이티브 |
| 정합 검증 | 강제 없음 — Dataview 질의로 반(半)검증 | 스키마 수준 모델링(단, 실데이터 연결은 양쪽 다 없음) |
| 시각화 | 그래프 뷰·ExcaliBrain(연상 중심) | Cytoscape 다이어그램(모델 중심) |
| 로컬-퍼스트·버전관리 | md+git — 최상 | 정적 웹(산출 RDF를 git 보관하는 식) |
| 에이전트/RAG 친화 | **md 자체가 LLM 컨텍스트·RAG 인덱싱에 최적** | RDF는 파서 필요 |
| 의존성 | Obsidian 앱(개인 도구 성격) | 브라우저만(무설치) |

### 검증 결론 (KubeMetal 맥락)

- 우리가 채택한 **`docs/10-glossary.md`(md+git+mermaid)** 방식은 사실상 "Obsidian식
  로컬 온톨로지의 저장소-네이티브 형태"다 — 앱 의존을 제거하고도 같은 이점(로컬 파일,
  버전관리, LLM 친화)을 얻으며, **우리 RAG 파이프라인이 docs/를 인덱싱하므로 용어집이
  자동으로 에이전트의 지식이 된다**(Obsidian 사례들이 별도 플러그인으로 얻으려는 것).
- 역할 분담: **repo-md 용어집(주 채택)** / Playground(형식 RDF 산출·시각 편집이 필요할
  때 보조) / Obsidian(사용자 개인 지식 큐레이션 선택지 — 저장소 표준으로는 불채택,
  앱 종속·정합 강제 부재).
