#!/bin/sh
# Installs the latest NextTodo command line into ~/.local/bin, on macOS or Linux
# (docs/agent.md#命令行):
#
#   curl -fsSL https://juniexd.github.io/NextTodo/install.sh | sh
set -eu

case "$(uname -s) $(uname -m)" in
  "Darwin arm64") file=nexttodo-macos-arm64 ;;
  "Linux x86_64") file=nexttodo-linux-x64 ;;
  "Linux aarch64" | "Linux arm64") file=nexttodo-linux-arm64 ;;
  *)
    echo "不支持这个平台：$(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac
dir=$HOME/.local/bin
mkdir -p "$dir"
# Written beside it and moved in place: a command line in use is never half written.
curl -fsSL -o "$dir/nexttodo.part" "https://github.com/JunieXD/NextTodo/releases/latest/download/$file"
chmod +x "$dir/nexttodo.part"
mv "$dir/nexttodo.part" "$dir/nexttodo"
echo "已安装 $("$dir/nexttodo" --version)：$dir/nexttodo"
case ":$PATH:" in
  *":$dir:"*) ;;
  *)
    echo "$dir 不在 PATH 里。把下面一行加进 shell 的配置文件（如 ~/.zshrc 或 ~/.bashrc），再打开新的终端："
    echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    ;;
esac
