#!/usr/bin/env node
// D-02：渲染 cask + formula 并推送 Homebrew tap（docs/d02-homebrew-plan.md §3.3/§4）。
//
// 调用方：release.yml 的 homebrew-tap job（tag 触发）。本地调试：
//   node scripts/brew-tap.mjs --dry-run           # 渲染到 output/brew-tap/，不推送
//   node scripts/brew-tap.mjs --local-asset <path> # 用本地文件替代 CLI universal tar.gz
//                                                   #（该 asset 在下个发版前不存在，本地测试用）
//
// 环境变量：
//   PRUNE_TAP_TOKEN   GitHub PAT（fine-grained，仅 tap 仓库 contents:write）
//   RELEASE_TAG       v0.13.2 形态
//   GITHUB_REPOSITORY JackXuyi/prune
//   TAP_REPO          默认 JackXuyi/homebrew-prune
//
// 幂等：渲染结果与 tap 现状一致时不产生 commit（同 tag 重跑 release.yml 安全）。
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const args = process.argv.slice(2);
const dryRun = args.includes("--dry-run");
const localAssetIdx = args.indexOf("--local-asset");
const localAssetPath = localAssetIdx >= 0 ? args[localAssetIdx + 1] : null;

const repo = process.env.GITHUB_REPOSITORY || "JackXuyi/prune";
const tapRepo = process.env.TAP_REPO || "JackXuyi/homebrew-prune";
const token = process.env.PRUNE_TAP_TOKEN;
const tag = process.env.RELEASE_TAG;

if (!tag) throw new Error("缺少 RELEASE_TAG（如 v0.13.2）");
if (!/^v\d+\.\d+\.\d+$/.test(tag)) throw new Error(`RELEASE_TAG 形态不对：${tag}`);
const version = tag.slice(1);
const headers = token ? { Authorization: `token ${token}`, Accept: "application/vnd.github+json" } : {};

function fetchJson(url) {
  const res = spawnSync("curl", ["-sSL", ...(token ? ["-H", `Authorization: token ${token}`] : []), url], {
    encoding: "utf8",
    maxBuffer: 1 << 26,
  });
  if (res.status !== 0) throw new Error(`curl ${url} 失败：${res.stderr}`);
  return JSON.parse(res.stdout);
}

function sha256File(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

// ---------------------------------------------------------------- 资产发现

const releases = fetchJson(`https://api.github.com/repos/${repo}/releases?per_page=100`);
const rel = releases.find((r) => r.tag_name === tag);
if (!rel) throw new Error(`找不到 ${tag} 的 Release（是否已发布？draft 对 API 按标签不可见）。`);
const assets = rel.assets ?? [];
const pick = (re) => {
  const a = assets.find((a) => re.test(a.name));
  if (!a) throw new Error(`Release ${tag} 缺少资产（匹配 ${re}）——bundle/cli-universal 是否都成功？`);
  return a;
};
const dmgArm = pick(/^Prune_.*_aarch64\.dmg$/);
const dmgIntel = pick(/^Prune_.*_x64\.dmg$/);
// CLI universal asset 自 v0.13.2（D-02 落地后的第一个发版）才存在；
// 本地测试可用 --local-asset 提供文件，跳过远端查找
const cliAsset = localAssetPath ? null : pick(/^Prune_CLI_.*_universal_macos\.tar\.gz$/);

// 资产 sha256 的来源有两级：
//   1) GitHub API 的 digest 字段（"sha256:…"）——由 GitHub 对存储对象实算，免下载 13MB；
//   2) digest 缺失（早期上传的资产）时退回 curl 落盘再算——以实际字节为准。
// 不用 fetch 直读内存：大文件在受限网络下会一直等到 socket 超时，且失败没有中间态。
function assetSha(asset, label) {
  const d = typeof asset.digest === "string" ? asset.digest : "";
  if (d.startsWith("sha256:") && d.length === 71) {
    console.log(`[brew-tap] ${label} sha256 ← API digest`);
    return d.slice(7);
  }
  console.log(`[brew-tap] ${label} sha256 ← 下载实算（${(asset.size / 1e6).toFixed(1)}MB）`);
  const tmp = join(tmpdir(), `prune-sha-${label}-${Date.now()}`);
  try {
    execFileSync("curl", ["-fsSL", "--retry", "3", "-o", tmp, ...(token ? ["-H", `Authorization: token ${token}`] : []), asset["browser_download_url"]], {
      stdio: ["ignore", "ignore", "inherit"],
    });
    return sha256File(tmp);
  } finally {
    rmSync(tmp, { force: true });
  }
}

const shaArm = assetSha(dmgArm, "arm");
const shaIntel = assetSha(dmgIntel, "intel");

// ---------------------------------------------------------------- 渲染

const CASK = `cask "prune" do
  arch arm: "aarch64", intel: "x64"

  version "${version}"
  sha256 arm: "${shaArm}", intel: "${shaIntel}"

  url "https://github.com/${repo}/releases/download/v#{version}/Prune_#{version}_#{arch}.dmg",
      verified: "github.com/${repo}/"
  name "Prune"
  desc "Local dev-machine health toolbox: disk, memory, ports, network"
  homepage "https://github.com/${repo}"

  livecheck do
    url :url
    strategy :github_latest
  end

  # 应用内置自动更新（latest.json + minisign 签名校验）。brew 元数据不追踪
  # 应用内升级；如需经 brew 管理版本：brew upgrade --cask prune
  auto_updates true
  depends_on macos: ">= :ventura"

  app "Prune.app"

  # CLI 随 .app 分发（Contents/Resources/bin/prune，与 GUI 同一次构建，版本零错位），
  # binary stanza 链接到 $(brew --prefix)/bin/prune；卸载 cask 一并移除。
  binary "#{appdir}/Prune.app/Contents/Resources/bin/prune"

  zap trash: [
    "~/Library/Application Support/com.xuyi.prune",
    "~/Library/Caches/com.xuyi.prune",
    "~/Library/Logs/com.xuyi.prune",
    "~/Library/Preferences/com.xuyi.prune.plist",
  ]
end
`;

const cliSha = localAssetPath ? sha256File(localAssetPath) : assetSha(cliAsset, "cli");
const FORMULA = `class PruneCli < Formula
  desc "Local dev-machine health CLI (disk, memory, ports, network)"
  homepage "https://github.com/${repo}"
  url "https://github.com/${repo}/releases/download/v#{version}/Prune_CLI_#{version}_universal_macos.tar.gz"
  version "${version}"
  sha256 "${cliSha}"

  livecheck do
    url :stable
    strategy :github_latest
  end

  depends_on macos: :ventura

  def install
    bin.install "prune"
  end

  def caveats
    <<~EOS
      CLI only. For the menu-bar app, run:
        brew install --cask jackxuyi/prune/prune
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/prune --version")
  end
end
`;

// ---------------------------------------------------------------- 输出

if (dryRun) {
  const out = join("output", "brew-tap", "Casks");
  mkdirSync(join("output", "brew-tap", "Formula"), { recursive: true });
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, "prune.rb"), CASK);
  writeFileSync(join("output", "brew-tap", "Formula", "prune-cli.rb"), FORMULA);
  console.log(`[brew-tap] DRY_RUN：已渲染到 output/brew-tap/（arm ${shaArm.slice(0, 12)}… intel ${shaIntel.slice(0, 12)}… cli ${cliSha.slice(0, 12)}…）`);
  process.exit(0);
}

if (!token) throw new Error("缺少 PRUNE_TAP_TOKEN——到 Settings → Secrets and variables → Actions 添加（fine-grained PAT，仅 tap 仓库 contents:write）");

// ---------------------------------------------------------------- 推 tap

const dir = join(tmpdir(), `prune-tap-${Date.now()}`);
console.log(`[brew-tap] clone ${tapRepo}…`);
try {
  execFileSync("git", ["clone", "--depth", "1", `https://x-access-token:${token}@github.com/${tapRepo}.git`, dir], { stdio: "inherit" });
} catch {
  throw new Error(
    [
      `无法 clone ${tapRepo}。发版前需要一次性完成三件事：`,
      `  1) 在 GitHub 建公开仓库 ${tapRepo}（空仓库即可，含 README 能立住）`,
      `  2) 建 main 分支，并预先放 Casks/ 与 Formula/ 两个空目录（否则 push 后 tap 不可用）`,
      `  3) Settings → Secrets and variables → Actions 添加 PRUNE_TAP_TOKEN`,
      `     （fine-grained PAT，仅该仓库 Contents: Read and write）`,
      `本地调试可用 --dry-run，不需要 token 与 tap 仓库。`,
    ].join("\n"),
  );
}
mkdirSync(join(dir, "Casks"), { recursive: true });
mkdirSync(join(dir, "Formula"), { recursive: true });
writeFileSync(join(dir, "Casks", "prune.rb"), CASK);
writeFileSync(join(dir, "Formula", "prune-cli.rb"), FORMULA);

const status = execFileSync("git", ["status", "--porcelain"], { cwd: dir, encoding: "utf8" }).trim();
if (!status) {
  console.log("[brew-tap] 渲染结果与 tap 现状一致，无 commit（幂等跳过）");
  rmSync(dir, { recursive: true, force: true });
  process.exit(0);
}
execFileSync("git", ["add", "-A"], { cwd: dir, stdio: "inherit" });
execFileSync("git", ["-c", "user.name=prune-release-bot", "-c", "user.email=release@prune.local", "commit", "-m", `prune ${tag}`], { cwd: dir, stdio: "inherit" });
execFileSync("git", ["push", "origin", "HEAD"], { cwd: dir, stdio: "inherit" });
rmSync(dir, { recursive: true, force: true });
console.log(`[brew-tap] ✓ 已推送 ${tapRepo}（prune ${tag}）`);
