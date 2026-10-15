import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { SupportBundleResult, SupportBundleVerificationReport } from '../types/ipc';

export function useSupportBundle() {
  const [result, setResult] = useState<SupportBundleResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [verificationReport, setVerificationReport] = useState<SupportBundleVerificationReport | null>(null);
  const [verifyError, setVerifyError] = useState<string | null>(null);
  const [verifying, setVerifying] = useState(false);
  const mounted = useRef(false);
  const pending = useRef(false);
  const verifyPending = useRef(false);

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  const create = useCallback(async () => {
    // D1 (local, #17): serialize clicks to avoid duplicate disk bundles; no queue,
    // retry after completion. Ignore UI updates after unmount; IPC still finishes.
    if (pending.current) return;
    pending.current = true;
    setLoading(true);
    setResult(null);
    setError(null);
    setVerificationReport(null);
    setVerifyError(null);
    try {
      const response = await invoke<SupportBundleResult>('create_support_bundle');
      if (mounted.current) setResult(response);
    } catch (err) {
      if (mounted.current) setError(String(err));
    } finally {
      pending.current = false;
      if (mounted.current) setLoading(false);
    }
  }, []);

  const verify = useCallback(async (bundleDir?: string, expectedManifestSha256?: string) => {
    const targetDir = bundleDir || result?.bundle_dir;
    if (!targetDir || verifyPending.current) return;
    verifyPending.current = true;
    setVerifying(true);
    setVerifyError(null);
    try {
      const report = await invoke<SupportBundleVerificationReport>('verify_support_bundle', {
        bundleDir: targetDir,
        expectedManifestSha256: expectedManifestSha256 || null,
      });
      if (mounted.current) setVerificationReport(report);
    } catch (err) {
      if (mounted.current) setVerifyError(String(err));
    } finally {
      verifyPending.current = false;
      if (mounted.current) setVerifying(false);
    }
  }, [result]);

  return {
    result,
    error,
    loading,
    create,
    verificationReport,
    verifyError,
    verifying,
    verify,
  };
}
