import { Component, type ReactNode } from "react";

/** Instead of a blank page when something breaks: what happened, and a way to start over. Local
 * data is kept, so nothing made here is lost by reloading. */
export class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null; details: boolean }> {
  state = { error: null as Error | null, details: false };

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
          <div className="login-local">
            <button className="text-btn" onClick={() => this.setState({ details: !this.state.details })}>
              {this.state.details ? "收起错误详情" : "错误详情"}
            </button>
            {this.state.details && <p className="error-details">{this.state.error.message}</p>}
          </div>
        </div>
      </div>
    );
  }
}
