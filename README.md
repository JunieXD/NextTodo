# NextTodo

以先后关系为核心的待办应用：事项之间的依赖组成一张有向无环图，随时告诉你现在能做什么。文件夹可以无限嵌套，手机优先，多端实时同步，agent 能直接管理整个待办。

文档：[总体设计](docs/design.md)、[界面](docs/ui.md)、[数据模型](docs/data-model.md)、[API](docs/api.md)、[agent 接入](docs/agent.md)、[网页端](docs/web.md)。

## 常用命令

| 做什么 | 命令 |
|---|---|
| 跑全部检查 | `cargo fmt --check && cargo clippy --all-targets && cargo test` |
| 启动开发版服务端（`127.0.0.1:8080`） | `cargo run -p nexttodo-server -- --data .dev-data serve` |
| 创建管理员 | `cargo run -p nexttodo-server -- --data .dev-data create-admin <用户名>` |
| 安装命令行 `nexttodo` | `cargo install --path crates/cli` |
| 命令行连开发版 | `nexttodo login --url http://127.0.0.1:8080` |
| 网页端开发（`127.0.0.1:5173`，转发到开发版服务端） | `cd apps/web && pnpm install && pnpm dev` |
| 网页端检查 | `cd apps/web && pnpm check` |
| 在本机构建部署镜像 | `deploy/deploy.sh --build` |
| 部署到 rcvps | `deploy/deploy.sh` |
