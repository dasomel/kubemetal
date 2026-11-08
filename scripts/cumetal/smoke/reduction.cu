#include <cstdio>
#include <cuda_runtime.h>
__global__ void reduce(const int* in, unsigned long long* out, int n) {
  __shared__ int s[256];
  int t = threadIdx.x, i = blockIdx.x * blockDim.x + t;
  s[t] = i < n ? in[i] : 0;
  __syncthreads();
  for (int k = blockDim.x / 2; k > 0; k >>= 1) { if (t < k) s[t] += s[t + k]; __syncthreads(); }
  if (t == 0) atomicAdd(out, (unsigned long long)s[0]);
}
int main() {
  const int n = 1 << 20; int* h = new int[n]; unsigned long long ref = 0;
  for (int i = 0; i < n; ++i) { h[i] = (i % 97) - 13; ref += (unsigned long long)(long long)h[i]; }
  int* d; unsigned long long* dout; unsigned long long zero = 0, got = 0;
  if (cudaMalloc(&d, n * sizeof(int)) || cudaMalloc(&dout, sizeof(zero))) return 10;
  cudaMemcpy(d, h, n * sizeof(int), cudaMemcpyHostToDevice); cudaMemcpy(dout, &zero, sizeof(zero), cudaMemcpyHostToDevice);
  reduce<<<(n + 255) / 256, 256>>>(d, dout, n);
  if (cudaDeviceSynchronize() != cudaSuccess) return 11;
  cudaMemcpy(&got, dout, sizeof(got), cudaMemcpyDeviceToHost);
  printf("reduction n=%d got=%lld expected=%lld\n", n, (long long)got, (long long)ref);
  return got == ref ? 0 : 1;
}
