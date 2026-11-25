# 部署服务器

要多台设备实时同步、在浏览器里使用网页版，或者和别人共享文件夹，需要一台自己的服务器。服务端和网页版打包在同一个 Docker 镜像里，部署一次，网页、Android、iOS、macOS 和 Windows 都能连上。

不想部署服务器的，App 可以只在本机使用，几台设备之间借网盘同步，见[在本机使用](./local)。

## 需要准备

- **一台 Linux 服务器**：云服务器（VPS）、家里的 NAS 或小主机都可以，amd64 或 arm64。NextTodo 占用很少，配置要求不高。
- **Docker**：按 [Docker 官方文档](https://docs.docker.com/engine/install/)安装 Docker Engine，它自带 Docker Compose。NAS 的系统里一般叫 Container Manager 或“容器”。
- **受信任的 HTTPS 证书**：HTTPS 加密 App 和服务器之间传的密码和数据，经公网访问时最好配上。证书要是手机和浏览器默认信任的，比如 Let's Encrypt 签发的；自己签发的证书手机不认，用这种证书的地址连不上。证书一般给域名申请：
  - 服务器在公网上：在域名服务商那里添加一条 A 记录，把域名（比如 `todo.example.com`）指向服务器的公网 IP。在中国大陆的服务器上用域名提供网站，需要先完成备案。
  - 没有域名，或者域名暂时用不了（比如还在备案）：可以直接用服务器的公网 IP，Let's Encrypt 也给公网 IP 签证书，见[只用 IP 地址](#ip)。
  - 只在家里的局域网里用：可以不要证书，[直接用 HTTP](#http)，下面的反向代理和放行端口都不用准备。要用 HTTPS 的，同样需要一个域名和受信任的证书，比如用 DNS 验证的方式申请证书，再在路由器或局域网 DNS 里把域名指向这台机器。
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

最后一步拉取镜像很慢或者超时的，多半是服务器连不上镜像所在的 GHCR（`ghcr.io`），在中国大陆很常见。可以改从国内的 GHCR 镜像站拉取：按 Ctrl+C 停下，打开 `nexttodo` 目录里的 `.env` 文件（没有就新建一个，比如运行 `nano .env`），另起一行写上下面这行，保存后再运行 `docker compose up -d`。

```sh
NEXTTODO_IMAGE=ghcr.nju.edu.cn/juniexd/nexttodo:latest
```

- `.env` 里已经有 `NEXTTODO_IMAGE` 的（比如固定了版本），不要另加一行，只把镜像地址里的域名换成 `ghcr.nju.edu.cn`，镜像名和冒号后的版本号不变。
- 这是南京大学的镜像站，它拉不下来时换成毫秒镜像的 `ghcr.1ms.run`。镜像站是第三方提供的，以后可能失效。
- 服务器能用代理的，也可以[给 Docker 配上代理](https://docs.docker.com/engine/daemon/proxy/)，直接从 `ghcr.io` 拉取。这样 `.env` 里不用写这一行，已经写了的把域名改回 `ghcr.io`。
- 镜像站上的 `latest` 可能过一阵才换成新版本。[用命令升级](#用命令升级)时拉到的还是旧版本，就把这一行末尾的 `latest` 换成新的版本号。这样就固定在这个版本了，以后每次升级都要改版本号。
- NextTodo 的镜像不在 Docker Hub 上。Docker 设置里的 `registry-mirrors` 只加速 Docker Hub，对 `ghcr.io` 不起作用；把域名换成 `docker.1ms.run` 这类只加速 Docker Hub 的地址，会报 `manifest unknown`。

检查它是否在运行：

```sh
curl http://127.0.0.1:8080/api/v1/me
```

返回 `{"code":"unauthenticated","message":"请先登录"}` 就说明 NextTodo 已经启动，只是还没登录。

NextTodo 默认只在这台机器内部的 `127.0.0.1:8080` 上运行，外面访问不到，由同一台机器上的反向代理转给它：

- **换端口**：8080 被占用时，在 `nexttodo` 目录里的 `.env` 文件（没有就新建）里另起一行写上 `NEXTTODO_PORT=8790`，再运行 `docker compose up -d`。下面的 8080 都换成新端口。
- **反向代理连不到 `127.0.0.1`**：反向代理在另一台机器上，或者它自己也跑在 Docker 容器里（且不是 host 网络）时，它那边的 `127.0.0.1` 指的不是这台机器。这时把 `compose.yaml` 里 `ports` 一行开头的 `127.0.0.1` 换成反向代理能访问到的地址（比如这台机器的局域网 IP），再运行 `docker compose up -d`。这个端口是不加密的 HTTP，不要对公网开放。

## 2. 配置反向代理和 HTTPS {#proxy}

只在局域网里用、不配证书的，跳过这一步，见[直接用 HTTP](#http)。

不管用哪个反向代理，NextTodo 只要求这几点：

| 要求 | 说明 |
|---|---|
| 用 HTTPS 提供，证书受信任 | 浏览器打开 `https://你的域名`（或 IP）时，地址栏没有证书警告 |
| 整个网站转发到 `http://127.0.0.1:8080` | 要单独占一个域名、子域名或 IP，不能放在某个路径下（如 `example.com/todo`） |
| 支持 WebSocket | 各设备靠 `/api/v1/sync` 上的 WebSocket 实时同步。NextTodo 每 30 秒发一次心跳，反向代理的空闲超时不能短于 30 秒（Nginx 默认的 60 秒就够） |
| 把原来的 `Host` 传过去 | 或者设置 `X-Forwarded-Host`。网页版的同步连接靠它确认是同一个网站 |
| 请求体允许 30 MB | 附件最大 20 MB，agent 用 base64 上传时约 27 MB。Nginx 默认只收 1 MB，要像下面那样设 `client_max_body_size`；Caddy 默认不限制 |

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

    # 附件最大 20 MB，Nginx 默认只收 1 MB 的请求体
    client_max_body_size 30m;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }
}
```

`client_max_body_size` 让附件能上传（最大 20 MB，Nginx 默认只收 1 MB），`proxy_set_header Host` 传原来的 `Host`，后三行开启 WebSocket。改完运行 `sudo nginx -s reload`（OpenResty 是 `sudo openresty -s reload`）。

### 面板和 NAS 自带的反向代理

1Panel、宝塔、群晖这类面板通常有“反向代理”或“网站”功能。新建一个，按上面的表格填写：目标（后端）地址填 `http://127.0.0.1:8080`，打开 WebSocket 支持，申请或上传证书。有的面板没有 WebSocket 开关，要在自定义配置里加上和上面 Nginx 一样的 `Upgrade`、`Connection` 两行。上传大小的限制也要放到 30 MB 以上（Nginx 系的面板就是 `client_max_body_size`），否则较大的附件传不上去。具体在哪里设置，见面板自己的文档。

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

### 直接用 HTTP {#http}

只在家里的局域网，或 Tailscale、ZeroTier 这类组网里用时，可以不要证书和反向代理，直接用 HTTP 连 NextTodo：

1. 把 `compose.yaml` 里 `ports` 一行开头的 `127.0.0.1` 换成这台机器的局域网 IP（用 Tailscale 这类组网的，换成组网里的 IP），比如 `- "192.168.1.10:${NEXTTODO_PORT:-8080}:8080"`，再运行 `docker compose up -d`。这样只有这个网络里的设备连得上；这台机器最好在路由器里设成固定 IP。不要删掉整个 IP：那样会在所有网卡上监听，机器有公网地址时，公网也能连上这个不加密的端口。
2. 浏览器打开 `http://` 加这台机器的局域网 IP 和端口，比如 `http://192.168.1.10:8080`。App 里的服务器地址填 `192.168.1.10:8080` 就行，局域网的地址不写协议时按 `http://` 连。

HTTP 不加密：同一网络里的人能看到你的密码和待办，也能篡改。只在信任的网络里这样用。经公网访问的最好配上 HTTPS；一定要用 HTTP 的，第 1 步里 `ports` 一行开头不写 IP（`- "${NEXTTODO_PORT:-8080}:8080"`），云服务器的安全组还要放行这个端口，App 里的服务器地址要写上 `http://`（比如 `http://203.0.113.10:8080`）。

网页版经 HTTP 打开时，断网后刷新就打不开页面（已经打开的照常可以离线用），也不能拷贝图片。

### 检查

用浏览器打开 `https://todo.example.com`（只用 IP 的打开 `https://` 加 IP；[直接用 HTTP](#http) 的打开 `http://` 加 IP 和端口），能看到 NextTodo 的登录页就成功了。用 HTTPS 的，地址栏还要没有证书警告。WebSocket 是否正常，要在下一步登录以后才看得出来，见[遇到问题](#troubleshooting)。

## 3. 创建账号并登录

回到 `nexttodo` 目录，创建管理员账号。下面的 `admin` 是用户名，可以换成自己想用的（字母、数字和下划线），然后按提示输入两次密码：

```sh
docker compose exec nexttodo /nexttodo-server create-admin admin
```

然后在 App 里点“连接服务器”，服务器地址填浏览器里打开的那个地址（如 `todo.example.com`），再填用户名和密码登录。[直接用 HTTP](#http) 的，局域网地址可以不写 `http://`，公网地址要写上。浏览器里打开这个地址就是网页版。

谁忘了密码（包括管理员自己），在 `nexttodo` 目录里运行下面的命令，把 `admin` 换成那个人的用户名，再按提示输入两次新密码。这个账号的设备都要用新密码重新登录，API 令牌不受影响。

```sh
docker compose exec nexttodo /nexttodo-server reset-password admin
```

提示认不出 `reset-password` 时，说明镜像还是旧的（在设置里升级过），先按[用命令升级](#用命令升级)拉取新镜像。

## 4. 邀请其他人 {#invite}

每人一个账号，数据彼此独立，要一起做的事可以[共享文件夹](./share)。管理员在“设置 → 服务器”的“邀请注册”一组里点“生成邀请码”，邀请链接会自动复制，发给对方即可。对方打开链接，或者在登录页选“邀请码注册”并填入邀请码，就能注册。每个邀请码只能注册一个账号。

## 5. 让服务器替你整理 {#ai}

[AI 整理](./ai)的配置默认只能保存在各自的设备上，整理时设备要联网。想让服务器自己整理、提交后设备可以离线，要给服务器设一个加密密钥：

1. 在 `nexttodo` 目录里运行下面的命令，生成密钥并写进 `.env` 文件：

   ```sh
   echo "NEXTTODO_SECRET_KEY=$(openssl rand -base64 32)" >> .env
   ```

2. 运行 `docker compose up -d` 让它生效。
3. 在 App 的“设置 → AI 整理”里，保存位置选“同步到服务器”。

服务器用这个密钥加密保存大家的 API 密钥。`.env` 文件要保管好；换掉或丢了它，已保存的 API 密钥就解不开，要重新填写。

### 用内网里的模型 {#lan-model}

服务器默认只访问公网上 HTTPS 的模型接口，免得有人借它访问你的内网。想让它用同一台机器或局域网里的 [Ollama 这类模型](./ai#ollama)：

1. 在 `compose.yaml` 的 `environment` 下面加一行 `NEXTTODO_AI_PRIVATE_NETWORK: "true"`，和 `RUST_LOG` 对齐。
2. 运行 `docker compose up -d` 让它生效。

打开以后，这台服务器上的每个用户都能让它访问内网里的地址，只在信任所有用户时打开。

NextTodo 在容器里，`127.0.0.1` 指的是容器自己。同一台机器上的 Ollama，接口地址要填这台机器的局域网 IP，比如 `http://192.168.1.10:11434/v1`，还要让 Ollama 监听这个地址（设环境变量 `OLLAMA_HOST=192.168.1.10`）。Ollama 没有密码，不要让它监听公网地址或所有地址（`0.0.0.0`），否则别人也能用它。

## 升级

出了新版本时，管理员打开 NextTodo 会看到提示条“服务器可升级到 0.1.N”，“设置 → 服务器”里也写着。想第一时间知道，可以在 [GitHub 仓库](https://github.com/JunieXD/NextTodo)右上角点 **Watch** → **Custom**，勾选 **Releases** 后点 **Apply**，发布新版本时 GitHub 会发邮件通知。

升级有两种方法，结果一样，可以混用。数据会自动升级，不用另外操作。Android 和电脑上的 App 不用手动升级：服务器升级后，它们会自动获取新版本。iOS 的 App 在 SideStore 里更新，见[下载和安装](./install#更新)。App 比服务器新时（比如 iOS 的 App 先更新了），不是管理员的人会看到提示“服务器的版本比 App 旧，部分改动可能无法保存，请联系管理员升级服务器”。

### 在设置里升级

在“设置 → 服务器”的“服务器版本”一行点 **升级**，确认后，服务器自己下载新版本并换上，这一行显示下载的进度。换上时各设备会断开几秒，随后自动重连，网页会自动刷新。新版本起不来时，服务器自动退回原来的版本，数据不受影响。

- 服务器要能连上 GHCR（`ghcr.io`），连不上时用命令升级。设置里的升级不看 `.env` 里的 `NEXTTODO_IMAGE`，总是从 GHCR 下载，改从[镜像站](#start)拉取的也一样。
- 这一行没有 **升级** 按钮的，是服务器的版本太旧：用命令升级一次，以后就能在设置里升级了。
- 在 `.env` 里用 `NEXTTODO_IMAGE` 固定了版本的，在设置里升级以后，服务器用的是新版本。要换回旧版本，见[部署文档](https://github.com/JunieXD/NextTodo/blob/main/docs/deploy.md#在设置里升级)。

### 用命令升级

在服务器上的 `nexttodo` 目录里运行：

```sh
docker compose pull && docker compose up -d
```

如果在 `.env` 里写了 `NEXTTODO_IMAGE` 固定版本，先把其中的版本号改成新的。

## 备份

服务器每天在 `data/snapshots/` 里保存一份数据库快照，保留最近 7 份。附件的文件不在快照里，在 `data/files/`。把这些快照和 `data/files/` 定期复制到别处，就是备份。从快照恢复的步骤见[部署文档](https://github.com/JunieXD/NextTodo/blob/main/docs/deploy.md#备份和恢复)。

## 遇到问题 {#troubleshooting}

先看 NextTodo 的日志：在 `nexttodo` 目录里运行 `docker compose logs --tail 100`。

| 现象 | 多半是 | 检查 |
|---|---|---|
| 拉取镜像很慢或者超时 | 连不上 GHCR（`ghcr.io`） | 改从 GHCR 的镜像站拉取，见[第 1 步](#start) |
| 拉取镜像时报 `manifest unknown` | 那个地址上没有这个镜像：用了只加速 Docker Hub 的地址，或者镜像站上还没有这个版本 | 核对 `.env` 里的 `NEXTTODO_IMAGE`，见[第 1 步](#start) |
| 网页打不开 | 域名、端口或反向代理 | 域名是否指向这台服务器，端口是否放行，反向代理是否在运行，它的错误日志 |
| 502 Bad Gateway | 反向代理连不到 NextTodo | NextTodo 是否在运行（[第 1 步](#start)的 `curl`），反向代理转发的地址和端口对不对；反向代理在容器里时见第 1 步最后一条 |
| 浏览器提示证书不安全，App 连接失败 | 证书不受信任或过期 | 换成受信任的证书，比如 Let's Encrypt；只能用 IP 访问的见[只用 IP 地址](#ip) |
| 只用 IP，证书申请下来了，但浏览器和 App 都连不上 | 没设 `default_sni` | 见[只用 IP 地址](#ip) |
| 能登录，但一直显示“离线”，别的设备的改动要刷新才出现 | WebSocket 没打通 | 反向代理是否开了 WebSocket；网页版还要检查是否传了原来的 `Host` |
| 网页直接用 HTTP 登录后，马上又回到登录页 | 这个地址以前用 HTTPS 登录过，浏览器留着只走 HTTPS 的旧登录信息，HTTP 的页面换不掉它 | 先用 `https://` 打开并退出登录；HTTPS 已经停用的，在浏览器的网站设置里删掉这个网站的 Cookie |
| App 里填服务器地址后连接失败，浏览器能打开 | 地址写错 | 只填域名（如 `todo.example.com`）或 IP，不写路径；IPv6 地址要加方括号，如 `[fd00::1]:8080`。不写协议时，局域网的地址按 `http://`，其余按 `https://`；[直接用 HTTP](#http) 的公网地址要写上 `http://` |
