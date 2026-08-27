# Security Policy

English | [한국어](SECURITY-ko.md)

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| v0.x    | :white_check_mark: |

## Security Scope & Credential Isolation

KubeMetal interacts with local Kubernetes configurations (`kubeconfig`), Metal GPU device handles, and container runtime sockets.

- Never commit private keys, cluster tokens, or sensitive endpoint configs.
- Secrets must be stored in macOS Keychain or protected credential stores.
- Tauri IPC commands validate all arguments before spawning OS processes.

## Reporting a Vulnerability

Please report vulnerabilities privately via GitHub Private Vulnerability Reporting or by contacting maintainers directly. Do not open public issues for sensitive security defects. Acknowledgements will be provided within 48 hours.

Reference: [OpenForge Security Standard](https://github.com/dasomel/openforge/blob/main/docs/security.md)
