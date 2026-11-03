# Gitee 双平台发版流程

> 目标：mac 与 Windows 客户端版本可重复、可追溯地产出，产物统一挂在 Gitee Release 上。

## 为什么分两层

| 层 | 跑在哪 | 职责 | 自动化程度 |
|---|---|---|---|
| Gitee Go 流水线 `.workflow/prune-ci.yml` | Gitee 公用构建机（Linux） | 版本号一致性校验 + `tsc` 类型检查 + `vite` 前端构建 | 全自动 |
| `scripts/release.mjs` | 你自己的 macOS / Windows 机器 | `tauri build` 出安装包 → 打 tag 推送 → 创建/更新 Gitee Release 并上传 | 一条命令 |

原因：Tauri 的 `.dmg`/`.app` 必须在 macOS 上构建，`.msi`/`.exe` 必须在 Windows（MSVC）上构建。
`scripts/check-windows.sh` 里的 windows-gnu 交叉编译只做编译基线检查，不能产出可分发安装包
（`prune-app` 的 cdylib 在 GNU 链接器下会撞 65535 符号上限）。所以安装包由各平台机器各跑一次同一脚本，
两次执行写入同一个 Release，互不覆盖。

## 一次性配置

1. 生成令牌：Gitee → 设置 → 私人令牌 → 新建，勾选 `projects`（仓库读写）
2. 在仓库根目录建 `.env.release`（已被 `.gitignore` 忽略）：

```
GITEE_TOKEN=你的令牌
```

3. 开通 Gitee Go：仓库页 → 服务 → Gitee Go → 开通（需账号绑定手机号）

## 日常发版

**macOS（当前机器）**

```bash
pnpm release patch          # 0.1.0 → 0.1.1：bump → 构建 dmg → 打 tag → 推 Gitee → 建 Release → 传 dmg
```

**Windows 机器（同一 tag，补齐安装包）**

```bash
pnpm release 0.1.1 --skip-build   # 跳过 bump/构建，仅上传已有 msi/exe 到同一个 Release
# 若产物已构建完成但还想重新上传，配合 --skip-build 前先手动 pnpm tauri build
```

常用开关：

| 参数 | 作用 |
|---|---|
| `patch` / `minor` / `major` / `x.y.z` | 版本递增方式 |
| `--skip-build` | 跳过 bump 与构建，直接上传 `src-tauri/target/**/bundle` 下的产物 |
| `--no-push` | 只本地提交+打 tag，不推送 |
| `--dry-run` | 只打印动作，不写文件、不构建、不上传 |

## 产物命名与位置

构建输出在 `src-tauri/target/<target-triple>/release/bundle/`：

| 平台 | 文件 | 说明 |
|---|---|---|
| macOS arm64 | `dmg/Prune_<version>_aarch64.dmg` | 双击安装 |
| macOS x64 | `dmg/Prune_<version>_x64.dmg` | Intel 机器需在 Intel 机器或 `--target x86_64-apple-darwin` 下构建 |
| Windows | `msi/Prune_<version>_x64_en-US.msi` | MSI 安装包 |
| Windows | `nsis/Prune_<version>_x64-setup.exe` | NSIS 安装包 |

## 可选：全自动（自托管构建机）

如果希望 push tag 后完全无人值守，把一台 Windows 机器和一台 Mac 注册为 Gitee Go 构建机，
然后打开 `.workflow/prune-ci.yml` 里已注释的 `bundle-windows` stage，把 `executor` 指向对应资源池。
这样产出的安装包会进「发布记录」，再由流水线末端的 Gitee Release 插件上传（或继续用脚本上传）。

## 签名

| 平台 | 现状 | 正式分发需要 |
|---|---|---|
| macOS | 未签名，首次打开需右键 → 打开，或 `xattr -cr /Applications/Prune.app` | Apple Developer ID 证书 + `notarytool` 公证，写入 `APPLE_CERTIFICATE` / `APPLE_ID` / `APPLE_TEAM_ID` |
| Windows | 未签名，SmartScreen 会提示 | 代码签名证书（EV 证书可免提示） |

## 与 GitHub Actions 方案的关系

现在两条链路并存，各管一个 Release 平台：

| 链路 | 谁构建 | 产物去处 | 适用 |
|---|---|---|---|
| GitHub Actions `.github/workflows/release.yml` | GitHub 托管 runner（macOS + Windows） | GitHub Draft Release | tag 一推就自动出双平台包，见 `docs/release-github.md` |
| `pnpm release`（本文）+ Gitee Go 校验 | 你自己的 mac / win 机器 | Gitee Release | 国内下载、给 Gitee 用户 |

`scripts/release.mjs` 不再写死 `origin`：它按 host 自动识别远程 —— gitee.com 的远程用来传 Release，
github.com 的远程用来推 tag 触发 Actions。所以推 tag 时会同时更新两边，不想触发 Actions 加 `--no-github`。
