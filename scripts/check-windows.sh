#!/usr/bin/env bash
# Windows 编译基线检查（W0）
#
# 目的：在 macOS / Linux 上验证 cfg(windows) 分支真能编译，提前拦住
# 「libc::kill / CommandExt::raw_arg / crate::popover 这类只在某一平台存在」的错误，
# 不必等真机或 CI 才发现。W0 实测就抓到一个：raw_arg 是 trait 方法而非固有方法。
#
# 为什么用 windows-gnu 而不是 windows-msvc：
#   rusqlite 的 bundled 需要用 C 编译器编 SQLite；macOS 上没有 MSVC 工具链，
#   但有 x86_64-w64-mingw32-gcc 就能在本机完成这一步。
#
# 工具链来源（脚本自动探测，两者选其一即可）：
#   1) Homebrew：brew install mingw-w64            → x86_64-w64-mingw32-gcc
#   2) llvm-mingw（自包含，推荐，国内网络更稳）：
#        https://github.com/mstorsjo/llvm-mingw/releases
#        解压后把 <dir>/bin 加进 PATH，或设 MINGW_BIN=<dir>/bin
#
# 注意 llvm-mingw 用 compiler-rt，不提供 libgcc/libgcc_eh，而 rustc 的
# windows-gnu target 会硬链接 -lgcc -lgcc_eh；脚本会自动生成一份 shim 目录
# （libgcc.a → libclang_rt.builtins，libgcc_eh.a → libunwind.a）补上。
#
# 用法：
#   scripts/check-windows.sh              # 默认：cargo check 整个 workspace（含 tests/examples）
#   scripts/check-windows.sh --build      # 额外真链接一次 prune-cli，产出 PE 可执行文件
#
# 已知限制：
#   prune-app 同时声明了 cdylib（给移动端用），windows-gnu 下 cdylib 链接会撞上
#   「too many exported symbols (max 65535)」。这是 GNU 链接器的限制，真实 Windows
#   构建走 MSVC 不受影响，因此 --build 只链接 CLI（纯 bin crate）。
set -euo pipefail

cd "$(dirname "$0")/.."

TARGET="${TARGET:-x86_64-pc-windows-gnu}"
DO_BUILD=0
for arg in "$@"; do
  case "$arg" in
    --build) DO_BUILD=1 ;;
    *) echo "未知参数：$arg" >&2; exit 2 ;;
  esac
done

if ! rustup target list --installed | grep -qx "$TARGET"; then
  echo "==> 安装 target $TARGET"
  # 国内直连 static.rust-lang.org 常失败，可临时指定镜像：
  # RUSTUP_DIST_SERVER=https://rsproxy.cn scripts/check-windows.sh
  rustup target add "$TARGET"
fi

case "$TARGET" in
  *-gnu) ;;
  *) echo "当前脚本只处理 *-gnu 目标（收到 ${TARGET}）" >&2; exit 1 ;;
esac

# ---------------------------------------------------------------- 工具链定位
if [ -n "${MINGW_BIN:-}" ]; then
  PATH="$MINGW_BIN:$PATH"
fi
if ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
  BREW_PREFIX="$(brew --prefix mingw-w64 2>/dev/null || true)"
  if [ -n "$BREW_PREFIX" ] && [ -d "$BREW_PREFIX/bin" ]; then
    PATH="$BREW_PREFIX/bin:$PATH"
  fi
fi
if ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
  cat >&2 <<'EOF'
缺少 Windows C 交叉编译器（rusqlite bundled 必需）。二选一：

  brew install mingw-w64
  # 或下载自包含的 llvm-mingw 并指定：
  MINGW_BIN=/path/to/llvm-mingw/bin scripts/check-windows.sh
EOF
  exit 1
fi
export PATH

# ------------------------------------------------- libgcc shim（llvm-mingw 专用）
# GNU 版 mingw-w64 自带 libgcc.a；llvm-mingw 没有，需要指向 compiler-rt。
# 注意不能信 `-print-libgcc-file-name`：llvm-mingw 的 gcc wrapper 会把它谎报成
# libclang_rt.builtins-x86_64.a（文件存在，但 rustc 要的是 `-lgcc`/`-lgcc_eh`
# 这两个库名）。所以这里直接做一次真实链接探针。
SHIM_DIR=""
PROBE_DIR="$(mktemp -d)"
if printf 'int main(void){return 0;}\n' \
  | x86_64-w64-mingw32-gcc -x c - -o "$PROBE_DIR/probe.exe" -lgcc -lgcc_eh >/dev/null 2>&1; then
  : # 原生 GNU 工具链，libgcc 齐备
else
  RES_DIR="$(x86_64-w64-mingw32-clang --print-resource-dir 2>/dev/null || true)"
  BUILTINS="$RES_DIR/lib/windows/libclang_rt.builtins-x86_64.a"
  UNWIND="$(cd "$(dirname "$(command -v x86_64-w64-mingw32-gcc)")/.." && pwd)/x86_64-w64-mingw32/lib/libunwind.a"
  if [ -f "$BUILTINS" ] && [ -f "$UNWIND" ]; then
    SHIM_DIR="${TMPDIR:-/tmp}/prune-mingw-libshim"
    mkdir -p "$SHIM_DIR"
    ln -sf "$BUILTINS" "$SHIM_DIR/libgcc.a"
    ln -sf "$UNWIND" "$SHIM_DIR/libgcc_eh.a"
    echo "==> libgcc shim：${SHIM_DIR}（compiler-rt 替代 GCC 运行库）"
  else
    echo "⚠️ 找不到 compiler-rt/libunwind，链接阶段可能失败（cargo check 不受影响）" >&2
  fi
fi
rm -rf "$PROBE_DIR"

RUSTFLAGS_KEY="CARGO_TARGET_$(echo "$TARGET" | tr 'a-z-' 'A-Z_')_RUSTFLAGS"
if [ -n "$SHIM_DIR" ]; then
  export "${RUSTFLAGS_KEY}=-L $SHIM_DIR"
fi

# ---------------------------------------------------------------- 编译检查
echo "==> cargo check --workspace --all-targets --target $TARGET"
cargo check --workspace --all-targets --target "$TARGET"
echo "✅ cfg(windows) 分支编译通过"

# ---------------------------------------------------------------- 真链接（可选）
if [ "$DO_BUILD" = 1 ]; then
  echo "==> cargo build -p prune-cli（验证 C 库与链接器链路）"
  cargo build --target "$TARGET" -p prune-cli
  EXE="target/$TARGET/debug/prune.exe"
  if [ -f "$EXE" ]; then
    echo "✅ 产出 Windows 可执行文件：${EXE}（$(du -h "$EXE" | cut -f1)）"
  else
    echo "❌ 未找到 $EXE" >&2
    exit 1
  fi
fi

echo
echo "提示：这只是编译基线，运行时行为（内存口径 / 磁盘规则 / 托盘定位）仍需 Windows 真机验证。"
