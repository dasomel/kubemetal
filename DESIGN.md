---
version: alpha
name: KubeMetal Precision Instrument
description: >
  Apple 실리콘 하드웨어 감성의 소프트 화이트 계기판 라이트 UI. 단일 액센트(Silicon Violet)와
  3단 엘리베이션(페이지/카드/인셋)으로 위계를 표현하는 macOS 컨트롤 대시보드용 디자인 시스템.
  순백/순흑 대비를 피하고 부드러운 오프화이트·그레이 스케일로 눈의 피로를 낮춘다.
  DTCG(tokens.json)가 필요하면 `npx @google/design.md export --format dtcg DESIGN.md`로
  이 파일에서 파생 생성한다 — DESIGN.md가 유일한 소스다.
colors:
  primary: "#6D3FE0"
  primaryStrong: "#5B2FC7"
  inverse: "#FFFFFF"
  base: "#FAFAFA"
  surface: "#FFFFFF"
  surfaceRaised: "#F4F4F5"
  hairline: "#0F1115"
  ink: "#23262B"
  inkMuted: "#52565C"
  inkFaint: "#686C73"
  success: "#15803D"
  warning: "#B45309"
  danger: "#B91C1C"
  dangerStrong: "#9B1C1C"
typography:
  display:
    fontFamily: -apple-system, BlinkMacSystemFont, "Apple SD Gothic Neo", Pretendard, "SF Pro Display", "Segoe UI", Roboto, Helvetica, Arial, sans-serif
    fontSize: 22px
    fontWeight: 700
    lineHeight: 28px
    letterSpacing: -0.02em
  heading:
    fontFamily: -apple-system, BlinkMacSystemFont, "Apple SD Gothic Neo", Pretendard, "SF Pro Text", "Segoe UI", Roboto, Helvetica, Arial, sans-serif
    fontSize: 15px
    fontWeight: 600
    lineHeight: 20px
    letterSpacing: -0.01em
  label:
    fontFamily: -apple-system, BlinkMacSystemFont, "Apple SD Gothic Neo", Pretendard, "SF Pro Text", "Segoe UI", Roboto, Helvetica, Arial, sans-serif
    fontSize: 11px
    fontWeight: 600
    lineHeight: 14px
    letterSpacing: 0.08em
  body:
    fontFamily: -apple-system, BlinkMacSystemFont, "Apple SD Gothic Neo", Pretendard, "SF Pro Text", "Segoe UI", Roboto, Helvetica, Arial, sans-serif
    fontSize: 14px
    fontWeight: 400
    lineHeight: 20px
  bodyStrong:
    fontFamily: -apple-system, BlinkMacSystemFont, "Apple SD Gothic Neo", Pretendard, "SF Pro Text", "Segoe UI", Roboto, Helvetica, Arial, sans-serif
    fontSize: 14px
    fontWeight: 600
    lineHeight: 20px
  caption:
    fontFamily: -apple-system, BlinkMacSystemFont, "Apple SD Gothic Neo", Pretendard, "SF Pro Text", "Segoe UI", Roboto, Helvetica, Arial, sans-serif
    fontSize: 12px
    fontWeight: 500
    lineHeight: 16px
  metric:
    fontFamily: "'SF Mono', 'JetBrains Mono', ui-monospace, Menlo, monospace"
    fontSize: 34px
    fontWeight: 600
    lineHeight: 38px
    letterSpacing: -0.01em
rounded:
  sm: 6px
  md: 8px
  lg: 12px
  xl: 16px
  full: 9999px
spacing:
  xs: 4px
  sm: 8px
  md: 12px
  lg: 16px
  xl: 24px
  2xl: 32px
  3xl: 40px
components:
  panel:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.ink}"
    typography: "{typography.heading}"
    rounded: "{rounded.xl}"
    padding: 24px
  panel-inset:
    backgroundColor: "{colors.surfaceRaised}"
    textColor: "{colors.inkMuted}"
    typography: "{typography.body}"
    rounded: "{rounded.lg}"
    padding: 16px
  progress-track:
    backgroundColor: "{colors.base}"
    rounded: "{rounded.full}"
  label-caps:
    textColor: "{colors.inkFaint}"
    typography: "{typography.label}"
  metric-readout:
    textColor: "{colors.ink}"
    typography: "{typography.metric}"
  text-accent:
    textColor: "{colors.primary}"
    typography: "{typography.bodyStrong}"
  button-primary:
    backgroundColor: "{colors.primaryStrong}"
    textColor: "{colors.inverse}"
    typography: "{typography.bodyStrong}"
    rounded: "{rounded.md}"
    padding: 10px 16px
  button-ghost:
    backgroundColor: "{colors.surfaceRaised}"
    textColor: "{colors.ink}"
    typography: "{typography.bodyStrong}"
    rounded: "{rounded.md}"
    padding: 10px 16px
  button-danger:
    backgroundColor: "{colors.dangerStrong}"
    textColor: "{colors.inverse}"
    typography: "{typography.bodyStrong}"
    rounded: "{rounded.md}"
    padding: 10px 16px
  status-dot-success:
    backgroundColor: "{colors.success}"
    rounded: "{rounded.full}"
  status-dot-warning:
    backgroundColor: "{colors.warning}"
    rounded: "{rounded.full}"
  status-dot-danger:
    backgroundColor: "{colors.danger}"
    rounded: "{rounded.full}"
  border-hairline:
    backgroundColor: "{colors.hairline}"
---

## Overview

**정밀 계기판(Precision Instrument).** KubeMetal은 macOS 호스트와 Colima VM 사이의
하이브리드 MLOps 스택을 제어하는 데스크톱 컨트롤 패널이다. Apple 하드웨어의 절제된
알루미늄 마감을 기준으로, 화면은 "쨍한 다크 대시보드"가 아니라 소프트 화이트 계기판처럼
정보 위계가 명확하되 눈이 편안한 낮은 대비 톤을 지향한다.

- 배경은 **순백을 피한 소프트 화이트** 3단(`base` 페이지 → `surface` 카드 → `surfaceRaised`
  인셋)이며, 깊이는 두꺼운 보더가 아니라 **엘리베이션(표면 밝기 차 + 미세 그림자)**으로
  표현한다.
- 브랜드 색은 **Silicon Violet 하나**로 응축한다. 카드당 solid CTA는 항상 1개, 나머지
  액션은 ghost/quiet 톤으로 물러선다.
- 숫자(RAM GB, CPU %)는 계기판 눈금처럼 크고 tabular한 모노스페이스로, 라벨은
  11px uppercase + wide tracking으로 "장비 라벨" 느낌을 낸다.
- 상태(success/warning/danger)는 오직 상태 표시 전용이며, 항상 색 + 텍스트를 함께 쓴다.
- 텍스트는 순흑(`#000000`)을 쓰지 않는다. 가장 진한 텍스트도 소프트 그래파이트
  (`ink`)이며, 모든 대비는 "쨍함"이 아니라 "충분히 읽히는 부드러움"을 기준으로 조정한다.

## Colors

- **Primary — Silicon Violet (`#6D3FE0`, solid `#5B2FC7`):** 유일한 브랜드 액센트.
  링크·아이콘·포커스 링·인터랙티브 텍스트는 `primary`, 화이트 텍스트가 올라가는 solid
  버튼 배경은 대비를 확보한 `primaryStrong`을 쓴다. 화면당 solid 사용은 1곳으로 제한한다.
- **Base / Surface / SurfaceRaised (`#FAFAFA` / `#FFFFFF` / `#F4F4F5`):** 3단 엘리베이션
  소프트 화이트. `base`는 앱 최상위 배경(연한 오프화이트), `surface`는 패널/카드(순백에
  가장 가까움, 카드가 배경 위로 "떠 있는" 인상), `surfaceRaised`는 카드 내부 인셋 블록으로
  살짝 더 어두운 그레이를 써서 "안으로 들어간" 느낌을 낸다. 색조 차이가 아니라 밝기
  차이로만 단계를 구분한다(무채색 유지).
- **Hairline (`#0F1115`, 6~10% 알파로만 사용):** 표면 경계를 아주 얇게 표시할 때만 쓰는
  보더 색. 항상 `border-hairline/6` ~ `border-hairline/10` 형태의 저알파로 사용하며, 진한
  단색 보더로는 절대 쓰지 않는다.
- **Ink 스케일 (`ink #23262B` / `inkMuted #52565C` / `inkFaint #686C73`):** 텍스트 위계.
  타이틀·핵심 수치는 `ink`(순흑이 아닌 소프트 그래파이트), 본문은 `inkMuted`, 캡션·메타
  정보는 `inkFaint`.
- **Inverse (`#FFFFFF`):** `primaryStrong`/`dangerStrong` solid 버튼 위 텍스트 전용.
- **Success / Warning / Danger (`#15803D` / `#B45309` / `#B91C1C`, danger solid
  `#9B1C1C`):** 상태 표시 전용 색. 라이트 배경에서도 AA를 만족하는 저채도 톤으로
  조정했다. 장식적 강조로 사용하지 않는다.

모든 텍스트·아이콘 색상은 사용되는 배경(`base`/`surface`/`surfaceRaised`) 대비 WCAG AA
(4.5:1) 이상을 만족하도록 검증되었다. 순백 위 순흑 조합(`#FFFFFF` / `#000000`)은 사용하지
않는다 — 가장 진한 텍스트도 `ink`(`#23262B`)로 제한해 과도한 대비를 피한다.

## Typography

폰트 스택은 한국어 렌더링을 위해 `-apple-system, "Apple SD Gothic Neo", Pretendard, ...`
순서를 유지한다. 계기판 눈금 역할을 하는 수치는 별도로 모노스페이스(`typography.metric`)를
쓴다.

- **Display (22px/700, tracking -0.02em):** 헤더의 앱 이름 "KubeMetal" 1곳에만 사용한다.
- **Heading (15px/600, tracking -0.01em):** 패널 제목(예: "Colima K8s Control"). tight
  tracking으로 계기판 타이틀의 절제된 톤을 만든다.
- **Label (11px/600 uppercase, tracking +0.08em):** 계기판 라벨 전용. 섹션 이름, 스탯 블록
  상단 라벨(예: "UNIFIED MEMORY")에 uppercase로 적용한다. 본문 목적으로 확대 사용 금지.
- **Body / Body-strong (14px, 400/600):** 본문 텍스트와 본문 내 강조.
- **Caption (12px/500):** 보조 설명, 타임스탬프, 단위.
- **Metric (34px/600, 모노스페이스, tabular-nums, tracking -0.01em):** RAM/CPU 등 모든
  핵심 수치는 이 토큰만 사용해 숫자 폭을 고정하고 계기판처럼 크게 표시한다.

## Layout

페이지는 `max-w-7xl`(1280px)로 폭을 제한하고 좌우 여백은 `spacing.xl`(24px)을 쓴다.
카드 그룹은 반응형 그리드(`grid-cols-1 lg:grid-cols-2` 등)로 배치하며 카드 간 간격은
`spacing.xl`(24px), 카드 내부 요소 간 간격은 `spacing.lg`(16px)로 통일한다. 카드 패딩은
`spacing.xl`(24px), 카드 내부 서브 블록(패널 인셋) 패딩은 `spacing.lg`(16px)만 쓴다.
임의의 중간값 패딩(예: 20px, 14px)은 사용하지 않는다.

## Elevation & Depth

깊이는 **두꺼운 보더가 아니라 엘리베이션**으로 표현한다. `base`(페이지) → `surface`(카드,
가장 밝음) → `surfaceRaised`(인셋, 살짝 더 어두움) 순서로 표면의 밝기 차이가 앞뒤 관계를
만든다. 보더는 장식이 아니라 아주 얇은 구분선 용도로만 `hairline` 색을 6~10% 알파로
사용한다(`rgba(15,17,21,0.06~0.10)`에 해당). 카드에는 아주 미세한 그림자만 더해 표면이
배경 위에 살짝 떠 있는 느낌을 낸다 — 다크 테마의 무거운 그림자와 달리 그림자 자체가
튀지 않도록 절제한다:

- `panel` 기본 그림자: `0 1px 2px rgba(15,17,21,0.04), 0 10px 24px -10px rgba(15,17,21,0.12)`
- `panel-inset`(카드 내부 인셋 블록) 그림자: 없음 — 밝기 차이만으로 충분히 구분된다.

## Shapes

모서리는 계기판 베젤처럼 절제된 라운드를 쓴다. 패널/카드는 `rounded.xl`(16px), 카드
내부 서브 블록은 `rounded.lg`(12px), 버튼·인풋은 `rounded.md`(8px), 소형 배지·상태
점(dot)은 `rounded.sm`(6px) 또는 완전한 원(`rounded.full`)만 사용한다. pill(알약형) 배지는
상태 점 용도를 제외하고 남용하지 않는다.

## Components

- **panel**: 대시보드 카드/패널의 기본 컨테이너. `surface` 배경 + `ink` 텍스트.
- **panel-inset**: 패널 내부의 강조 블록(메트릭 카드, 상태 서브카드). `surfaceRaised` 배경.
- **progress-track**: 프로그레스 바 트랙. `base` 배경 위에 `primary` 채움을 얹는다.
- **label-caps**: 계기판 라벨. `inkFaint` 텍스트 + `typography.label`(uppercase 적용).
- **metric-readout**: 핵심 수치. `ink` 텍스트 + `typography.metric`(tabular-nums).
- **text-accent**: 링크/아이콘 등 인터랙티브 강조 텍스트. `primary` 텍스트.
- **button-primary**: 화면당 1개만 쓰는 solid CTA. `primaryStrong` 배경 + `inverse` 텍스트.
- **button-ghost**: 보조 액션. `surfaceRaised` 배경 + `ink` 텍스트, hairline 테두리.
- **button-danger**: 파괴적 액션(정지 등) 전용 solid 버튼. `dangerStrong` 배경 + `inverse` 텍스트.
- **status-dot-success / warning / danger**: 상태 표시 전용 점(dot). 항상 텍스트 라벨과
  함께 쓰고 점 단독으로 상태를 전달하지 않는다.
- **border-hairline**: 표면 경계 구분선. 항상 저알파로만 적용한다.

## Do's and Don'ts

- Do: 색상은 반드시 `colors.*` 토큰만 참조한다. 컴포넌트 코드에 raw hex나 Tailwind
  기본 팔레트(`slate-*`, `blue-*` 등)를 직접 쓰지 않는다.
- Do: solid 버튼(`button-primary`/`button-danger`)은 화면(카드)당 1개만 배치하고, 나머지
  액션은 `button-ghost`로 낮춘다.
- Do: 상태는 항상 상태 점(dot) + 텍스트 라벨을 함께 표기한다(색맹 대응). 점 색만으로
  상태를 구분하지 않는다.
- Do: 깊이는 `base → surface → surfaceRaised` 밝기 단계와 미세 그림자로만 표현한다.
- Don't: 문서·스펙 표기(D1, D4, NFR-02 같은 내부 결정/요구사항 번호)를 UI 텍스트에
  노출하지 않는다. 그런 표기는 설계 문서에만 남긴다.
- Don't: pill 배지를 정보 라벨로 남용하지 않는다. 상태 전용이 아니면 배지 대신 라벨 텍스트를
  쓴다.
- Don't: `hairline` 색을 불투명(100% 알파) 단색 보더로 쓰지 않는다. 항상 6~10% 알파로만
  적용한다.
- Don't: `success`/`warning`/`danger`를 장식적 강조(배경 포인트 컬러 등) 목적으로 쓰지
  않는다. 오직 상태 표시 전용이다.
- Don't: 순백(`#FFFFFF`) 배경 위에 순흑(`#000000`) 텍스트, 또는 그 반대 조합을 쓰지
  않는다. 가장 진한 텍스트도 `ink`(`#23262B`)로 제한해 과도한 대비를 피한다.
