#include "common.h"
__global__ void k(int w, int h, int* o) {
  int x = blockIdx.x * blockDim.x + threadIdx.x, y = blockIdx.y * blockDim.y + threadIdx.y;
  if (x < w && y < h) o[y * w + x] = y * 1000 + x;
}
int main() {
  const int w = 333, hh = 197; size_t n = (size_t)w * hh; int* d; int* h = (int*)malloc(n * sizeof(int));
  CK(cudaMalloc(&d, n * sizeof(int))); CK(cudaMemset(d, 0, n * sizeof(int)));
  dim3 b(16, 8), g((w + 15) / 16, (hh + 7) / 8);
  k<<<g, b>>>(w, hh, d); CK(cudaGetLastError()); CK(cudaDeviceSynchronize());
  CK(cudaMemcpy(h, d, n * sizeof(int), cudaMemcpyDeviceToHost));
  long bad = 0; for (int y = 0; y < hh; ++y) for (int x = 0; x < w; ++x) if (h[y * w + x] != y * 1000 + x) ++bad;
  char buf[64]; snprintf(buf, sizeof buf, "w=%d h=%d bad=%ld", w, hh, bad);
  return report("launch_2d", bad == 0, fnv(h, n * sizeof(int)), true, buf);
}
