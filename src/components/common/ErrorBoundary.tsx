import React from 'react';
import { AlertTriangle } from 'lucide-react';

interface ErrorBoundaryState {
  error: Error | null;
}

/**
 * React는 렌더링 중 잡히지 않은 예외가 발생하면 전체 트리를 언마운트한다 — 별도
 * 처리가 없으면 화면 전체가 빈 페이지로 보인다("화면이 없어짐"). 이 경계는 그 예외를
 * 붙잡아 원인을 보여주고 재시도할 수 있게 한다. `resetKey`가 바뀌면(예: 탭 전환)
 * 오류 상태를 자동으로 초기화한다.
 */
export class ErrorBoundary extends React.Component<
  React.PropsWithChildren<{ resetKey?: unknown }>,
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error('렌더링 오류:', error, info.componentStack);
  }

  componentDidUpdate(prevProps: React.PropsWithChildren<{ resetKey?: unknown }>) {
    if (this.state.error && prevProps.resetKey !== this.props.resetKey) {
      this.setState({ error: null });
    }
  }

  private handleReset = () => {
    this.setState({ error: null });
  };

  render() {
    const { error } = this.state;
    if (error) {
      return (
        <div className="rounded-xl bg-surface p-4 shadow-panel">
          <div className="flex items-center gap-2 mb-2 text-danger">
            <AlertTriangle className="w-4 h-4 shrink-0" />
            <h2 className="text-heading">화면을 표시하는 중 오류가 발생했습니다</h2>
          </div>
          <p className="text-body text-inkMuted mb-4 break-words">
            {error.message || '알 수 없는 오류'}
          </p>
          <button
            type="button"
            onClick={this.handleReset}
            className="py-2 px-3.5 bg-primaryStrong hover:brightness-110 text-inverse text-bodyStrong rounded-md transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
          >
            다시 시도
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
