# 部署服务器

要在多台设备之间同步，或者在浏览器里使用网页版，需要一台自己的服务器。服务端和网页版打包在同一个 Docker 镜像里，部署一次，网页、Android、macOS 和 Windows 都能连上。

只在一台设备上用的，不需要服务器，见[在本机使用](./local)。

## 需要准备

- **一台 Linux 服务器**：云服务商的云服务器（VPS）即可，amd64 或 arm64 都行。NextTodo 占用很少，配置要求不高。
- **一个域名**：在域名服务商那里添加一条 A 记录，把一个域名（比如 `todo.example.com`）指向服务器的公网 IP。在中国大陆的服务器上用域名提供网站，需要先完成备案。
- **放行端口**：在云服务商的安全组或防火墙里放行 80 和 443 端口，申请 HTTPS 证书和访问网页都要用到。
- **Docker**：按 [Docker 官方文档](https://docs.docker.com/engine/install/)在服务器上安装 Docker Engine，它自带 Docker Compose。

下面的命令都在服务器的终端里运行（用 SSH 登录服务器）。

## 1. 启动 NextTodo

```sh
mkdir nexttodo && cd nexttodo
curl -fsSLO https://raw.githubusercontent.com/JunieXD/NextTodo/main/deploy/compose.yaml
mkdir data && sudo chown 65534:65534 data
docker compose up -d
```

这几行依次：建一个目录，下载配置文件，建好存放数据的 `data` 目录，启动 NextTodo。数据库和每日快照都在 `data` 里。

NextTodo 现在只在服务器内部的 8080 端口上运行，外面还访问不到，下一步给它配上 HTTPS。

## 2. 配置 HTTPS

用 Caddy 最省事，它会自动申请和续期 HTTPS 证书。

1. 按 [Caddy 官方文档](https://caddyserver.com/docs/install)在服务器上安装 Caddy。
2. 把 `/etc/caddy/Caddyfile` 的内容换成下面三行，域名换成自己的：

   ```
   todo.example.com {
   	reverse_proxy 127.0.0.1:8080
   }
   ```

3. 运行 `sudo systemctl reload caddy` 让配置生效。

稍等片刻，用浏览器打开 `https://todo.example.com`，能看到 NextTodo 的登录页就成功了。已经在用 Nginx 的，配置见[部署文档](https://github.com/JunieXD/NextTodo/blob/main/docs/deploy.md#反向代理)。

## 3. 创建账号并登录

回到 `nexttodo` 目录，创建管理员账号。下面的 `admin` 是用户名，可以换成自己想用的（字母、数字和下划线），然后按提示输入两次密码：

```sh
docker compose exec nexttodo /nexttodo-server create-admin admin
```

然后在 App 里点“连接服务器”，服务器地址填域名（如 `todo.example.com`），再填用户名和密码登录。浏览器里打开域名就是网页版。

## 4. 邀请其他人

每人一个账号，数据彼此独立。管理员在“设置 → 邀请注册”里点“生成邀请码”，邀请链接会自动复制，发给对方即可。对方打开链接，或者在登录页选“邀请码注册”并填入邀请码，就能注册。每个邀请码只能注册一个账号。

## 5. 让服务器替你整理 {#ai}

[AI 整理](./ai)的配置默认只能保存在各自的设备上，整理时设备要联网。想让服务器自己整理、提交后设备可以离线，要给服务器设一个加密密钥：

1. 在 `nexttodo` 目录里运行下面的命令，生成密钥并写进 `.env` 文件：

   ```sh
   echo "NEXTTODO_SECRET_KEY=$(openssl rand -base64 32)" >> .env
   ```

2. 运行 `docker compose up -d` 让它生效。
3. 在 App 的 AI 整理设置里，保存位置选“同步到服务器”。

服务器用这个密钥加密保存大家的 API 密钥。`.env` 文件要保管好；换掉或丢了它，已保存的 API 密钥就解不开，要重新填写。

## 升级

出了新版本时，管理员打开 NextTodo 会看到提示条“服务器可升级到 0.1.N”，设置的“关于”里也写着。想第一时间知道，可以在 [GitHub 仓库](https://github.com/JunieXD/NextTodo)右上角点 **Watch** → **Custom**，勾选 **Releases** 后点 **Apply**，发布新版本时 GitHub 会发邮件通知。

在服务器上的 `nexttodo` 目录里运行：

```sh
docker compose pull && docker compose up -d
```

数据会自动升级，不用另外操作。如果在 `.env` 里写了 `NEXTTODO_IMAGE` 固定版本，先把其中的版本号改成新的。

App 不用手动升级：服务器升级后，各设备的 App 会自动获取新版本。

## 备份

服务器每天在 `data/snapshots/` 里保存一份数据库快照，保留最近 7 份。把这些快照定期复制到别处，就是备份。从快照恢复的步骤见[部署文档](https://github.com/JunieXD/NextTodo/blob/main/docs/deploy.md#备份和恢复)。

## 遇到问题

- 查看 NextTodo 的日志：在 `nexttodo` 目录里运行 `docker compose logs --tail 100`。
- 网页打不开：检查域名是否已经指向服务器，80 和 443 端口是否放行，Caddy 是否在运行（`systemctl status caddy`）。
