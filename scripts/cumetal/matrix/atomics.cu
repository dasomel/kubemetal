#include "common.h"
// int atomicAdd, CAS-loop max (via atomicCAS), float atomicAdd of small integers (exact regardless of order).
__global__ void k(int n, int* sum, int* mx, float* fsum) {
  int i = blockIdx.x * blockDim.x + threadIdx.x; if (i >= n) return;
  atomicAdd(sum, i % 7);
  int v = (i * 2654435761u) % 100003, old = *mx;
  while (v > old) { int prev = atomicCAS(mx, old, v); if (prev == old) break; old = prev; }
  atomicAdd(fsum, (float)(i % 5));
}
int main() {
  const int n = 1 << 18; int *dsum, *dmx; float* dfs; int hs = 0, hm = 0; float hf = 0;
  CK(cudaMalloc(&dsum, 4)); CK(cudaMalloc(&dmx, 4)); CK(cudaMalloc(&dfs, 4));
  CK(cudaMemset(dsum, 0, 4)); CK(cudaMemset(dmx, 0, 4)); CK(cudaMemset(dfs, 0, 4));
  k<<<n / 256, 256>>>(n, dsum, dmx, dfs); CK(cudaGetLastError()); CK(cudaDeviceSynchronize());
  CK(cudaMemcpy(&hs, dsum, 4, cudaMemcpyDeviceToHost)); CK(cudaMemcpy(&hm, dmx, 4, cudaMemcpyDeviceToHost)); CK(cudaMemcpy(&hf, dfs, 4, cudaMemcpyDeviceToHost));
  long rs = 0, rf = 0; int rm = 0;
  for (int i = 0; i < n; ++i) { rs += i % 7; int v = (int)((unsigned)(i * 2654435761u) % 100003); if (v > rm) rm = v; rf += i % 5; }
  bool ok = hs == rs && hm == rm && (long)hf == rf;  // rf < 2^24? 2^18*2 = 524k, exact in fp32
  char buf[128]; snprintf(buf, sizeof buf, "int_add=%d/%ld cas_max=%d/%d f_add=%.0f/%ld", hs, rs, hm, rm, hf, rf);
  int r[3] = {hs, hm, (int)hf};
  return report("atomics", ok, fnv(r, sizeof r), true, buf);
}
