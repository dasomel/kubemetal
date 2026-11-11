#include "common.h"
#include <cusparse.h>

#define SP(call) do { cusparseStatus_t s_ = (call); if (s_ != CUSPARSE_STATUS_SUCCESS) { \
  fprintf(stderr, "MATRIX_API_FAIL %s:%d %s -> %d (%s)\n", __FILE__, __LINE__, #call, (int)s_, cusparseGetErrorString(s_)); return 21; } } while (0)

int main() {
  // D22: measure only FP32, zero-based CSR, non-transpose SpMV. Cost: no
  // coverage of other sparse APIs; extend with separate probes, never infer it.
  const int rows = 4, cols = 5, nnz = 7;
  int offsets[] = {0, 2, 2, 5, 7};  // includes an empty row
  int indices[] = {0, 3, 1, 2, 4, 0, 4};
  float values[] = {1.5f, -2.0f, 3.0f, 0.5f, -1.0f, -4.0f, 2.5f};
  float x[] = {2.0f, -1.0f, 0.25f, 3.0f, -2.0f};
  float initial[] = {1.0f, -2.0f, 0.5f, 4.0f}, output[rows];
  float alpha = 1.25f, beta = -0.5f;
  int *d_offsets, *d_indices;
  float *d_values, *d_x, *d_y;
  CK(cudaMalloc(&d_offsets, sizeof offsets));
  CK(cudaMalloc(&d_indices, sizeof indices));
  CK(cudaMalloc(&d_values, sizeof values));
  CK(cudaMalloc(&d_x, sizeof x));
  CK(cudaMalloc(&d_y, sizeof initial));
  CK(cudaMemcpy(d_offsets, offsets, sizeof offsets, cudaMemcpyHostToDevice));
  CK(cudaMemcpy(d_indices, indices, sizeof indices, cudaMemcpyHostToDevice));
  CK(cudaMemcpy(d_values, values, sizeof values, cudaMemcpyHostToDevice));
  CK(cudaMemcpy(d_x, x, sizeof x, cudaMemcpyHostToDevice));
  CK(cudaMemcpy(d_y, initial, sizeof initial, cudaMemcpyHostToDevice));

  cusparseHandle_t handle;
  cusparseSpMatDescr_t matrix;
  cusparseDnVecDescr_t vec_x, vec_y;
  SP(cusparseCreate(&handle));
  SP(cusparseCreateCsr(&matrix, rows, cols, nnz, d_offsets, d_indices, d_values,
                     CUSPARSE_INDEX_32I, CUSPARSE_INDEX_32I, CUSPARSE_INDEX_BASE_ZERO, CUDA_R_32F));
  SP(cusparseCreateDnVec(&vec_x, cols, d_x, CUDA_R_32F));
  SP(cusparseCreateDnVec(&vec_y, rows, d_y, CUDA_R_32F));
  size_t workspace_bytes = 0;
  SP(cusparseSpMV_bufferSize(handle, CUSPARSE_OPERATION_NON_TRANSPOSE, &alpha, matrix,
                           vec_x, &beta, vec_y, CUDA_R_32F, CUSPARSE_SPMV_ALG_DEFAULT, &workspace_bytes));
  void* workspace = nullptr;
  if (workspace_bytes) CK(cudaMalloc(&workspace, workspace_bytes));
  SP(cusparseSpMV(handle, CUSPARSE_OPERATION_NON_TRANSPOSE, &alpha, matrix,
                 vec_x, &beta, vec_y, CUDA_R_32F, CUSPARSE_SPMV_ALG_DEFAULT, workspace));
  CK(cudaDeviceSynchronize());
  CK(cudaMemcpy(output, d_y, sizeof output, cudaMemcpyDeviceToHost));

  bool finite = true, ok = true;
  double worst = 0;
  const double atol = 1e-5, rtol = 1e-5;
  for (int row = 0; row < rows; ++row) {
    double sum = 0;
    for (int k = offsets[row]; k < offsets[row + 1]; ++k) sum += (double)values[k] * x[indices[k]];
    double reference = alpha * sum + beta * initial[row];
    // D22: fmax alone hides NaN; reject non-finite outputs before comparing.
    if (!std::isfinite(output[row])) { finite = false; ok = false; continue; }
    double error = fabs(output[row] - reference);
    worst = fmax(worst, error);
    if (error > atol + rtol * fabs(reference)) ok = false;
  }
  SP(cusparseDestroyDnVec(vec_y));
  SP(cusparseDestroyDnVec(vec_x));
  SP(cusparseDestroySpMat(matrix));
  SP(cusparseDestroy(handle));
  if (workspace) CK(cudaFree(workspace));
  CK(cudaFree(d_y)); CK(cudaFree(d_x)); CK(cudaFree(d_values));
  CK(cudaFree(d_indices)); CK(cudaFree(d_offsets));
  char detail[160];
  snprintf(detail, sizeof detail, "rows=%d cols=%d nnz=%d finite=%s max_abs_err=%.3e atol=1e-5 rtol=1e-5",
           rows, cols, nnz, finite ? "true" : "false", worst);
  return report("cusparse_spmv", ok, fnv(output, sizeof output), true, detail);
}
