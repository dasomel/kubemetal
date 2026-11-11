#include "common.h"
#include "parity.h"
#define T 16
__global__ void sgemm(int N, const float* A, const float* B, float* C) {
  __shared__ float As[T][T], Bs[T][T];
  int r = blockIdx.y * T + threadIdx.y, c = blockIdx.x * T + threadIdx.x; float acc = 0;
  for (int t = 0; t < N; t += T) {
    As[threadIdx.y][threadIdx.x] = A[r * N + t + threadIdx.x]; Bs[threadIdx.y][threadIdx.x] = B[(t + threadIdx.y) * N + c];
    __syncthreads();
    for (int k = 0; k < T; ++k) acc += As[threadIdx.y][k] * Bs[k][threadIdx.x];
    __syncthreads();
  }
  C[r * N + c] = acc;
}
int main() {
  const int N = 512; size_t b = (size_t)N * N * 4; float *A = (float*)malloc(b), *B = (float*)malloc(b), *C = (float*)malloc(b); float *dA, *dB, *dC;
  for (int i = 0; i < N * N; ++i) { A[i] = sinf(i * 0.01f); B[i] = cosf(i * 0.017f); }
  CK(cudaMalloc(&dA, b)); CK(cudaMalloc(&dB, b)); CK(cudaMalloc(&dC, b));
  CK(cudaMemcpy(dA, A, b, cudaMemcpyHostToDevice)); CK(cudaMemcpy(dB, B, b, cudaMemcpyHostToDevice));
  sgemm<<<dim3(N / T, N / T), dim3(T, T)>>>(N, dA, dB, dC); CK(cudaGetLastError()); CK(cudaDeviceSynchronize());
  CK(cudaMemcpy(C, dC, b, cudaMemcpyDeviceToHost));
  parity::Check check; check.scan(C, (size_t)N * N);  // normalise by sum|a||b|
  for (int i = 0; i < N; i += 7) for (int j = 0; j < N; j += 5) {
    double r = 0, m = 0; for (int k = 0; k < N; ++k) { r += (double)A[i * N + k] * B[k * N + j]; m += fabs((double)A[i * N + k] * B[k * N + j]); }
    check.add(i * N + j, parity::ratio(fabs(C[i * N + j] - r), m));
  }
  char buf[160]; check.detail(buf, sizeof buf, 1e-5);
  return report("sgemm_handwritten", check.passes(1e-5), fnv(C, b), true, buf);
}
