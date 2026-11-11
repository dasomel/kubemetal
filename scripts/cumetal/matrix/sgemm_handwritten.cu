#include "parity.h"
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstring>
#include <cstdlib>
#include <vector>

namespace benchmark {
inline bool count(const char* text, int limit, int& value) {
  if (!text[0]) return false;
  for (const char* p = text; *p; ++p) if (*p < '0' || *p > '9') return false;
  char* end; long n = strtol(text, &end, 10);
  if (*end || n < 1 || n > limit) return false;
  value = (int)n; return true;
}
inline int arguments(int argc, char** argv, int& warmup, int& iterations) {
  if (argc == 1) return 0;
  if (argc != 6 || strcmp(argv[1], "--benchmark") || strcmp(argv[2], "--warmup") ||
      strcmp(argv[4], "--iterations") || !count(argv[3], 20, warmup) ||
      !count(argv[5], 100, iterations)) return -1;
  return 1;
}
struct Samples {
  long long first_ns = 0;
  std::vector<long long> steady_ns;
  double median() const {
    auto sorted = steady_ns; std::sort(sorted.begin(), sorted.end());
    size_t n = sorted.size();
    return n % 2 ? sorted[n / 2] : sorted[n / 2 - 1] / 2.0 + sorted[n / 2] / 2.0;
  }
  long long p95() const {
    auto sorted = steady_ns; std::sort(sorted.begin(), sorted.end());
    return sorted[(95 * sorted.size() + 99) / 100 - 1];
  }
};
enum Outcome { ok, dispatch_failed, parity_failed, clock_failed };
// D22: time only launch + synchronization; validation stays outside the interval.
// Cost: host timing includes dispatch overhead. GPU-event timing needs separate evidence.
template<class Clock, class Dispatch, class Validate>
Outcome measure(int warmup, int iterations, Clock clock, Dispatch dispatch, Validate validate, Samples& out) {
  for (int i = 0; i < 1 + warmup + iterations; ++i) {
    auto start = clock();
    if (!dispatch()) return dispatch_failed;
    auto end = clock();
    if (end < start) return clock_failed;
    if (!validate()) return parity_failed;
    if (i == 0) out.first_ns = end - start;
    else if (i > warmup) out.steady_ns.push_back(end - start);
  }
  return ok;
}
inline void print(const Samples& samples, int warmup) {
  printf("BENCHMARK_RESULT {\"unit\":\"ns\",\"warmup_count\":%d,\"sample_count\":%zu,"
         "\"first_dispatch_ns\":%lld,\"median_ns\":%.1f,\"p95_ns\":%lld,\"samples_ns\":[",
         warmup, samples.steady_ns.size(), samples.first_ns, samples.median(), samples.p95());
  for (size_t i = 0; i < samples.steady_ns.size(); ++i) printf("%s%lld", i ? "," : "", samples.steady_ns[i]);
  printf("]}\n");
}
}

#ifndef CUMETAL_BENCHMARK_HOST_TEST
#include "common.h"
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
int main(int argc, char** argv) {
  int warmup = 0, iterations = 0;
  int mode = benchmark::arguments(argc, argv, warmup, iterations);
  if (mode < 0) {
    fprintf(stderr, "usage: sgemm_handwritten [--benchmark --warmup 1..20 --iterations 1..100]\n");
    return 2;
  }
  const int N = 512; size_t b = (size_t)N * N * 4; float *A = (float*)malloc(b), *B = (float*)malloc(b), *C = (float*)malloc(b); float *dA, *dB, *dC;
  for (int i = 0; i < N * N; ++i) { A[i] = sinf(i * 0.01f); B[i] = cosf(i * 0.017f); }
  CK(cudaMalloc(&dA, b)); CK(cudaMalloc(&dB, b)); CK(cudaMalloc(&dC, b));
  CK(cudaMemcpy(dA, A, b, cudaMemcpyHostToDevice)); CK(cudaMemcpy(dB, B, b, cudaMemcpyHostToDevice));
  parity::Check check;
  auto dispatch = [&]() {
    sgemm<<<dim3(N / T, N / T), dim3(T, T)>>>(N, dA, dB, dC);
    CK(cudaGetLastError()); CK(cudaDeviceSynchronize()); return true;
  };
  auto validate = [&]() {
    CK(cudaMemcpy(C, dC, b, cudaMemcpyDeviceToHost));
    check = parity::Check{}; check.scan(C, (size_t)N * N);
    for (int i = 0; i < N; i += 7) for (int j = 0; j < N; j += 5) {
      double r = 0, m = 0;
      for (int k = 0; k < N; ++k) { r += (double)A[i * N + k] * B[k * N + j]; m += fabs((double)A[i * N + k] * B[k * N + j]); }
      check.add(i * N + j, parity::ratio(fabs(C[i * N + j] - r), m));
    }
    return check.passes(1e-5);
  };
  if (mode) {
    benchmark::Samples samples;
    auto clock = []() { return std::chrono::duration_cast<std::chrono::nanoseconds>(
      std::chrono::steady_clock::now().time_since_epoch()).count(); };
    auto outcome = benchmark::measure(warmup, iterations, clock, dispatch, validate, samples);
    if (outcome == benchmark::ok) benchmark::print(samples, warmup);
    else if (outcome != benchmark::parity_failed) { fprintf(stderr, "BENCHMARK_FAILED timing/dispatch\n"); return 20; }
  } else { dispatch(); validate(); }
  char buf[160]; check.detail(buf, sizeof buf, 1e-5);
  return report("sgemm_handwritten", check.passes(1e-5), fnv(C, b), true, buf);
}
#endif
