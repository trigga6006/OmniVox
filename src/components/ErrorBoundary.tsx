import { Component, type ReactNode } from "react";
import { RefreshCw } from "lucide-react";

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { hasError: false, error: null };

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error("OmniVox UI crashed:", error, info.componentStack);
  }

  handleReset = () => {
    this.setState({ hasError: false, error: null });
  };

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex h-screen w-screen items-center justify-center bg-surface-0 p-8 text-text-primary">
          <div className="max-w-md text-center">
            <div className="mx-auto mb-5 flex h-14 w-14 items-center justify-center rounded-full border border-error/30 bg-error/[0.10]">
              <span className="text-xl font-semibold text-error">!</span>
            </div>
            <h2 className="mb-2 font-display text-xl font-semibold tracking-[-0.02em] text-text-primary">
              Something went wrong
            </h2>
            <p className="mb-1 text-sm text-text-muted">
              The UI encountered an unexpected error. Your recordings and settings are safe.
            </p>
            {this.state.error && (
              <p className="mb-5 break-all font-mono text-[11px] text-text-muted/70">
                {this.state.error.message}
              </p>
            )}
            <button
              onClick={this.handleReset}
              className="inline-flex items-center gap-2 rounded-lg border border-amber-400/35 bg-amber-500/[0.14] px-4 py-2 text-sm font-medium text-amber-200 transition-colors hover:border-amber-400/55 hover:bg-amber-500/[0.22]"
            >
              <RefreshCw size={14} />
              Reload UI
            </button>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
