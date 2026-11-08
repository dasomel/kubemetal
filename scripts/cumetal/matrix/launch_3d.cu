#include "common.h"
__global__ void k(int X, int Y, int Z, int* o) {
  int x = blockIdx.x * blockDim.x + threadIdx.x, y = blockIdx.y * blockDim.y + threadIdx.y, z = blockIdx.z * blockDim.z + threadIdx.z;
  if (x < X && y < Y && z < Z) o[(z * Y + y) * X + x] = z * 10000 + y * 100 + x;
}
int main() {
  const int X = 37, Y = 29, Z = 11; size_t n = (size_t)X * Y * Z; int* d; int* h = (int*)malloc(n * sizeof(int));
  CK(cudaMalloc(&d, n * sizeof(int))); CK(cudaMemset(d, 0, n * sizeof(int)));
  dim3 b(8, 4, 4), g((X + 7) / 8, (Y + 3) / 4, (Z + 3) / 4);
  k<<<g, b>>>(X, Y, Z, d); CK(cudaGetLastError()); CK(cudaDeviceSynchronize());
  CK(cudaMemcpy(h, d, n * sizeof(int), cudaMemcpyDeviceToHost));
  long bad = 0; for (int z = 0; z < Z; ++z) for (int y = 0; y < Y; ++y) for (int x = 0; x < X; ++x) if (h[(z * Y + y) * X + x] != z * 10000 + y * 100 + x) ++bad;
  char buf[64]; snprintf(buf, sizeof buf, "dims=%dx%dx%d bad=%ld", X, Y, Z, bad);
  return report("launch_3d", bad == 0, fnv(h, n * sizeof(int)), true, buf);
}
