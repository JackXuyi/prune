# GitHub 自动打包（macOS + Windows）

> 目标：push 一个 tag，GitHub 自动产出 macOS（arm64 / x64）与 Windows（x64）安装包，挂到 Draft Release。
> 工作流文件：`.github/workflows/release.yml`。

## 一、触发方式

| 触发 | 场景 | 行为 |
|---|---|---|
| push tag `v*` | 正式发版 | verify 通过后三平台并行构建 → 创建/更新 `Prune vX.Y.Z` Draft Release |
| 手动 `workflow_dispatch` | 试跑、补包 | 用**当前代码里的版本号**构建，同样产出 Draft Release，不会自动改版本号 |

> Draft 是刻意保留的：三平台产物都到齐、自己下载试装过再点 Publish。想改成自动发布，把
> `.github/workflows/release.yml` 里 `releaseDraft: true` 改成 `false`。

## 二、流水线结构

```
verify (ubuntu, ~1 min)          bundle (3 个并行 job, ~8-15 min)
├── 版本号三处一致校验      →    ├── macOS arm64  macos-latest + aarch64-apple-darwin
├── tsc 类型检查                ├── macOS x64    macos-latest + x86_64-apple-darwin
└── vite 前端构建               └── Windows x64  windows-latest + x86_64-pc-windows-msvc
```

几个刻意的设计：

| 决策 | 原因 |
|---|---|
| `verify` 先行，`bundle` 依赖它 | 版本号写错/类型挂掉时 1 分钟失败，不必白跑三个平台的 Rust 编译（lto 下每个 job 8 分钟起） |
| macOS x64 也在 Apple Silicon runner 上交叉编译 | Intel runner（`macos-13` 系列）已被 GitHub 陆续下线，`macos-15-intel` 也不保证长期存在；Rust 交叉编译 x86_64 官方支持，产物完整 |
| `swatinem/rust-cache` 的 `workspaces: . -> target` | 本仓库 Rust 工作区根在**仓库根目录**（`members = src-tauri, crates/*`），target 目录是 `./target`；写成 `src-tauri` 会缓存错目录，等于没缓存 |
| pnpm 固定 9 | 与 `.workflow/prune-ci.yml`（Gitee Go）保持一致，避免两套 CI 装出不同依赖树 |
| 不加 `--bundles` 限制 | `tauri.conf.json` 是 `targets: "all"`，Windows 会同时产出 `.msi` + NSIS `.exe`，多一个包不亏 |

## 三、产物清单

| 平台 | 文件 | 说明 |
|---|---|---|
| macOS arm64 | `Prune_<version>_aarch64.dmg` | M 系列芯片 |
| macOS x64 | `Prune_<version>_x64.dmg` | Intel 芯片 |
| Windows x64 | `Prune_<version>_x64-setup.exe` | NSIS，推荐分发给普通用户 |
| Windows x64 | `Prune_<version>_x64_en-US.msi` | MSI，适合域环境/静默安装 |

## 四、一次性配置

**必需：无。** `GITHUB_TOKEN` 由 Actions 自动注入，`permissions: contents: write` 已声明，仓库设置里不需要额外配 secret。

**可选（正式签名 / 公证）**：在仓库 Settings → Secrets and variables → Actions 添加：

| Secret | 用途 |
|---|---|
| `APPLE_CERTIFICATE` | `.p12` 证书的 base64（`openssl base64 -A -in cert.p12`） |
| `APPLE_CERTIFICATE_PASSWORD` | 导出 `.p12` 时设的密码 |
| `APPLE_SIGNING_IDENTITY` | 证书名，如 `Developer ID Application: XXX (TEAMID)`；配上后自动替换 ad-hoc 签名 |
| `KEYCHAIN_PASSWORD` | CI 临时钥匙串密码（随便设） |
| `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` | 公证（notarization）用 |

工作流里写的是 `APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY || '-' }}`：
secret 没配就退化成 `-`（ad-hoc 签名）。这一步不能省 —— Apple Silicon 上**未签名**的应用
打开会直接报「已损坏」，ad-hoc 签名至少能靠右键 → 打开放行。

Windows 目前未签名，SmartScreen 会提示「未知发布者」，需要 EV 代码签名证书才能免提示。

## 五、和 Gitee 发版的关系

两条链路互不干扰，各管一个 Release 平台：

| 链路 | 谁构建 | 产物去处 | 适用 |
|---|---|---|---|
| GitHub Actions（本文） | GitHub 托管 runner | GitHub Draft Release | 想让 tag 一推就双平台出包 |
| `pnpm release`（`docs/release-gitee.md`） | 你自己的 mac/win 机器 | Gitee Release | 国内下载、给 Gitee 用户 |

`pnpm release` 现在会**同时**把 tag 推到 Gitee 和 GitHub（GitHub 用来自动触发 Actions）；
不想触发就加 `--no-github`。

## 六、版本号纪律

`verify` 会硬性校验三处版本号一致：

- `package.json` → `version`
- `src-tauri/tauri.conf.json` → `version`
- `src-tauri/Cargo.toml` → `[package] version`

以及 tag 名必须等于 `v<version>`。所以打 tag 前一律先 `pnpm bump <patch|minor|major|x.y.z>`，
不要手改版本号，也不要复用已存在的 tag（tag 已推到远端后 `release.yml` 的 `git tag -f` 会推不上去）。

## 七、排障

| 现象 | 原因 / 处理 |
|---|---|
| `verify` 报版本号不一致 | 忘了 `pnpm bump`，或只改了 `package.json` |
| `verify` 报 tag 与版本号不匹配 | 先 bump 再打 tag，顺序不能反 |
| Windows job 在 `cargo build` 处挂 | 大概率是 `cfg(windows)` 分支没在本机验证过（本机无 MSVC 工具链），看日志定位；`docs/windows-support-plan.md` 有背景 |
| macOS 产物被 Gatekeeper 拦 | 预期行为，未公证；右键 → 打开，或 `xattr -cr /Applications/Prune.app` |
| 缓存一直不命中 | 检查 `swatinem/rust-cache` 的 `workspaces` 是否仍是 `. -> target`（仓库改结构时要跟着改） |
