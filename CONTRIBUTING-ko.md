# KubeMetal 기여 가이드 (Contributing Guide)

[English](CONTRIBUTING.md) | 한국어

## 로컬 개발 환경

```bash
# 의존성 설치
pnpm install

# Tauri 데스크톱 개발 서버 실행
pnpm tauri dev
```

## 기여 지침

- UI 컴포넌트는 `DESIGN.md`의 시맨틱 토큰을 사용합니다.
- 단위 테스트를 통과해야 합니다 (`cargo test`, `pnpm test`).
- TypeScript 타입 검사를 통과해야 합니다 (`pnpm typecheck`).
