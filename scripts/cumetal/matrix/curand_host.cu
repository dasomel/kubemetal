#include "common.h"
#include <curand.h>
// Sanity only (a PRNG has no CPU reference): uniform in (0,1], mean~0.5, var~1/12; normal mean~0, std~1; same seed -> same stream.
static int gen(float* d, float* h, int n, bool normal, unsigned long long seed) {
  curandGenerator_t g; curandStatus_t s = curandCreateGenerator(&g, CURAND_RNG_PSEUDO_DEFAULT); if (s) { fprintf(stderr, "MATRIX_API_FAIL curandCreateGenerator=%d\n", (int)s); return 21; }
  if ((s = curandSetPseudoRandomGeneratorSeed(g, seed))) { fprintf(stderr, "MATRIX_API_FAIL setSeed=%d\n", (int)s); return 22; }
  s = normal ? curandGenerateNormal(g, d, n, 0.0f, 1.0f) : curandGenerateUniform(g, d, n);
  if (s) { fprintf(stderr, "MATRIX_API_FAIL generate=%d\n", (int)s); return 23; }
  CK(cudaDeviceSynchronize()); CK(cudaMemcpy(h, d, n * 4, cudaMemcpyDeviceToHost)); curandDestroyGenerator(g); return 0;
}
int main() {
  const int n = 1 << 20; float *d, *u = (float*)malloc(n * 4), *u2 = (float*)malloc(n * 4), *z = (float*)malloc(n * 4);
  CK(cudaMalloc(&d, n * 4)); int rc;
  if ((rc = gen(d, u, n, false, 1234))) return rc; if ((rc = gen(d, u2, n, false, 1234))) return rc; if ((rc = gen(d, z, n, true, 99))) return rc;
  double um = 0, uv = 0, zm = 0, zv = 0; int oor = 0;
  for (int i = 0; i < n; ++i) { um += u[i]; zm += z[i]; if (!(u[i] > 0.0f && u[i] <= 1.0f)) ++oor; }
  um /= n; zm /= n; for (int i = 0; i < n; ++i) { uv += (u[i] - um) * (u[i] - um); zv += (z[i] - zm) * (z[i] - zm); } uv /= n; zv /= n;
  bool same = memcmp(u, u2, n * 4) == 0;
  bool ok = oor == 0 && fabs(um - 0.5) < 2e-3 && fabs(uv - 1.0 / 12) < 2e-3 && fabs(zm) < 5e-3 && fabs(sqrt(zv) - 1.0) < 5e-3 && same;
  char buf[160]; snprintf(buf, sizeof buf, "u_mean=%.4f u_var=%.4f n_mean=%.4f n_std=%.4f out_of_range=%d same_seed_identical=%d", um, uv, zm, sqrt(zv), oor, (int)same);
  return report("curand_host", ok, fnv(u, n * 4), true, buf);
}
