# 部署服务器

要在多台设备之间同步，或者在浏览器里使用网页版，需要一台自己的服务器。服务端和网页版打包在同一个 Docker 镜像里，部署一次，网页、Android、macOS 和 Windows 都能连上。

只在一台设备上用的，不需要服务器，见[在本机使用](./local)。

## 需要准备

- **一台 Linux 服务器**：云服务器（VPS）、家里的 NAS 或小主机都可以，amd64 或 arm64。NextTodo 占用很少，配置要求不高。
- **Docker**：按 [Docker 官方文档](https://docs.docker.com/engine/install/)安装 Docker Engine，它自带 Docker Compose。NAS 的系统里一般叫 Container Manager 或“容器”。
- **受信任的 HTTPS 证书**：Android App 和网页版只能通过 HTTPS 连接服务器，证书要是手机和浏览器默认信任的，比如 Let's Encrypt 签发的；自己签发的证书手机不认，所以没有受信任证书的地址都连不上。证书一般给域名申请：
  - 服务器在公网上：在域名服务商那里添加一条 A 记录，把域名（比如 `todo.example.com`）指向服务器的公网 IP。在中国大陆的服务器上用域名提供网站，需要先完成备案。
  - 没有域名，或者域名暂时用不了（比如还在备案）：可以直接用服务器的公网 IP，Let's Encrypt 也给公网 IP 签证书，见[只用 IP 地址](#ip)。
  - 只在家里的局域网里用：同样需要一个域名和受信任的证书，比如用 DNS 验证的方式申请证书，再在路由器或局域网 DNS 里把域名指向这台机器。
- **一个反向代理**：负责 HTTPS，把请求转给 NextTodo。Caddy、Nginx、OpenResty、1Panel 或宝塔面板里的网站、NAS 自带的反向代理都可以，服务器上已经有的就直接用它，要求见[第 2 步](#proxy)。
- **放行端口**：在防火墙（云服务器还有安全组）里放行反向代理用的端口，通常是 443；用 HTTP 验证的方式自动申请证书（Caddy 默认就是）还要放行 80。

下面的命令都在服务器的终端里运行（用 SSH 登录服务器）。

## 1. 启动 NextTodo {#start}

```sh
mkdir nexttodo && cd nexttodo
curl -fsSLO https://raw.githubusercontent.com/JunieXD/NextTodo/main/deploy/compose.yaml
mkdir data && sudo chown 65534:65534 data
docker compose up -d
```

这几行依次：建一个目录，下载配置文件，建好存放数据的 `data` 目录（NextTodo 在容器里以 65534 号用户运行，目录要归它），启动 NextTodo。数据库和每日快照都在 `data` 里。

检查它是否在运行：

```sh
curl http://127.0.0.1:8080/api/v1/me
```

返回 `{"code":"unauthenticated","message":"请先登录"}` 就说明 NextTodo 已经启动，只是还没登录。

NextTodo 默认只在这台机器内部的 `127.0.0.1:8080` 上运行，外面访问不到，由同一台机器上的反向代理转给它：

- **换端口**：8080 被占用时，在 `nexttodo` 目录里建一个 `.env` 文件，写上 `NEXTTODO_PORT=8790`，再运行 `docker compose up -d`。下面的 8080 都换成新端口。
- **反向代理连不到 `127.0.0.1`**：反向代理在另一台机器上，或者它自己也跑在 Docker 容器里（且不是 host 网络）时，它那边的 `127.0.0.1` 指的不是这台机器。这时把 `compose.yaml` 里 `ports` 一行开头的 `127.0.0.1` 换成反向代理能访问到的地址（比如这台机器的局域网 IP），再运行 `docker compose up -d`。这个端口是不加密的 HTTP，不要对公网开放。

## 2. 配置反向代理和 HTTPS {#proxy}

不管用哪个反向代理，NextTodo 只要求这几点：

| 要求 | 说明 |
|---|---|
| 用 HTTPS 提供，证书受信任 | 浏览器打开 `https://你的域名`（或 IP）时，地址栏没有证书警告 |
| 整个网站转发到 `http://127.0.0.1:8080` | 要单独占一个域名、子域名或 IP，不能放在某个路径下（如 `example.com/todo`） |
| 支持 WebSocket | 各设备靠 `/api/v1/sync` 上的 WebSocket 实时同步。NextTodo 每 30 秒发一次心跳，反向代理的空闲超时不能短于 30 秒（Nginx 默认的 60 秒就够） |
| 把原来的 `Host` 传过去 | 或者设置 `X-Forwarded-Host`。网页版的同步连接靠它确认是同一个网站 |

下面是常见的几种写法，域名都换成自己的。

### Caddy

Caddy 会自动申请和续期证书，上面的要求默认都满足。按 [Caddy 官方文档](https://caddyserver.com/docs/install)安装后，在 `/etc/caddy/Caddyfile` 里加上：

```
todo.example.com {
	reverse_proxy 127.0.0.1:8080
}
```

运行 `sudo systemctl reload caddy` 让配置生效。

### Nginx 或 OpenResty

OpenResty 基于 Nginx，写法相同。证书用 certbot 等工具申请，或者用已有的：

```nginx
server {
    listen 443 ssl;
    server_name todo.example.com;
    ssl_certificate     /path/to/fullchain.pem;
    ssl_certificate_key /path/to/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }
}
```

`proxy_set_header Host` 传原来的 `Host`，后三行开启 WebSocket。改完运行 `sudo nginx -s reload`（OpenResty 是 `sudo openresty -s reload`）。

### 面板和 NAS 自带的反向代理

1Panel、宝塔、群晖这类面板通常有“反向代理”或“网站”功能。新建一个，按上面的表格填写：目标（后端）地址填 `http://127.0.0.1:8080`，打开 WebSocket 支持，申请或上传证书。有的面板没有 WebSocket 开关，要在自定义配置里加上和上面 Nginx 一样的 `Upgrade`、`Connection` 两行。具体在哪里设置，见面板自己的文档。

### 只用 IP 地址 {#ip}

没有能用的域名时，可以给服务器的公网 IP 申请 Let's Encrypt 的证书。这种证书的有效期只有 6 天左右，要靠程序自动续期，用 Caddy（2.11 或更新的版本）最省事。Caddyfile 写成：

```
{
	default_sni 203.0.113.10
}

203.0.113.10 {
	tls {
		issuer acme {
			profile shortlived
		}
	}
	reverse_proxy 127.0.0.1:8080
}
```

- 两处 `203.0.113.10` 都换成服务器的公网 IP。只能用公网 IP，`192.168.x.x` 这类局域网地址申请不到；公网 IP 变了要改配置。
- `profile shortlived` 申请 IP 证书要用的短期证书。Caddy 在到期前自己续期，保持它一直运行就行。
- `default_sni` 不能省：按 IP 访问时，App 和浏览器不告诉服务器要访问哪个名字，云服务器上 Caddy 看到的本机地址又往往是内网 IP，没有这一行它就不知道该用哪张证书，证书申请下来了，连接照样失败。
- Let's Encrypt 验证时要从外网按 IP 连到服务器的 80 或 443 端口，防火墙和安全组要放行这两个端口。
- App 里的服务器地址填这个 IP，比如 `203.0.113.10`。

也可以用别的工具申请，前提是它支持 Let's Encrypt 的 `shortlived` 证书档，能每隔几天自动续期，续期后让反向代理加载新证书，比如 certbot 5.3 起的 `--ip-address`。

### 检查

用浏览器打开 `https://todo.example.com`（只用 IP 的打开 `https://` 加 IP），能看到 NextTodo 的登录页、地址栏没有证书警告，就成功了。WebSocket 是否正常，要在下一步登录以后才看得出来，见[遇到问题](#troubleshooting)。

## 3. 创建账号并登录

回到 `nexttodo` 目录，创建管理员账号。下面的 `admin` 是用户名，可以换成自己想用的（字母、数字和下划线），然后按提示输入两次密码：

```sh
docker compose exec nexttodo /nexttodo-server create-admin admin
```

然后在 App 里点“连接服务器”，服务器地址填域名（如 `todo.example.com`）或 IP，再填用户名和密码登录。浏览器里打开这个地址就是网页版。

谁忘了密码（包括管理员自己），在 `nexttodo` 目录里运行下面的命令，把 `admin` 换成那个人的用户名，再按提示输入两次新密码。这个账号的设备都要用新密码重新登录，API 令牌不受影响。

```sh
docker compose exec nexttodo /nexttodo-server reset-password admin
```

提示认不出 `reset-password` 时，说明镜像还是旧的（在设置里升级过），先按[用命令升级](#用命令升级)拉取新镜像。

## 4. 邀请其他人

每人一个账号，数据彼此独立。管理员在“设置 → 邀请注册”里点“生成邀请码”，邀请链接会自动复制，发给对方即可。对方打开链接，或者在登录页选“邀请码注册”并填入邀请码，就能注册。每个邀请码只能注册一个账号。

## 5. 让服务器替你整理 {#ai}

[AI 整理](./ai)的配置默认只能保存在各自的设备上，整理时设备要联网。想让服务器自己整理、提交后设备可以离线，要给服务器设一个加密密钥：

1. 在 `nexttodo` 目录里运行下面的命令，生成密钥并写进 `.env` 文件：

   ```sh
   echo "NEXTTODO_SECRET_KEY=$(openssl rand -base64 32)" >> .env
   ```

2. 运行 `docker compose up -d` 让它生效。
3. 在 App 的“设置 → AI 整理”里，保存位置选“同步到服务器”。

服务器用这个密钥加密保存大家的 API 密钥。`.env` 文件要保管好；换掉或丢了它，已保存的 API 密钥就解不开，要重新填写。

## 升级

出了新版本时，管理员打开 NextTodo 会看到提示条“服务器可升级到 0.1.N”，“设置 → 关于”里也写着。想第一时间知道，可以在 [GitHub 仓库](https://github.com/JunieXD/NextTodo)右上角点 **Watch** → **Custom**，勾选 **Releases** 后点 **Apply**，发布新版本时 GitHub 会发邮件通知。

升级有两种方法，结果一样，可以混用。数据会自动升级，不用另外操作。App 不用手动升级：服务器升级后，各设备的 App 会自动获取新版本。

### 在设置里升级

在“设置 → 关于”的“服务器版本”一行点 **升级**，确认后，服务器自己下载新版本并换上，这一行显示下载的进度。换上时各设备会断开几秒，随后自动重连，网页会自动刷新。新版本起不来时，服务器自动退回原来的版本，数据不受影响。

- 服务器要能连上 GHCR（`ghcr.io`），连不上时用命令升级。
- 这一行没有 **升级** 按钮的，是服务器的版本太旧：用命令升级一次，以后就能在设置里升级了。
- 在 `.env` 里用 `NEXTTODO_IMAGE` 固定了版本的，在设置里升级以后，服务器用的是新版本。要换回旧版本，见[部署文档](https://github.com/JunieXD/NextTodo/blob/main/docs/deploy.md#在设置里升级)。

### 用命令升级

在服务器上的 `nexttodo` 目录里运行：

```sh
docker compose pull && docker compose up -d
```

如果在 `.env` 里写了 `NEXTTODO_IMAGE` 固定版本，先把其中的版本号改成新的。

## 备份

服务器每天在 `data/snapshots/` 里保存一份数据库快照，保留最近 7 份。把这些快照定期复制到别处，就是备份。从快照恢复的步骤见[部署文档](https://github.com/JunieXD/NextTodo/blob/main/docs/deploy.md#备份和恢复)。

## 遇到问题 {#troubleshooting}

先看 NextTodo 的日志：在 `nexttodo` 目录里运行 `docker compose logs --tail 100`。

| 现象 | 多半是 | 检查 |
|---|---|---|
| 网页打不开 | 域名、端口或反向代理 | 域名是否指向这台服务器，端口是否放行，反向代理是否在运行，它的错误日志 |
| 502 Bad Gateway | 反向代理连不到 NextTodo | NextTodo 是否在运行（[第 1 步](#start)的 `curl`），反向代理转发的地址和端口对不对；反向代理在容器里时见第 1 步最后一条 |
| 浏览器提示证书不安全，App 连接失败 | 证书不受信任或过期 | 换成受信任的证书，比如 Let's Encrypt；只能用 IP 访问的见[只用 IP 地址](#ip) |
| 只用 IP，证书申请下来了，但浏览器和 App 都连不上 | 没设 `default_sni` | 见[只用 IP 地址](#ip) |
| 能登录，但一直显示“离线”，别的设备的改动要刷新才出现 | WebSocket 没打通 | 反向代理是否开了 WebSocket；网页版还要检查是否传了原来的 `Host` |
| App 里填服务器地址后连接失败，浏览器能打开 | 地址写错 | 只填域名（如 `todo.example.com`）或 IP，不写路径；不写协议时按 `https://` |
