import React from 'react';
import { Cpu, Layers, HardDrive } from 'lucide-react';

export const Header: React.FC = () => {
  return (
    <header className="border-b border-default bg-surface px-6 py-4 flex items-center justify-between">
      <div className="flex items-center gap-3">
        <div className="w-10 h-10 rounded-lg bg-accent-strong flex items-center justify-center">
          <Layers className="w-5 h-5 text-inverse" />
        </div>
        <div>
          <h1 className="text-title text-primary flex items-center gap-2">
            KubeMetal
            <span className="text-caption px-2 py-0.5 rounded-full bg-accent/10 text-accent border border-accent/20">
              v0.1.0 MVP
            </span>
          </h1>
          <p className="text-caption text-secondary">
            macOS Host (MLX) + Colima K3s Hybrid MLOps Architecture
          </p>
        </div>
      </div>

      <div className="flex items-center gap-4 text-caption text-secondary">
        <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-surface-raised border border-default">
          <Cpu className="w-3.5 h-3.5 text-accent" />
          <span>Host: macOS Unified Memory</span>
        </div>
        <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-surface-raised border border-default">
          <HardDrive className="w-3.5 h-3.5 text-accent" />
          <span>VM: vz + virtiofs</span>
        </div>
      </div>
    </header>
  );
};
