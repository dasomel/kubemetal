#include "common.h"
__global__ void k(int n, int* o) { int i = blockIdx.x * blockDim.x + threadIdx.x; if (i < n) o[i] = i * 3 + 1; }
int main() {
  const int n = 100003; int* d; int* h = (int*)malloc(n * sizeof(int));
  CK(cudaMalloc(&d, n * sizeof(int))); CK(cudaMemset(d, 0, n * sizeof(int)));
  k<<<(n + 127) / 128, 128>>>(n, d); CK(cudaGetLastError()); CK(cudaDeviceSynchronize());
  CK(cudaMemcpy(h, d, n * sizeof(int), cudaMemcpyDeviceToHost));
  long bad = 0; for (int i = 0; i < n; ++i) if (h[i] != i * 3 + 1) ++bad;
  char buf[64]; snprintf(buf, sizeof buf, "n=%d bad=%ld", n, bad);
  return report("launch_1d", bad == 0, fnv(h, n * sizeof(int)), true, buf);
}
