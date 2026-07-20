import React, { useEffect, useState } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Archive, Database, HardDrive, Rocket, ArrowUpRight, Eye, EyeOff, RefreshCw } from 'lucide-react';
import { useServiceAccess } from '../../hooks/useServiceAccess';
import type { ServiceAccess, CredentialItem } from '../../types/ipc';

const openEndpoint = (url: string) => {
  openUrl(url).catch(() => window.open(url, '_blank'));
};

const SERVICE_ICON: Record<string, React.ElementType> = {
  MLflow: Archive,
  'SeaweedFS S3 API': Database,
  'SeaweedFS Filer UI': HardDrive,
  'Model Serving': Rocket,
};

const MASK = '••••••••••';

const isSecretKey = (key: string) => key.toLowerCase().includes('secret');

const CredentialRow: React.FC<{ item: CredentialItem }> = ({ item }) => {
  const secret = isSecretKey(item.key);
  const [revealed, setRevealed] = useState(!secret);

  return (
    <div className="flex items-center gap-1.5">
      <span className="text-caption text-inkFaint w-32 shrink-0 truncate">{item.key}</span>
      <input
        type="text"
        readOnly
        value={revealed ? item.value : MASK}
        onClick={(e) => (e.target as HTMLInputElement).select()}
        className="flex-1 min-w-0 px-2 py-1 rounded-md bg-surfaceRaised border border-hairline/8 text-caption text-ink font-mono focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
      />
      {secret && (
        <button
          type="button"
          onClick={() => setRevealed((r) => !r)}
          aria-label={revealed ? '값 숨기기' : '값 보기'}
          className="shrink-0 p-1.5 rounded-md bg-surfaceRaised hover:brightness-95 text-inkMuted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        >
          {revealed ? <EyeOff className="w-3.5 h-3.5" /> : <Eye className="w-3.5 h-3.5" />}
        </button>
      )}
    </div>
  );
};

const ServiceCard: React.FC<{ service: ServiceAccess }> = ({ service }) => {
  const Icon = SERVICE_ICON[service.service] ?? Database;
  const isOk = service.health === 'ok';

  return (
    <div className="rounded-xl bg-surface p-5 shadow-panel">
      <div className="flex items-center gap-2 mb-3">
        <Icon className="w-4 h-4 text-primary shrink-0" />
        <h3 className="text-bodyStrong text-ink truncate">{service.service}</h3>
      </div>

      <div className="flex items-center gap-1.5 text-caption text-inkMuted mb-2">
        <span className={`w-2 h-2 rounded-full shrink-0 ${isOk ? 'bg-success' : 'bg-danger'}`} />
        <span>{isOk ? '정상' : '연결 불가'}</span>
        <span className="text-inkFaint truncate">· {service.url}</span>
      </div>

      {service.credential_hint && (
        <div className="text-caption text-inkFaint mb-2">{service.credential_hint}</div>
      )}

      {service.credentials.length > 0 && (
        <div className="space-y-1.5 mb-3">
          {service.credentials.map((item) => (
            <CredentialRow key={item.key} item={item} />
          ))}
        </div>
      )}

      <button
        type="button"
        onClick={() => openEndpoint(service.url)}
        className="px-2.5 py-1 rounded-md bg-surfaceRaised hover:brightness-95 text-primary text-caption flex items-center gap-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
      >
        원클릭 접근
        <ArrowUpRight className="w-3 h-3" />
      </button>

      {!isOk && (
        <div className="text-caption text-inkFaint mt-2 pt-2 border-t border-hairline/8">
          대시보드에서 포트포워딩 시작
        </div>
      )}
    </div>
  );
};

export const AccessConsole: React.FC = () => {
  const { services, loading, error, loaded, refresh } = useServiceAccess();

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="space-y-6">
      <div className="rounded-xl bg-surface p-4 flex items-center justify-between gap-4 shadow-panel">
        <div>
          <h2 className="text-heading text-ink mb-0.5">접근 콘솔</h2>
          <p className="text-caption text-inkMuted">
            로컬 단일 사용자 SSO — 서비스별 크리덴셜 자동 확인과 원클릭 인증 접근을 제공합니다.
          </p>
        </div>
        <button
          type="button"
          onClick={() => refresh()}
          disabled={loading}
          className="shrink-0 px-3 py-1.5 rounded-md bg-surfaceRaised hover:brightness-95 disabled:opacity-50 text-caption text-inkMuted flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        >
          <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
          {loading ? '새로고침 중...' : '새로고침'}
        </button>
      </div>

      {error && (
        <div className="rounded-xl bg-surface p-4 shadow-panel text-caption text-danger">
          서비스 접근 정보 조회 실패: {error}
        </div>
      )}

      {loaded && !error && services.length === 0 ? (
        <div className="rounded-xl bg-surface p-8 shadow-panel text-center text-inkMuted text-body">
          조회된 서비스가 없습니다.
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {services.map((service) => (
            <ServiceCard key={service.service} service={service} />
          ))}
        </div>
      )}
    </div>
  );
};
