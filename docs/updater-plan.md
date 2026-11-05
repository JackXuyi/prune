# GitHub 源自动升级（Tauri v2 Updater）

> 目标：Prune 客户端以 GitHub Release 为源自动升级——启动静默检查 + 托盘「检查更新」+ 面板更新卡片，三入口共用一套 Rust 逻辑。

## 一、整体链路

```
push tag v* → GitHub Actions 三平台出包（Draft）
           → tauri-action 生成升级产物（.sig 签名 + latest.json 清单，三 job 自动合并）
           → 你试装后点 Publish
           → https://github.com/JackXuyi/prune/releases/latest/download/latest.json 生效
           → 老客户端 5 s 内静默比对版本 → 发现新版 → 面板卡片 → 下载（minisign 校验）→ 安装 → 重启
```

| 环节 | 谁负责 |
|---|---|
| 构建升级产物 + latest.json | 现有 `release.yml` 的 tauri-action（已加 `includeUpdaterJson: true`） |
| 签名升级产物 | minisign 密钥对（私钥走 GitHub Secrets，公钥内置客户端） |
| 检查 / 下载 / 安装 | `src-tauri/src/updater.rs`（`tauri-plugin-updater` 2.14.x） |
| UI | 托盘菜单项 + `src/components/UpdateCard.tsx`（面板 footer 上方） |

## 二、关键设计决策

| 决策 | 原因 |
|---|---|
| 更新逻辑全在 Rust 侧，前端走自定义命令 | 自定义命令**不走插件权限门**，capabilities 保持最小权限（仍只有 `core:default`），不用为 updater JS API 放权 |
| 状态存 `UpdateState` + 事件广播 | 托盘检查时面板多半是隐藏的；事件照收，面板重开后 `update_pending` 兜底查询，状态不丢 |
| 启动静默检查（延迟 5 s） | 「沉默即正常」：已是最新不发任何事件，不打扰 |
| `download_and_install` + `app.restart()` | 插件 2.14.1 语义：macOS/Linux 装完需手动重启（`restart()` 返回 `!`）；Windows 启动 NSIS 安装器后自动退出 |
| 空 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 密钥对以空密码生成（`-p ""`），CI 与本地都不用管密码 |

## 三、一次性配置（已完成 / 待做）

| 项 | 状态 | 说明 |
|---|---|---|
| 密钥对生成 | ✅ 已生成 | 私钥 `~/.tauri/prune-updater.key`（**不进仓库**），公钥已写入 `tauri.conf.json` |
| GitHub Secrets | ⬜ 手动 | Settings → Secrets and variables → Actions 添加两个 secret（值见下） |
| capabilities | ✅ 无需改 | 自定义命令不进 ACL |
| Draft Release 流程 | ✅ 兼容 | `releases/latest/download/latest.json` 只指向已 Publish 的 Release，「试装后再 Publish」纪律不变 |

Secret 内容（本地取值）：

```bash
# TAURI_SIGNING_PRIVATE_KEY（整段贴入，包括首尾行）
cat ~/.tauri/prune-updater.key

# TAURI_SIGNING_PRIVATE_KEY_PASSWORD（留空字符串即可，密钥无密码）
```

> ⚠️ **私钥丢了 = 已发布客户端永远无法再自动升级**（换公钥需要用户重装）。
> 建议把 `prune-updater.key` 备份进密码管理器。

## 四、代码落点

| 文件 | 内容 |
|---|---|
| `src-tauri/src/updater.rs` | 检查 / 待装状态 / 下载安装三个命令 + 启动静默检查 + 事件契约 |
| `src-tauri/src/lib.rs` | 注册插件 + `UpdateState` + 启动检查 spawn |
| `src-tauri/src/tray.rs` | 右键菜单「检查更新」项（异步派发，不阻塞菜单线程） |
| `src-tauri/tauri.conf.json` | `bundle.createUpdaterArtifacts: true` + `plugins.updater`（endpoint + pubkey） |
| `src/components/UpdateCard.tsx` | 面板更新卡片：待更新 / 下载进度 / 重启提示 / 「已是最新」轻提示 |
| `src/lib/ipc.ts` | `update_check` / `update_pending` / `update_install` 封装 |
| `.github/workflows/release.yml` | 签名 env + `includeUpdaterJson: true` + 私钥 preflight |

事件契约（Rust → 前端，`app.emit` 广播全部窗口）：

| 事件 | 负载 | 时机 |
|---|---|---|
| `update://available` | `{ status: "available", version, currentVersion, notes, date }` | 发现新版（启动静默 / 手动共用） |
| `update://check-result` | `{ status: "up-to-date" }` 或 `{ status: "error", message }` | 仅手动检查的无更新 / 出错回执 |
| `update://progress` | `{ downloaded, total }` | 下载中（total 可为 null） |
| `update://installed` | — | 安装完成，即将重启 |

## 五、产物形态

| 平台 | 升级产物 | 说明 |
|---|---|---|
| macOS | `Prune_*_aarch64.app.tar.gz` + `.sig`（x64 同理） | updater 用 tar.gz，不是 dmg |
| Windows | `Prune_*_x64-setup.exe` + `.sig` | NSIS 安装包直接复用 |
| 三平台共用 | `latest.json` | tauri-action 三个并行 job 各生成一份，自动合并进 Release |

`latest.json` 里带 minisign 签名，客户端内置公钥校验——**与 Apple 代码签名是两套独立机制**，ad-hoc 签名 / 未公证不影响升级链路。

## 六、验证与发版纪律

1. **本地跑通链路**：`pnpm tauri dev` → 托盘右键「检查更新」→ 应提示「已是最新」（latest.json 尚无内容时会报错，属预期——还没发过带升级产物的版本）。
2. **真实升级验证**：装 v0.3.0 → `pnpm bump` 到 v0.3.1 → 推 tag → CI 出包 → Publish Release → 老版本点「检查更新」→ 卡片出现 → 立即更新 → 应用重启为新版本。
3. 版本号纪律不变：`pnpm bump` 三处统一，tag 必须等于 `v<version>`。
4. 本地 `pnpm release`（Gitee 链路）如需产出升级包：本机导出 `TAURI_SIGNING_PRIVATE_KEY=$(cat ~/.tauri/prune-updater.key)` 再构建；否则只影响升级产物，不影响基础安装包。

## 七、排障

| 现象 | 处理 |
|---|---|
| verify 报 `TAURI_SIGNING_PRIVATE_KEY 未配置` | 按第三节配 Secrets |
| bundle 阶段报签名相关错误 | 检查私钥是否完整（含首尾注释行）、是否被 base64 折行 |
| 客户端检查一直报错 | 确认 Release 已 **Publish**（Draft 不可发现）；确认 latest.json 在 Release 资产里 |
| 「发现新版本」但不下载 | 检查 latest.json 中 platform 条目的签名 / url 是否与资产一致（多 job 并发合并失败的兜底：重新跑一次 workflow） |
| 升级后版本没变 | 确认 `pnpm bump` 真的递增过、tag 没有复用 |
