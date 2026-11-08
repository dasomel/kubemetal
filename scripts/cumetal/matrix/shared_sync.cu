#include "common.h"
// Block-wise tree reduction (dynamic shared) + static-shared tile transpose, both need __syncthreads.
__global__ void blocksum(const int* in, int* out, int n) {
  extern __shared__ int s[];
  int t = threadIdx.x, i = blockIdx.x * blockDim.x + t;
  s[t] = i < n ? in[i] : 0; __syncthreads();
  for (int st = blockDim.x / 2; st > 0; st >>= 1) { if (t < st) s[t] += s[t + st]; __syncthreads(); }
  if (t == 0) out[blockIdx.x] = s[0];
}
#define T 16
__global__ void transpose(const int* in, int* out, int N) {
  __shared__ int tile[T][T + 1];
  int x = blockIdx.x * T + threadIdx.x, y = blockIdx.y * T + threadIdx.y;
  tile[threadIdx.y][threadIdx.x] = in[y * N + x]; __syncthreads();
  int tx = blockIdx.y * T + threadIdx.x, ty = blockIdx.x * T + threadIdx.y;
  out[ty * N + tx] = tile[threadIdx.x][threadIdx.y];
}
int main() {
  const int n = 1 << 16, B = 256, nb = n / B; int *hi = (int*)malloc(n * 4), *ho = (int*)malloc(nb * 4);
  for (int i = 0; i < n; ++i) hi[i] = (i * 7) % 101 - 50;
  int *di, *dout; CK(cudaMalloc(&di, n * 4)); CK(cudaMalloc(&dout, nb * 4));
  CK(cudaMemcpy(di, hi, n * 4, cudaMemcpyHostToDevice));
  blocksum<<<nb, B, B * sizeof(int)>>>(di, dout, n); CK(cudaGetLastError()); CK(cudaDeviceSynchronize());
  CK(cudaMemcpy(ho, dout, nb * 4, cudaMemcpyDeviceToHost));
  long bad = 0; for (int b = 0; b < nb; ++b) { int r = 0; for (int j = 0; j < B; ++j) r += hi[b * B + j]; if (r != ho[b]) ++bad; }
  const int N = 128; int *ti = (int*)malloc(N * N * 4), *to = (int*)malloc(N * N * 4); int *dti, *dto;
  for (int i = 0; i < N * N; ++i) ti[i] = i;
  CK(cudaMalloc(&dti, N * N * 4)); CK(cudaMalloc(&dto, N * N * 4)); CK(cudaMemcpy(dti, ti, N * N * 4, cudaMemcpyHostToDevice));
  transpose<<<dim3(N / T, N / T), dim3(T, T)>>>(dti, dto, N); CK(cudaGetLastError()); CK(cudaDeviceSynchronize());
  CK(cudaMemcpy(to, dto, N * N * 4, cudaMemcpyDeviceToHost));
  for (int y = 0; y < N; ++y) for (int x = 0; x < N; ++x) if (to[y * N + x] != ti[x * N + y]) ++bad;
  char buf[64]; snprintf(buf, sizeof buf, "blocks=%d bad=%ld", nb, bad);
  return report("shared_sync", bad == 0, fnv(to, N * N * 4, fnv(ho, nb * 4)), true, buf);
}
