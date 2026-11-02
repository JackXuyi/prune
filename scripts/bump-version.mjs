#!/usr/bin/env node
/**
 * 统一递增版本号：package.json / src-tauri/tauri.conf.json / src-tauri/Cargo.toml / Cargo.lock
 *
 * 用法：
 *   pnpm bump            # patch: 0.1.0 -> 0.1.1
 *   pnpm bump minor      # 0.1.0 -> 0.2.0
 *   pnpm bump major      # 0.1.0 -> 1.0.0
 *   pnpm bump 1.2.3      # 指定具体版本号
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { execSync } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')

// ---------- 解析目标版本 ----------
const arg = process.argv[2] || 'patch'
const pkgPath = join(root, 'package.json')
const pkg = JSON.parse(readFileSync(pkgPath, 'utf8'))
const current = pkg.version

let target
if (/^\d+\.\d+\.\d+/.test(arg)) {
  target = arg
} else if (['patch', 'minor', 'major'].includes(arg)) {
  const [maj, min, pat] = current.split('.').map(Number)
  const next = { major: [maj + 1, 0, 0], minor: [maj, min + 1, 0], patch: [maj, min, pat + 1] }[arg]
  target = next.join('.')
} else {
  console.error(`无效参数: "${arg}"，支持 patch / minor / major / x.y.z`)
  process.exit(1)
}

// ---------- 逐个文件更新 ----------
const results = []

// 1. package.json
pkg.version = target
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n')
results.push(['package.json', current, target])

// 2. src-tauri/tauri.conf.json
const confPath = join(root, 'src-tauri', 'tauri.conf.json')
const confRaw = readFileSync(confPath, 'utf8')
const conf = JSON.parse(confRaw)
if (conf.version && conf.version !== target) {
  conf.version = target
  writeFileSync(confPath, JSON.stringify(conf, null, 2) + '\n')
  results.push(['src-tauri/tauri.conf.json', conf.version, target])
}

// 3. src-tauri/Cargo.toml（只改 [package] 下的 version 行）
const cargoPath = join(root, 'src-tauri', 'Cargo.toml')
const cargoRaw = readFileSync(cargoPath, 'utf8')
let changed = false
const cargoNew = cargoRaw.replace(/^version\s*=\s*"[^"]+"/m, (m) => {
  const oldV = m.match(/"([^"]+)"/)[1]
  if (oldV !== target) {
    changed = true
    results.push(['src-tauri/Cargo.toml', oldV, target])
  }
  return `version = "${target}"`
})
if (cargoNew !== cargoRaw) writeFileSync(cargoPath, cargoNew)

// 4. Cargo.lock（让锁文件与 workspace 版本保持一致，避免 CI 重复校验失败）
if (changed) {
  const { existsSync } = await import('node:fs')
  const { homedir } = await import('node:os')
  const cargoBins = [
    process.env.CARGO,
    'cargo',
    join(homedir(), '.cargo', 'bin', 'cargo'), // rustup 默认安装位置（非交互 shell 不在 PATH）
  ].filter(Boolean)
  const cargoBin = cargoBins.find((c) => c === 'cargo' || existsSync(c))
  if (!cargoBin) {
    console.warn('⚠️ 未找到 cargo，请手动运行: cargo update -p prune')
  } else {
    try {
      execSync(`${cargoBin} update -p prune --precise ${target}`, { cwd: root, stdio: 'ignore' })
      results.push(['Cargo.lock', current, target])
    } catch {
      console.warn(`⚠️ cargo update -p prune 失败，请手动运行: cargo update -p prune --precise ${target}`)
    }
  }
}

// ---------- 输出 ----------
if (results.length === 0) {
  console.log(`所有文件版本号已是 ${target}，无需更新。`)
} else {
  console.log(`✅ 版本号已更新: ${current} -> ${target}\n`)
  for (const [file, from, to] of results) {
    console.log(`  ${file}: ${from} -> ${to}`)
  }
  console.log(`\n下一步:`)
  console.log(`  git add -A && git commit -m "chore: bump version to ${target}"`)
  console.log(`  git tag v${target}`)
  console.log(`  git push origin main --tags`)
}
