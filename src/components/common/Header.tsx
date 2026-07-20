import React from 'react';
import { Cpu, Layers, HardDrive } from 'lucide-react';

export const Header: React.FC = () => {
  return (
    <header className="border-b border-slate-800 bg-slate-900/50 backdrop-blur px-6 py-4 flex items-center justify-between">
      <div className="flex items-center gap-3">
        <div className="w-10 h-10 rounded-xl bg-gradient-to-tr from-blue-600 to-indigo-500 flex items-center justify-center shadow-lg shadow-blue-500/20">
          <Layers className="w-5 h-5 text-white" />
        </div>
        <div>
          <h1 className="text-xl font-extrabold tracking-tight text-white flex items-center gap-2">
            KubeMetal
            <span className="text-xs px-2 py-0.5 rounded-full bg-blue-500/10 text-blue-400 border border-blue-500/20 font-medium">
              v0.1.0 MVP
            </span>
          </h1>
          <p className="text-xs text-slate-400">
            macOS Host (MLX) + Colima K3s Hybrid MLOps Architecture
          </p>
        </div>
      </div>

      <div className="flex items-center gap-4 text-xs text-slate-400">
        <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-slate-800/60 border border-slate-700/50">
          <Cpu className="w-3.5 h-3.5 text-indigo-400" />
          <span>Host: macOS Unified Memory</span>
        </div>
        <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-slate-800/60 border border-slate-700/50">
          <HardDrive className="w-3.5 h-3.5 text-blue-400" />
          <span>VM: vz + virtiofs</span>
        </div>
      </div>
    </header>
  );
};
