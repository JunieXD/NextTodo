# 部署

NextTodo 的服务端和网页端在同一个 Docker 镜像里，部署一次，网页、Android、macOS 和 Windows 就都能连上。决策见[总体设计](design.md#部署)。

## 用 Docker 部署

需要一台装了 Docker 和 Docker Compose 的 Linux 服务器（amd64 或 arm64），以及一个能配 HTTPS 的域名。

```sh
mkdir nexttodo && cd nexttodo
curl -fsSLO https://raw.githubusercontent.com/JunieXD/NextTodo/main/deploy/compose.yaml
mkdir data && sudo chown 65534:65534 data
docker compose up -d
docker compose exec nexttodo /nexttodo-server create-admin <用户名>
```

- 服务端以 65534 用户运行，`data/` 要归它所有，里面是 SQLite 数据库和每日快照。
- 默认只监听本机的 `127.0.0.1:8080`，交给同一台机器上的反向代理。换端口在同目录的 `.env` 里写 `NEXTTODO_PORT=8790`。
- 默认跟随最新版本（`latest`）。要固定版本，在 `.env` 里写 `NEXTTODO_IMAGE=ghcr.io/juniexd/nexttodo:<版本号>`，版本号见 [Releases](https://github.com/JunieXD/NextTodo/releases)。
- [AI 整理](ai.md)的配置要同步到服务器（提交后设备可以离线），在 `.env` 里写 `NEXTTODO_SECRET_KEY=<32 字节的 base64>`，可以用 `openssl rand -base64 32` 生成。服务端用它加密用户的 API 密钥，它不在 `data/` 里，数据库和快照单独泄露时密钥仍然安全。不设置时，配置只能保存在各自的设备上。换掉或丢了它，已保存的 API 密钥就解不开，要在设置里重新填写。
- `create-admin` 会让你输两次密码。其他人用邀请码注册：管理员在设置的“邀请注册”里生成邀请码或复制邀请链接。

## 反向代理

反向代理负责 HTTPS，要开启 WebSocket，并把原来的 `Host` 传给服务端（或设 `X-Forwarded-Host`），网页的同步连接靠它确认是同源页面。

Caddy 默认就满足这些要求，还会自动申请证书：

```
todo.example.com {
	reverse_proxy 127.0.0.1:8080
}
```

Nginx：

```nginx
server {
    listen 443 ssl;
    server_name todo.example.com;
    # ssl_certificate 和 ssl_certificate_key 指向证书

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }
}
```

配好以后，浏览器打开这个域名就是网页端；App 第一次登录时在“服务器地址”里填这个域名。

## 升级

```sh
docker compose pull && docker compose up -d
```

App 不用手动升级：它们从登录的服务器取更新，服务端升级后，Android 的网页部分下次打开时生效，需要换安装包时会提示；桌面 App 在后台下载，重启后生效。见[总体设计](design.md#app-更新)。

## 备份和恢复

服务端每天在 `data/snapshots/` 留一份数据库快照，保留最近 7 份。另外备份时复制整个 `data/` 即可。恢复：

```sh
docker compose stop
cp data/snapshots/snapshot-<时间>.db data/nexttodo.db
rm -f data/nexttodo.db-wal data/nexttodo.db-shm
docker compose start
```

## 用部署脚本

从源码构建并经 ssh 部署到自己的服务器，不经过 GHCR：

```sh
NEXTTODO_HOST=myserver NEXTTODO_DIR=/srv/nexttodo NEXTTODO_PORT=8080 deploy/deploy.sh
```

- 先运行 `deploy/build.sh`（见[构建发布](#构建发布)），再按服务器的架构打出镜像，用 `docker save | ssh <主机> docker load` 传过去。
- 三个环境变量都要给出：ssh 主机名、服务器上的绝对路径（只含字母、数字和 `/._-`）、本机监听的端口。端口没有默认值，免得部署时把正在运行的服务换到反向代理不知道的端口。
- 服务器上的 `NEXTTODO_DIR` 里放 `compose.yaml`、`.env` 和 `data/`。`deploy/switch.sh` 负责换版本：先检查新配置，留着旧的 `compose.yaml` 和 `.env`（`.env` 只换 `NEXTTODO_IMAGE` 和 `NEXTTODO_PORT`，其他设置如 `NEXTTODO_SECRET_KEY` 保持不变，文件只有 root 能读），新版本启动后 30 秒内首页没有响应就换回上一个。

## 构建发布

发布在 Mac（Apple 芯片）上构建，一次产出服务端镜像的内容和各端安装包。

**需要**：

| 用途 | 工具 |
|---|---|
| 服务端和命令行（Linux amd64、arm64） | Rust，`rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl`，`cargo-zigbuild` 和 Zig |
| 网页端 | Node.js 和 pnpm |
| Android | JDK 21、Android SDK 和 NDK，`rustup target add aarch64-linux-android`，`cargo-ndk`，见 [Android](android.md#构建和签名) |
| macOS 和 Windows | 见[桌面 App](desktop.md#构建) |
| 镜像 | Docker（推多架构镜像要 buildx），`jq` |

**签名密钥**：从环境变量读取，不进仓库。

| 环境变量 | 内容 |
|---|---|
| `NEXTTODO_KEYSTORE`、`NEXTTODO_KEYSTORE_PASSWORD` | Android 正式版的密钥库和密码 |
| `TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 桌面 App 更新密钥的私钥文件和密码，公钥在 `tauri.conf.json` |

用自己的密钥构建时，要把 `tauri.conf.json` 里的公钥换成自己的。已经装了正式发布版的设备收不到这样构建的更新，因为签名不同。

**脚本**：

| 命令 | 做什么 |
|---|---|
| `deploy/build.sh` | 构建全部，版本 `0.1.<提交数>`。`target/deploy` 是镜像的内容（两种架构的服务端、网页和 `/app/` 下的更新），`target/release-assets` 是安装包、各平台的命令行 `nexttodo-*`、本机模式用的更新（`android.json`、`desktop.json` 和 `NextTodo.app.tar.gz`）和 `SHA256SUMS` |
| `deploy/release.sh <版本说明.md>` | 发布已推送的提交：镜像推到 GHCR（版本号和 `latest` 两个标签），安装包传到 GitHub Releases（标签 `v<版本号>`）。要先 `docker login ghcr.io` 并登录 `gh` |
| `deploy/deploy.sh` | 见[用部署脚本](#用部署脚本) |

- 三个脚本都要求工作区干净，版本说明文件放在仓库外面。
- 安装包的文件名带版本号：`NextTodo-<版本号>-android.apk`、`NextTodo-<版本号>-macos-arm64.dmg`、`NextTodo-<版本号>-windows-x64-setup.exe`。
