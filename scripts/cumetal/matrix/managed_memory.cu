#include "common.h"
__global__ void k(float* p, int n) { int i = blockIdx.x * blockDim.x + threadIdx.x; if (i < n) p[i] = p[i] * 2.0f + 1.0f; }
int main() {
  const int n = 1 << 16; float* m;
  CK(cudaMallocManaged(&m, n * sizeof(float)));
  for (int i = 0; i < n; ++i) m[i] = (float)(i % 1000);
  k<<<n / 256, 256>>>(m, n); CK(cudaGetLastError()); CK(cudaDeviceSynchronize());
  long bad = 0; for (int i = 0; i < n; ++i) if (m[i] != (float)(i % 1000) * 2.0f + 1.0f) ++bad;
  uint64_t d = fnv(m, n * sizeof(float)); CK(cudaFree(m));
  char buf[48]; snprintf(buf, sizeof buf, "n=%d bad=%ld", n, bad);
  return report("managed_memory", bad == 0, d, true, buf);
}
