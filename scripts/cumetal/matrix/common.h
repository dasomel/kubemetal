// Shared helpers for the CuMetal compatibility matrix probes (issue #84 Phase 2).
// Exit codes: 0 pass, 1 numeric mismatch, 20+ API failure (20 + call site is not
// tracked; the failing call is printed on stderr). Every probe prints exactly one
// "MATRIX_RESULT probe=<name> status=<PASS|MISMATCH> digest=<hex> <detail>" line.
#pragma once
#include <cstdio>
#include <cstdlib>
#include <cstdint>
#include <cmath>
#include <cstring>
#include <cuda_runtime.h>

#define CK(call) do { cudaError_t e_ = (call); if (e_ != cudaSuccess) { \
  fprintf(stderr, "MATRIX_API_FAIL %s:%d %s -> %s\n", __FILE__, __LINE__, #call, cudaGetErrorString(e_)); exit(20); } } while (0)

// FNV-1a over raw bytes: digest identical across runs == bit-identical outputs.
static inline uint64_t fnv(const void* p, size_t n, uint64_t h = 1469598103934665603ull) {
  const unsigned char* b = (const unsigned char*)p;
  for (size_t i = 0; i < n; ++i) { h ^= b[i]; h *= 1099511628211ull; }
  return h;
}
// digest=na marks outputs that are legitimately order-dependent (stability is then judged by pass-in-all-runs).
static inline int report(const char* name, bool ok, uint64_t digest, bool has_digest, const char* detail) {
  if (has_digest) printf("MATRIX_RESULT probe=%s status=%s digest=%016llx %s\n", name, ok ? "PASS" : "MISMATCH", (unsigned long long)digest, detail);
  else printf("MATRIX_RESULT probe=%s status=%s digest=na %s\n", name, ok ? "PASS" : "MISMATCH", detail);
  return ok ? 0 : 1;
}
