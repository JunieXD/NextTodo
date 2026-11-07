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
            <h1>页面出错</h1>
            <p>本机数据未受影响，重新加载即可继续。</p>
          </div>
          <button className="filled-btn" onClick={() => location.reload()}>
            重新加载
          </button>
          {/* For telling the developer: it means little to the user. */}
          <details className="error-details">
            <summary>错误详情</summary>
            {this.state.error.message}
          </details>
        </div>
      </div>
    );
  }
}
