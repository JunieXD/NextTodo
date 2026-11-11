# 部署

NextTodo 的服务端和网页端在同一个 Docker 镜像里，部署一次，网页、Android、macOS 和 Windows 就都能连上。决策见[总体设计](design.md#部署)。

## 用 Docker 部署

需要一台装了 Docker 和 Docker Compose 的 Linux 服务器（amd64 或 arm64），以及受信任的 HTTPS 证书：一般给域名申请，没有能用的域名时也可以给公网 IP 申请 Let's Encrypt 的短期证书，配置见官网的[只用 IP 地址](https://juniexd.github.io/NextTodo/guide/server#ip)。

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

反向代理负责 HTTPS，要开启 WebSocket，并把原来的 `Host` 传给服务端（或设 `X-Forwarded-Host`），网页的同步连接靠它确认是同源页面。整个域名（或 IP）都要转给 NextTodo：网页的资源和接口都从根路径开始，不能放在某个路径下。面板、NAS 和反向代理在容器里时的做法和排错，见官网的[部署服务器](https://juniexd.github.io/NextTodo/guide/server#proxy)。

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

配好以后，浏览器打开这个域名就是网页端；App 第一次登录时在“服务器地址”里填这个域名（只用 IP 的填 IP）。

## 升级

有新版本时，管理员在网页和 App 的设置“关于”里会看到提示，见[总体设计](design.md#部署)。两种方法，结果一样，可以混用：

- **在设置里升级**：点“可升级到 0.1.N”旁边的“升级”，见[下文](#在设置里升级)。
- **用命令**：在服务器上运行下面的命令；在 `.env` 里固定了版本的，先把 `NEXTTODO_IMAGE` 改成新的版本号。

  ```sh
  docker compose pull && docker compose up -d
  ```

数据库在新版本启动时自动迁移。App 不用手动升级：它们从登录的服务器取更新，服务端升级后，Android 的网页部分下次打开时生效，需要换安装包时会提示；桌面 App 在后台下载，重启后生效。见[总体设计](design.md#app-更新)。

### 在设置里升级

服务端自己下载新版本、换上，不经过 Docker：容器里的服务端碰不到宿主机的 Docker，把 `docker.sock` 交给它等于交出整台服务器。代码在 `crates/server/src/upgrade.rs`。

1. **下载**：管理员点“升级”并确认后，服务端从 GHCR 匿名拉取这个版本的镜像 `ghcr.io/juniexd/nexttodo:<版本号>` 里服务器架构的那一份，和 `docker compose pull` 拉的是同一份，约 45 MB。按 OCI 的分发协议：先取镜像索引，挑出 `linux/<架构>` 的清单，再取清单里的两层（服务端程序和网页），令牌在仓库回 401 时按它给的地址取。清单和每一层下载完都按 SHA-256 摘要校验，再解压进 `data/versions/<版本号>.part/`：只能有 `nexttodo-server` 这个文件和 `web/` 下的普通文件和目录，有别的路径或文件类型就整个作废，缺了 `nexttodo-server` 或 `web/index.html` 也作废。最后运行一次新的 `nexttodo-server --version`，报出的版本号对得上，再把这些文件同步到磁盘。这期间照常服务。下载好的版本在记下切换以前一直叫 `.part`，镜像里的服务端不会交给它：这时它起不来，没有可以退回的副本。
2. **切换**：像收到 SIGTERM 时一样停止服务，进行中的请求最多等 10 秒（和 Compose 停容器时一样，等模型的请求可能更久，期间谁都连不上）。然后拿住数据库的连接一直到换掉进程，还没结束的请求、同步连接和后台任务都写不进去，副本之后就不会再有确认过的改动：把数据库复制一份到 `data/snapshots/before-<版本号>.db`（只留最近一次升级的，不计入每天的 7 份）；在 `data/versions/trying` 记下要试的版本和这份副本；再把 `<版本号>.part` 改名为 `<版本号>`。每一步都同步到磁盘（连同所在目录）再做下一步，断电也不会乱了顺序。最后用 `exec` 把进程换成新版本的服务端，参数不变。进程和容器都不退出，各设备断开几秒后自动重连。
3. **确认**：新版本迁移完数据库、开始监听后，处理请求前，删掉 `trying` 并同步到磁盘，升级就算成功；`data/versions/` 里别的版本这时删掉。从 `data/versions/` 运行的服务端，用它旁边的 `web/` 提供网页，不管 `--web` 和 `NEXTTODO_WEB`。
4. **以后每次启动**：镜像里的服务端不论运行的是哪个命令（`serve`、`create-admin`、`restore` 都是），先看有没有 `trying`，有就先退回（见下表最后一行，要独占数据目录锁，服务端正在运行时报错）；再看 `data/versions/`，有比自己新的版本，就用 `exec` 交给它，所以 `docker compose exec nexttodo /nexttodo-server create-admin` 照样能用；没有就自己运行，`serve` 时删掉 `data/versions/` 里不比自己新的版本（比如之后又用命令换了更新的镜像）。所以哪个新就用哪个。

**失败时**：

| 什么时候 | 怎么办 | “关于”里写 |
|---|---|---|
| 连不上 GHCR，或没有这个版本 | 删掉 `.part`，照常服务 | 升级失败：无法从 GHCR 下载新版本，请检查服务器的网络，或用命令升级 |
| 校验不符，或内容不对 | 同上 | 升级失败：下载的内容不完整，请重试 |
| 新版本运行不了（比如 `data/` 所在的盘不允许执行程序） | 同上 | 升级失败：新版本无法在这台服务器上运行，请用命令升级 |
| 写不进 `data/`（比如磁盘满了） | 同上 | 升级失败：存储空间不足（按原因写） |
| 切换时复制数据库失败（比如磁盘满了） | 拿着数据库的连接退出进程，记下 `data/versions/failed`（磁盘满时可能也记不下）。容器重新启动后照常运行原来的版本，删掉下载的 `.part` | 升级到 0.1.N 失败：没能换上新版本，服务器仍是 0.1.M，原因见服务器的日志（docker compose logs）。没记下的，正看着这一行的网页和 App 见到服务器回来、版本没变，也这样写 |
| 换上后没能确认（迁移失败、启动时崩溃） | 进程退出，容器按重启策略重新启动（`compose.yaml` 里是 `restart: unless-stopped`）。镜像里的服务端看到 `trying`，就把其中的副本原样放回（不经过 `restore`：副本可能来自比镜像新的版本，`restore` 会把它迁移到镜像的版本），删掉 `-wal` 和 `-shm`、新版本，在 `data/versions/failed` 记下它，再按第 4 步启动，回到升级前的版本。每一步也都同步到磁盘，中途断电，下次启动从头再退一次。新版本处理请求前就确认了，没确认就没有写入，不丢数据 | 同上 |

前四种可以直接重试。没有设重启策略的容器，最后一种情况下会停着，手动启动一次，同样会退回；网页和 App 等 3 分钟没等到服务器回来，就写“服务器没有重新启动，请在服务器上查看（docker compose ps）”。服务器重新回答以后，网页和 App 向它要版本号（`/app/desktop.json`），是新版本才算升级成功。

**限制**：

- 服务器要能连上 `ghcr.io`，连不上的用命令升级。要经代理连接的，在 `compose.yaml` 的 `environment` 里加上 `HTTPS_PROXY`。
- 只有 Linux 上带版本号构建（`deploy/build.sh`）的服务端能这样升级；开发版、本机模式和加上这个功能以前的版本没有“升级”按钮，照旧附升级方法的链接。
- 每个版本解压后约 60 MB，升级期间 `data/versions/` 里最多有两个。
- 想换回旧的镜像（比如用 `NEXTTODO_IMAGE` 固定一个旧版本）时，先删掉 `data/versions/`，否则启动时又会交给里面更新的版本；数据库已被新版本迁移过的，再用 `data/snapshots/before-<新版本号>.db` 恢复，见[备份和恢复](#备份和恢复)。
- 测试：`crates/server/tests/upgrade.rs` 用一个照 GHCR 行为的假仓库测下载和退回。换进程（`exec`）和容器的重启策略只能在真实的 Compose 部署上测：用本机的 `registry:2` 放两个版本的镜像，临时把 `upgrade::REGISTRY` 改成它的地址构建，试一次成功的升级和一次起不来的。

## 备份和恢复

服务端每天在 `data/snapshots/` 留一份数据库快照，保留最近 7 份。要另外备份，复制这些快照即可；运行中直接复制 `nexttodo.db` 不一定得到完整的副本。恢复：

```sh
docker compose stop
docker compose run --rm nexttodo restore /data/snapshots/snapshot-<时间>.db
docker compose start
```

- 快照先检查是不是完好的 NextTodo 数据库，不是就报错，什么都不改。现在的数据库损坏或者丢了也能恢复；它好好的却读不出来（权限、磁盘出错），或者正被别的程序读着，就报错停下。
- 被替换的数据库和它的 `-wal`、`-shm`，有哪个就原样复制哪个到 `data/before-restore-<Unix 秒>/`（已存在就加序号），不自动删除。
- `serve` 和 `create-admin` 运行时共享数据目录锁 `data/lock`，`restore` 要独占它：服务端没停就运行 `restore` 会报错，什么都不改。`sqlite3` 这类直接打开数据库的程序不拿这把锁，恢复前要先关掉。中途被打断时，再运行一次即可。
- 快照之后的改动、登录和 API 令牌都会消失，快照之后才登录的设备要重新登录。各设备下次连上时重新下载全部数据，还没发出的改动保留，照常发送。原理见[数据模型](data-model.md#mutations)。

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
- 安装包和命令行的文件名不带版本号（`NextTodo-android.apk`、`NextTodo-macos-arm64.dmg`、`NextTodo-windows-x64-setup.exe`、`nexttodo-<平台>`），`https://github.com/JunieXD/NextTodo/releases/latest/download/<文件名>` 总是下载最新版，官网、README 和 App 都用这个地址。

## 官网

官网 <https://juniexd.github.io/NextTodo/> 面向用户，介绍 NextTodo 并讲解怎么用；仓库的 `docs/` 是开发文档，官网用到其中的细节时链接过去。源码在 `apps/site`，用 VitePress 构建。

| 命令 | 做什么 |
|---|---|
| `cd apps/site && pnpm dev` | 本地预览 |
| `deploy/site.sh` | 发布：在本地构建，把构建结果作为一个提交推到 `gh-pages` 分支，GitHub Pages 从这个分支的根目录提供网页 |

- 不用 GitHub Actions 构建。发布要求工作区干净；和上次发布的内容相同时不提交。
- 截图用 `docs/images` 里的，和 README 共用一份。
- “使用教程”（`apps/site/guide/`）写给什么都不懂的用户：一步步讲清怎么做，按钮和菜单的叫法和 App 里的一致；设计和实现的细节留在 `docs/`，教程链接过去。App 里的帮助链接指向这些页面（`apps/web/src/ui/help.tsx` 的调用处），改页面路径或标题锚点时一起改。
- 站内搜索用浏览器的 `Intl.Segmenter` 把中文切成词（Chrome 87、Safari 14.1、Firefox 125 起），更早的浏览器里只有搜索用不了。
