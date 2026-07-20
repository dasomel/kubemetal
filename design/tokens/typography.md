# Typography

폰트 스택은 한국어 렌더링을 고려해 `-apple-system, "Apple SD Gothic Neo", Pretendard, ...` 순서로 구성했다 (`tailwind.config.js` `fontFamily.sans`). 수치·코드는 `font-mono`.

## 스케일

| 토큰 | 크기/행간 | 굵기 | 용도 | Tailwind 클래스 |
|---|---|---|---|---|
| caption | 12/16px | 500 | 캡션, 단위, 타임스탬프 | `text-caption` |
| body | 14/20px | 400 | 본문 텍스트 | `text-body` |
| body-strong | 14/20px | 600 | 본문 내 라벨/강조 | `text-body-strong` |
| subtitle | 16/24px | 500 | 카드 제목 | `text-subtitle` |
| section | 20/28px | 700 | 섹션 헤더(패널 h2) | `text-section` |
| title | 24/32px | 800 | 페이지 타이틀(앱 이름) | `text-title` |
| metric | 32/40px | 700 | RAM/CPU 등 핵심 수치 | `text-metric tabular-nums` |

## 위계 규칙

1. 한 화면에 `title`은 헤더의 앱 이름 1곳에만 사용한다.
2. 패널 제목(`시스템 자원 현황`, `Colima K8s Control` 등)은 `section`, 카드 내부 소제목은 `subtitle`을 사용해 페이지 타이틀과 구분되게 한다.
3. 본문 설명은 `body`, 부가 설명·단위·메타 정보는 `caption`만 사용한다. `caption`을 본문 목적으로 확대 사용하지 않는다.
4. **모든 메트릭 수치(RAM GB, CPU %, 사용률)는 `text-metric` + `tabular-nums`를 사용**해 숫자 폭을 고정하고 큰 사이즈로 표시한다. 단위(GB, %)는 옆에 `caption` 톤으로 붙인다.
5. 색상은 `colors.md`의 시맨틱 텍스트 색만 사용한다(`text-primary`/`text-secondary`/`text-muted`). 타이포 크기와 색상 위계를 함께 낮추지 않는다(예: title에 muted 금지).
