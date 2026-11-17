#!/usr/bin/env node
// D-02 构建侧（docs/d02-homebrew-plan.md §3.1）：把 prune CLI 编进 macOS .app，
// 让 cask 用 binary stanza 链接出来（GUI + CLI 同一份产物，版本零错位）。
//
// 调用方：src-tauri/tauri.macos.conf.json 的 build.beforeBuildCommand（仅 macOS
// 构建路径）——Windows/Linux 的 bundle 内容保持与历史版本一致，不受影响。
//
// 目标 triple 解析优先级：
//   CLI_TARGET_TRIPLE      CI 显式注入（release.yml bundle job）
//   TAURI_ENV_TARGET_TRIPLE Tauri CLI 为 before*Command 注入的环境变量
//   rustc -vV host         本机 dev 构建（tauri build 不带 --target 时）
//
// 产物落 src-tauri/resources/bin/prune（tauri.macos.conf.json 的
// bundle.resources 映射为 .app 内的 Contents/Resources/bin/prune）。
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function hostTriple() {
  const out = execFileSync("rustc", ["-vV"], { encoding: "utf8" });
  const m = /host:\s*(\S+)/.exec(out);
  if (!m) throw new Error("无法从 rustc -vV 解析 host triple");
  return m[1];
}

const triple = process.env.CLI_TARGET_TRIPLE || process.env.TAURI_ENV_TARGET_TRIPLE || hostTriple();
const binName = process.platform === "win32" ? "prune.exe" : "prune";

console.log(`[prepare-cli-resource] cargo build -p prune-cli --target ${triple}`);
execFileSync("cargo", ["build", "--release", "-p", "prune-cli", "--target", triple], {
  cwd: root,
  stdio: "inherit",
});

const src = join(root, "target", triple, "release", binName);
const dest = join(root, "src-tauri", "resources", "bin", "prune");
if (!statSync(src).isFile()) {
  throw new Error(`CLI 二进制不存在：${src}`);
}
mkdirSync(dirname(dest), { recursive: true });
copyFileSync(src, dest);
console.log(`[prepare-cli-resource] ${triple} → ${dest}`);
