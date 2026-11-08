#include "common.h"
__global__ void inc(int* p, int n) { int i = blockIdx.x * blockDim.x + threadIdx.x; if (i < n) p[i] += 1; }
int main() {
  const int n = 1 << 18; size_t b = n * 4; int *h, *r = (int*)malloc(b), *d; cudaStream_t s; cudaEvent_t e0, e1;
  CK(cudaMallocHost(&h, b)); for (int i = 0; i < n; ++i) h[i] = i;
  CK(cudaMalloc(&d, b)); CK(cudaStreamCreate(&s)); CK(cudaEventCreate(&e0)); CK(cudaEventCreate(&e1));
  CK(cudaEventRecord(e0, s));
  CK(cudaMemcpyAsync(d, h, b, cudaMemcpyHostToDevice, s));
  for (int it = 0; it < 4; ++it) inc<<<n / 256, 256, 0, s>>>(d, n);
  CK(cudaMemcpyAsync(r, d, b, cudaMemcpyDeviceToHost, s));
  CK(cudaEventRecord(e1, s)); CK(cudaStreamSynchronize(s)); CK(cudaEventSynchronize(e1));
  float ms = -1; CK(cudaEventElapsedTime(&ms, e0, e1));
  long bad = 0; for (int i = 0; i < n; ++i) if (r[i] != i + 4) ++bad;
  bool tok = ms >= 0.0f && ms < 60000.0f;  // sanity only: timing value itself is not asserted deterministic
  CK(cudaStreamDestroy(s));
  char buf[96]; snprintf(buf, sizeof buf, "bad=%ld elapsed_ms=%.3f timing_sane=%d", bad, ms, (int)tok);
  return report("stream_event", bad == 0 && tok, fnv(r, b), true, buf);
}
