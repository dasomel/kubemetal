---
name: scope-discipline
description: Keep changes within the smallest coherent scope and preserve desktop, native, ML runtime, Kubernetes, filesystem, network, and credential boundaries.
---

# Scope Discipline

Treat Tauri command exposure, filesystem/process/network access, model/tool authorization, credential scope, public APIs, and destructive cluster actions as design-level scope expansion. Avoid unrelated refactors.
