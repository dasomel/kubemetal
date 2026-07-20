import React from 'react';
import { Cpu, Layers, HardDrive } from 'lucide-react';

export const Header: React.FC = () => {
  return (
    <header className="border-b border-hairline/8 bg-surface px-6 py-4 flex items-center justify-between">
      <div className="flex items-center gap-3">
        <div className="w-10 h-10 rounded-lg bg-primaryStrong flex items-center justify-center">
          <Layers className="w-5 h-5 text-inverse" />
        </div>
        <div>
          <h1 className="text-display text-ink flex items-center gap-2">
            KubeMetal
            <span className="text-caption font-normal text-inkFaint">v0.1.0 MVP</span>
          </h1>
          <p className="text-caption text-inkMuted">
            macOS Host (MLX) + Colima K3s Hybrid MLOps Architecture
          </p>
        </div>
      </div>

      <div className="flex items-center gap-3 text-caption text-inkMuted">
        <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-surfaceRaised border border-hairline/8">
          <Cpu className="w-3.5 h-3.5 text-primary" />
          <span>Host: macOS Unified Memory</span>
        </div>
        <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-surfaceRaised border border-hairline/8">
          <HardDrive className="w-3.5 h-3.5 text-primary" />
          <span>VM: vz + virtiofs</span>
        </div>
      </div>
    </header>
  );
};
