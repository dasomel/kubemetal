#include "common.h"
// Warp-level butterfly reduction with __shfl_xor_sync; each 32-lane warp sums its lanes.
__global__ void k(const int* in, int* out, int n) {
  int i = blockIdx.x * blockDim.x + threadIdx.x; int v = i < n ? in[i] : 0;
  for (int m = 16; m > 0; m >>= 1) v += __shfl_xor_sync(0xffffffffu, v, m);
  if ((threadIdx.x & 31) == 0) out[i / 32] = v;
}
int main() {
  const int n = 1 << 14, nw = n / 32; int *h = (int*)malloc(n * 4), *o = (int*)malloc(nw * 4); int *di, *dout;
  for (int i = 0; i < n; ++i) h[i] = (i * 13) % 97 - 40;
  CK(cudaMalloc(&di, n * 4)); CK(cudaMalloc(&dout, nw * 4)); CK(cudaMemcpy(di, h, n * 4, cudaMemcpyHostToDevice));
  k<<<n / 128, 128>>>(di, dout, n); CK(cudaGetLastError()); CK(cudaDeviceSynchronize());
  CK(cudaMemcpy(o, dout, nw * 4, cudaMemcpyDeviceToHost));
  long bad = 0; for (int w = 0; w < nw; ++w) { int r = 0; for (int j = 0; j < 32; ++j) r += h[w * 32 + j]; if (r != o[w]) ++bad; }
  char buf[64]; snprintf(buf, sizeof buf, "warps=%d bad=%ld", nw, bad);
  return report("warp_shuffle", bad == 0, fnv(o, nw * 4), true, buf);
}
