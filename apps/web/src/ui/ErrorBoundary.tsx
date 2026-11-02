import { Component, type ReactNode } from "react";

/** Instead of a blank page when something breaks: what happened, and a way to start over. Local
 * data is kept, so nothing made here is lost by reloading. */
export class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="login">
        <div className="login-card">
          <div className="login-brand">
            <h1>出错了</h1>
            <p>{this.state.error.message}</p>
            <p>本机的数据都还在，重新加载就能继续。</p>
          </div>
          <button className="filled-btn" onClick={() => location.reload()}>
            重新加载
          </button>
        </div>
      </div>
    );
  }
}
