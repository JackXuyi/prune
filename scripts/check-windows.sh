#!/usr/bin/env bash
# Windows 编译基线检查（W0）
#
# 目的：在 macOS / Linux 上验证 cfg(windows) 分支能通过编译，
# 提前拦住「libc::kill / crate::popover / statvfs 这类只在 unix 存在」的错误，
# 不必等真机或 CI 才发现。
#
# 为什么用 windows-gnu 而不是 windows-msvc：
#   rusqlite 的 bundled 特性需要用 C 编译器编 SQLite，macOS 上没有 MSVC 工具链，
#   mingw-w64 提供的 x86_64-w64-mingw32-gcc 可以在本机完成这一步。
#
# 依赖：
#   brew install mingw-w64        # 提供 x86_64-w64-mingw32-gcc
#   rustup target add x86_64-pc-windows-gnu
#
# 用法：
#   scripts/check-windows.sh              # 检查整个 workspace
#   TARGET=x86_64-pc-windows-msvc scripts/check-windows.sh   # 有 MSVC 工具链时
set -euo pipefail

cd "$(dirname "$0")/.."

TARGET="${TARGET:-x86_64-pc-windows-gnu}"

if ! rustup target list --installed | grep -qx "$TARGET"; then
  echo "==> 安装 target $TARGET"
  # 国内网络直连 static.rust-lang.org 常失败，可临时指定镜像：
  # RUSTUP_DIST_SERVER=https://rsproxy.cn scripts/check-windows.sh
  rustup target add "$TARGET"
fi

case "$TARGET" in
  *-gnu)
    if ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
      echo "缺少 mingw-w64（rusqlite bundled 需要 C 工具链）：brew install mingw-w64" >&2
      exit 1
    fi
    ;;
esac

echo "==> cargo check --target $TARGET（workspace）"
cargo check --workspace --all-targets --target "$TARGET"

echo "==> 完成：cfg(windows) 分支编译通过（真机行为仍需 Windows 机器验证）"
