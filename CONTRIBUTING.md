# Contributing to KubeMetal

English | [한국어](CONTRIBUTING-ko.md)

## Development Setup

```bash
# Install dependencies
pnpm install

# Run Tauri desktop app in dev mode
pnpm tauri dev
```

## Guidelines

- All UI components must use semantic tokens from `DESIGN.md`.
- Keep tests current (`cargo test` and `pnpm test`).
- Type check must pass without errors (`pnpm typecheck`).
