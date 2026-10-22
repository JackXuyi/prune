#!/bin/bash
# reclaim-scan.sh — 开发机可回收空间只读扫描
# 安全声明：本脚本只执行 du / df / docker system df，不删除、不移动任何文件。
set -u

H="$HOME"
OUT="$(mktemp)"
NODE_TMP="$(mktemp)"
trap 'rm -f "$OUT" "$NODE_TMP"' EXIT

human() { awk -v k="${1:-0}" 'BEGIN{
  if (k>=1048576) printf "%.1f GB", k/1048576;
  else if (k>=1024) printf "%.0f MB", k/1024;
  else printf "%d KB", k;
}'; }

scan() { # group | label | path
  local g="$1" label="$2" path="$3" kb
  if [ -e "$path" ]; then
    kb=$(du -sk "$path" 2>/dev/null | awk '{print $1}')
    kb=${kb:-0}
    printf "%s\t%s\t%s\t%s\n" "$g" "$label" "$(human "$kb")" "$kb" >> "$OUT"
  fi
}

echo "扫描中…（大目录需要几十秒）" >&2

# ---------- Xcode ----------
scan "Xcode"       "DerivedData（编译产物）"        "$H/Library/Developer/Xcode/DerivedData"
scan "Xcode"       "iOS DeviceSupport"             "$H/Library/Developer/Xcode/iOS DeviceSupport"
scan "Xcode"       "Archives（归档包）"             "$H/Library/Developer/Xcode/Archives"
scan "Xcode"       "ModuleCache（模块缓存）"         "$H/Library/Developer/Xcode/ModuleCache.noindex"
scan "Xcode"       "SwiftUI Previews 缓存"          "$H/Library/Developer/Xcode/UserData/Previews"
scan "Xcode"       "Simulator 设备"                 "$H/Library/Developer/CoreSimulator/Devices"
scan "Xcode"       "Simulator 运行时缓存"            "$H/Library/Developer/CoreSimulator/Caches"
scan "Xcode"       "Xcode 总缓存"                   "$H/Library/Caches/com.apple.dt.Xcode"

# ---------- JS / TS ----------
scan "JS"          "npm 缓存"                       "$H/.npm/_cacache"
scan "JS"          "Yarn 缓存"                      "$H/Library/Caches/Yarn"
scan "JS"          "pnpm store"                     "$H/Library/pnpm/store"
scan "JS"          "pnpm store（旧路径）"            "$H/.local/share/pnpm/store"
scan "JS"          "Bun 安装缓存"                    "$H/.bun/install/cache"
scan "JS"          "Playwright 浏览器"              "$H/Library/Caches/ms-playwright"
scan "JS"          "Cypress 二进制"                  "$H/Library/Caches/Cypress"
scan "JS"          "Electron 下载缓存"               "$H/Library/Caches/electron"

# node_modules 聚合扫描
find "$H" -maxdepth 8 -type d -name node_modules -prune -print 2>/dev/null > "$NODE_TMP"
node_count=$(wc -l < "$NODE_TMP" | tr -d ' ')
node_kb=0
if [ "$node_count" -gt 0 ]; then
  while IFS= read -r d; do
    k=$(du -sk "$d" 2>/dev/null | awk '{print $1}')
    node_kb=$((node_kb + ${k:-0}))
  done < "$NODE_TMP"
fi
printf "JS\tnode_modules（%s 个目录）\t%s\t%s\n" "$node_count" "$(human "$node_kb")" "$node_kb" >> "$OUT"

# ---------- PHP / Ruby / Java / Go / Rust ----------
scan "后端"        "CocoaPods 缓存"                  "$H/Library/Caches/CocoaPods"
scan "后端"        "Composer 缓存"                   "$H/Library/Caches/composer"
scan "后端"        "Gradle 缓存"                     "$H/.gradle/caches"
scan "后端"        "Maven 本地仓库"                  "$H/.m2/repository"
scan "后端"        "Go module 缓存"                  "$H/go/pkg/mod"
scan "后端"        "Cargo registry"                  "$H/.cargo/registry"
scan "后端"        "Homebrew 下载缓存"               "$H/Library/Caches/Homebrew"

# ---------- 容器 / 虚拟化 ----------
scan "容器"        "Docker VM 磁盘"                  "$H/Library/Containers/com.docker.docker/Data/vms"
scan "容器"        "OrbStack 数据"                   "$H/Library/Group Containers/HUAQ24HBR6.dev.orbstack"
scan "容器"        "Colima 数据"                     "$H/.colima"

# ---------- 输出 ----------
echo "=== TABLES ==="
cat "$OUT"
echo "=== NODE_DETAIL ==="
printf "count=%s total_kb=%s\n" "$node_count" "$node_kb"
echo "=== DISK ==="
df -h / | tail -1
echo "=== DOCKER ==="
if command -v docker >/dev/null 2>&1; then
  docker system df 2>/dev/null || echo "docker-not-running"
else
  echo "docker-not-installed"
fi
echo "=== DONE ==="
