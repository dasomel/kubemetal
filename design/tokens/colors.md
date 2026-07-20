# Colors

다크 대시보드 팔레트. 원본 정의는 `design-tokens.json`, Tailwind 매핑은 `tailwind.config.js`.
모든 텍스트 색상은 배경 대비 WCAG AA(4.5:1) 이상 검증됨(아래 표).

## 팔레트

| 토큰 | HEX | 용도 | Tailwind 클래스 |
|---|---|---|---|
| bg.base | `#0b0f16` | 앱 최상위 배경 | `bg-base` |
| bg.surface | `#131a26` | 카드/패널 배경 | `bg-surface` |
| bg.surface-raised | `#1c2433` | 서브 카드, 강조 블록 | `bg-surface-raised` |
| border.default | `#2b3444` | 카드 테두리, 구분선 | `border-default` / `border` |
| border.strong | `#5a6684` | 버튼·인풋 등 인터랙티브 테두리(3:1) | `border-strong` |
| text.primary | `#f4f7fb` | 타이틀, 핵심 수치 (16.2:1) | `text-primary` |
| text.secondary | `#c7cedb` | 본문 (11.0:1) | `text-secondary` |
| text.muted | `#98a2b5` | 캡션, 보조 설명 (6.8:1) | `text-muted` |
| text.inverse | `#ffffff` | accent-strong/danger-strong solid 버튼 위 텍스트 | `text-inverse` |
| accent | `#5b93f5` / strong `#2f5fc4` | 링크·아이콘·포커스 링 / solid 버튼 | `text-accent`, `bg-accent-strong` |
| success | `#3ddc97` | RUNNING/Ready 상태 | `text-success`, `bg-success/10` |
| warning | `#f5b642` | 경고 상태 | `text-warning`, `bg-warning/10` |
| danger | `#ff7a7a` / strong `#bf3939` | STOPPED/정지 액션 | `text-danger`, `bg-danger-strong` |

## 사용 규칙

1. **컴포넌트 내 raw HEX·`slate-*`·`blue-*` 등 팔레트 직접 클래스 사용 금지.** 반드시 위 시맨틱 클래스만 사용한다. (예외: `index.css`의 토큰 정의 자체)
2. 배경은 3단 위계만 사용: `bg-base`(앱 배경) → `bg-surface`(카드) → `bg-surface-raised`(카드 내부 강조 블록). 그 외 임의 톤 추가 금지.
3. 본문/라벨 텍스트는 `text-secondary` 이상만 사용한다. `text-muted`는 캡션·타임스탬프 등 부가 정보 전용이며 절대 본문에 쓰지 않는다.
4. 상태(RUNNING/STOPPED/Ready 등)는 **색 + 텍스트 라벨 + 아이콘**을 항상 함께 표기한다(색맹 대응). 색만으로 상태를 구분하지 않는다.
5. solid 버튼(흰 텍스트)에는 `accent-strong` / `danger-strong`만 사용한다. 기본 `accent`/`danger`는 텍스트·아이콘·테두리 전용이며 흰 텍스트와 대비가 부족하다(AA 미달).
6. 테두리는 장식용 `border-default`, 포커스링·인터랙티브 강조는 `border-strong` 또는 `ring-accent`를 사용한다.
