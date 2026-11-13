#!/usr/bin/env node
/**
 * Gitee 一键发版：bump → 构建当前平台安装包 → 打 tag 推送（Gitee + GitHub）→ 创建/更新 Gitee Release 并上传产物
 *
 * 用法：
 *   pnpm release                # patch 递增（0.1.0 → 0.1.1）
 *   pnpm release minor          # 0.1.0 → 0.2.0
 *   pnpm release 0.2.0          # 指定版本
 *   pnpm release --no-push      # 只构建+本地打 tag，不推送
 *   pnpm release --no-github    # 只推 Gitee，不推 GitHub（不触发 GitHub Actions）
 *   pnpm release --skip-build   # 跳过构建，直接上传已有产物（用于补传另一个平台的包）
 *   pnpm release --dry-run      # 只打印将要执行的动作
 *
 * 前置：
 *   export GITEE_TOKEN=xxxx     # Gitee 私人令牌，需 projects 权限（读写仓库内容）
 *
 * 跨平台：在 macOS 上跑产出 .dmg，在 Windows 上跑产出 .msi/.exe，
 * 两次执行同一个 tag 会把产物追加到同一个 Release（幂等）。
 *
 * 远程：按 host 自动识别 —— gitee.com 的远程用于 Release 上传，github.com 的远程用于
 * 推送 tag 触发 .github/workflows/release.yml 自动构建双平台安装包。
 */
import { execFileSync, execSync } from 'node:child_process'
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs'
import { basename, dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const argv = process.argv.slice(2)
const flags = new Set(argv.filter((a) => a.startsWith('--')))
const versionArg = argv.find((a) => !a.startsWith('--')) || 'patch'

const DRY = flags.has('--dry-run')
const SKIP_BUILD = flags.has('--skip-build')
const NO_PUSH = flags.has('--no-push')

const log = (...a) => console.log(...a)
const step = (t) => log(`\n▶ ${t}`)
const ok = (t) => log(`  ✅ ${t}`)
const warn = (t) => log(`  ⚠️  ${t}`)

// ---------------------------------------------------------------- 仓库信息
function git(cmd) {
  return execSync(`git ${cmd}`, { cwd: root, encoding: 'utf8' }).trim()
}

// origin 现在指向 GitHub（双平台打包走 GitHub Actions），Gitee 是独立远程。
// 这里不写死 origin，而是按 host 自动匹配远程，避免改远程名就把发版推错地方。
function pickRemote(hostRe, preferred) {
  const names = git('remote').split('\n').map((s) => s.trim()).filter(Boolean)
  const ordered = [preferred, ...names.filter((n) => n !== preferred)]
  for (const name of ordered) {
    const url = git(`remote get-url ${name}`)
    const match = url.match(hostRe)
    if (match) return { name, url, owner: match[1], repo: match[2] }
  }
  return null
}

const GITEE_RE = /gitee\.com[^:/]*[:/]([^/]+)\/([^/.]+?)(?:\.git)?$/
const GITHUB_RE = /github\.com[^:/]*[:/]([^/]+)\/([^/.]+?)(?:\.git)?$/

const giteeRemote = pickRemote(GITEE_RE, 'gitee')
if (!giteeRemote) {
  console.error('未找到指向 gitee.com 的远程，请先添加：')
  console.error('  git remote add gitee git@gitee.com:<owner>/<repo>.git')
  process.exit(1)
}
const githubRemote = pickRemote(GITHUB_RE, 'origin')
const OWNER = giteeRemote.owner
const REPO = giteeRemote.repo
const API = 'https://gitee.com/api/v5'
log(`仓库：${OWNER}/${REPO}（远程 ${giteeRemote.name}）`)
if (githubRemote) log(`GitHub：${githubRemote.owner}/${githubRemote.repo}（远程 ${githubRemote.name}）`)
log(`平台：${process.platform} / ${process.arch}`)

// ---------------------------------------------------------------- 1. 版本号
function currentVersion() {
  return JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).version
}

function resolveVersion(arg) {
  if (/^\d+\.\d+\.\d+/.test(arg)) return arg
  const [maj, min, pat] = currentVersion().split('.').map(Number)
  const table = { patch: [maj, min, pat + 1], minor: [maj, min + 1, 0], major: [maj + 1, 0, 0] }
  if (!table[arg]) {
    console.error(`无效版本参数：${arg}（支持 patch / minor / major / x.y.z）`)
    process.exit(1)
  }
  return table[arg].join('.')
}

const from = currentVersion()
const to = resolveVersion(versionArg)
const tag = `v${to}`

if (!SKIP_BUILD) {
  step(`更新版本号 ${from} → ${to}`)
  if (DRY) warn('dry-run：跳过实际写入')
  else execFileSync('node', [join(root, 'scripts', 'bump-version.mjs'), to], { cwd: root, stdio: 'inherit' })
}

// ---------------------------------------------------------------- 2. 构建
const targetTriple =
  process.platform === 'darwin'
    ? process.arch === 'arm64'
      ? 'aarch64-apple-darwin'
      : 'x86_64-apple-darwin'
    : process.platform === 'win32'
      ? 'x86_64-pc-windows-msvc'
      : 'x86_64-unknown-linux-gnu'

if (!SKIP_BUILD) {
  step(`构建安装包（${targetTriple}）`)
  // pnpm 不一定在 PATH 里（新机器 / 受限 shell）；本地 node_modules 里就有
  // 同一份 @tauri-apps/cli，回退用它，命令语义完全一致
  let hasPnpm = true
  try {
    execSync('command -v pnpm', { stdio: 'pipe' })
  } catch {
    hasPnpm = false
  }
  const cmd = hasPnpm
    ? `pnpm tauri build --target ${targetTriple}`
    : `node_modules/.bin/tauri build --target ${targetTriple}`
  log(`  $ ${cmd}`)
  if (DRY) warn('dry-run：跳过构建')
  else execSync(cmd, { cwd: root, stdio: 'inherit' })
}

// ---------------------------------------------------------------- 3. 收集产物
function walk(dir, acc = []) {
  if (!existsSync(dir)) return acc
  for (const name of readdirSync(dir)) {
    const p = join(dir, name)
    const st = statSync(p)
    if (st.isDirectory()) walk(p, acc)
    else acc.push({ path: p, size: st.size })
  }
  return acc
}

// 产物可能在带 target 三元组的目录，也可能在默认 target/release 下（不带 --target 构建时）
// 注意两种布局：src-tauri 独立项目时是 src-tauri/target/...；Cargo workspace（本仓库）
// 共享根 target，产物在仓库根的 target/... 下
const bundleCandidates = [
  join(root, 'target', targetTriple, 'release', 'bundle'),
  join(root, 'target', 'release', 'bundle'),
  join(root, 'src-tauri', 'target', targetTriple, 'release', 'bundle'),
  join(root, 'src-tauri', 'target', 'release', 'bundle'),
]
const extensions = process.platform === 'darwin' ? ['.dmg', '.app.tar.gz'] : ['.msi', '.exe']
const assets = bundleCandidates.flatMap((dir) => walk(dir)).filter((f) => extensions.some((e) => f.path.endsWith(e)))

if (assets.length === 0) {
  warn(`未在 bundle 目录找到安装包：${bundleCandidates.map((d) => d.replace(root + '/', '')).join(' 或 ')}`)
  if (!DRY) {
    console.error('请先完整构建，或检查 bundle 路径。')
    process.exit(1)
  }
} else {
  step('待上传产物')
  for (const a of assets) log(`  - ${basename(a.path)}（${(a.size / 1024 / 1024).toFixed(1)} MB）`)
}

// ---------------------------------------------------------------- 4. 提交 + tag
if (!SKIP_BUILD && !DRY) {
  step('提交版本变更并打 tag')

  // tag 已存在 = 拒绝。updater 以 tag 判断「有没有新版本」，用 -f 覆盖已有 tag
  // 会产生「同一个版本号对应两份不同产物」的不可复现状态：已经下过 v0.8.3 的
  // 用户再也收不到更新，而仓库里的 v0.8.3 已经是另一份代码。
  let tagExists = false
  try {
    git(`rev-parse -q --verify refs/tags/${tag}`)
    tagExists = true
  } catch {
    tagExists = false
  }
  if (tagExists) {
    console.error(`tag ${tag} 已存在，拒绝覆盖。`)
    console.error(`  换个版本号：pnpm bump patch`)
    console.error(`  或先确认无产物依赖后删除：git tag -d ${tag}`)
    process.exit(1)
  }

  execSync('git add -A', { cwd: root, stdio: 'inherit' })
  // 有变更才提交；一旦提交失败（hook 拒绝 / 冲突）必须中止 —— 原先的
  // `|| true` 会把这个失败吞掉，随后 tag 仍打在不含本次版本变更的 commit 上
  let hasStaged = true
  try {
    execSync('git diff --cached --quiet', { cwd: root, stdio: 'ignore' })
    hasStaged = false
  } catch {
    hasStaged = true
  }
  if (hasStaged) {
    execSync(`git commit -m "chore(release): ${tag}"`, { cwd: root, stdio: 'inherit' })
    ok(`版本变更已提交`)
  } else {
    warn('工作区无变更（版本号已是目标值），跳过提交')
  }

  execSync(`git tag ${tag}`, { cwd: root, stdio: 'inherit' })
  ok(`tag ${tag} 已创建`)
  if (NO_PUSH) {
    warn(`--no-push：未推送，请手动 git push ${giteeRemote.name} master --tags`)
  } else {
    execSync(`git push ${giteeRemote.name} HEAD --tags`, { cwd: root, stdio: 'inherit' })
    ok(`已推送到 Gitee（${giteeRemote.name}）`)
    // 顺手推 GitHub：tag 一落地，GitHub Actions 就会自动构建 macOS + Windows 安装包
    if (githubRemote && !flags.has('--no-github')) {
      try {
        execSync(`git push ${githubRemote.name} HEAD --tags`, { cwd: root, stdio: 'inherit' })
        ok(`已推送到 GitHub（${githubRemote.name}），Actions 将自动构建双平台安装包`)
      } catch {
        warn('GitHub 推送失败，跳过（不影响 Gitee 发版）')
      }
    }
  }
}

// ---------------------------------------------------------------- 5. Gitee Release
// 令牌来源：环境变量 GITEE_TOKEN，或仓库根目录 .env.release（已 gitignore）
function loadToken() {
  if (process.env.GITEE_TOKEN) return process.env.GITEE_TOKEN.trim()
  const envFile = join(root, '.env.release')
  if (existsSync(envFile)) {
    const line = readFileSync(envFile, 'utf8')
      .split('\n')
      .find((l) => l.trim().startsWith('GITEE_TOKEN='))
    if (line) return line.split('=').slice(1).join('=').trim().replace(/^["']|["']$/g, '')
  }
  return ''
}

const token = loadToken()
if (!token) {
  warn('未设置 GITEE_TOKEN，跳过 Release 发布')
  log('  获取方式：Gitee → 设置 → 私人令牌 → 新建（勾选 projects）')
  log(`  然后：GITEE_TOKEN=xxx pnpm release ${to} --skip-build`)
  process.exit(0)
}

async function api(path, init = {}) {
  const res = await fetch(`${API}${path}`, init)
  const text = await res.text()
  let data
  try {
    data = text ? JSON.parse(text) : {}
  } catch {
    data = { raw: text }
  }
  return { ok: res.ok, status: res.status, data }
}

// 令牌只走请求头，不进 URL：query string 会落进代理日志 / ps 输出 / Gitee 访问日志。
// Gitee v5 的对象接口支持 `Authorization: Bearer <token>`；极少数老接口不认头，
// 因此 GET 保留一次 query 降级 —— 但会明确打印警告，让「令牌进日志」这件事可见，
// 而不是像原先那样静默地把 token 拼在每一个 URL 上。
const headers = {
  'Content-Type': 'application/json',
  Authorization: `Bearer ${token}`,
}

step('查找已有 Release')
let release = null
const releasePath = `/repos/${OWNER}/${REPO}/releases/tags/${tag}`
let found = await api(releasePath, { headers })
if (!found.ok && (found.status === 401 || found.status === 403)) {
  warn('请求头鉴权被拒，降级为 query token（本次令牌会出现在 URL / 日志中）')
  found = await api(`${releasePath}?access_token=${encodeURIComponent(token)}`)
}
if (found.ok && found.data?.id) {
  release = found.data
  ok(`复用已有 Release #${release.id}（${release.name}）`)
} else {
  log(`  未找到 ${tag} 的 Release，创建中…`)
  if (DRY) {
    warn('dry-run：跳过创建')
    process.exit(0)
  }
  const body = readChangelog(to)
  const created = await api(`/repos/${OWNER}/${REPO}/releases`, {
    method: 'POST',
    headers,
    body: JSON.stringify({
      access_token: token,
      tag_name: tag,
      name: `Prune ${tag}`,
      body,
      target_commitish: git('rev-parse --abbrev-ref HEAD'),
      prerelease: false,
    }),
  })
  if (!created.ok) {
    console.error('创建 Release 失败：', created.status, created.data)
    process.exit(1)
  }
  release = created.data
  ok(`Release 已创建 #${release.id}`)
  log(`  ${release.html_url ?? ''}`)
}

function readChangelog(ver) {
  const p = join(root, 'CHANGELOG.md')
  if (existsSync(p)) {
    const txt = readFileSync(p, 'utf8')
    const idx = txt.indexOf(`## ${ver}`)
    if (idx >= 0) return txt.slice(idx, idx + 2000)
  }
  return `Prune ${tag}\n\n- 构建平台：${process.platform} / ${process.arch}`
}

step('上传产物到 Release')
if (DRY) {
  warn('dry-run：跳过上传')
  process.exit(0)
}
let failed = 0
for (const a of assets) {
  const form = new FormData()
  form.append('access_token', token)
  const buf = readFileSync(a.path)
  form.append('file', new Blob([buf]), basename(a.path))
  const up = await api(`/repos/${OWNER}/${REPO}/releases/${release.id}/attach_files`, {
    method: 'POST',
    body: form,
  })
  if (up.ok) {
    ok(`${basename(a.path)} → ${up.data?.browser_download_url ?? 'uploaded'}`)
  } else {
    failed++
    warn(`${basename(a.path)} 上传失败：${up.status} ${JSON.stringify(up.data).slice(0, 200)}`)
  }
}

log(`\n${failed === 0 ? '🎉 发版完成' : '⚠️ 部分产物上传失败'}：${release.html_url ?? `${OWNER}/${REPO}/releases`}`)
log(`  下一步：另一个平台执行  GITEE_TOKEN=xxx pnpm release ${to} --skip-build  以补齐安装包`)
process.exit(failed === 0 ? 0 : 1)
