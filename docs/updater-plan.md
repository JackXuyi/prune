# GitHub 源自动升级（Tauri v2 Updater）

> 目标：Prune 客户端以 GitHub Release 为源自动升级——启动静默检查 + 托盘「检查更新」+ 设置页「检查更新」+ 面板更新卡片，四入口共用一套 Rust 逻辑。

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
| UI | 托盘菜单项 + 设置页「关于」卡片（`AboutSection.tsx`）+ `src/components/UpdateCard.tsx`（面板 footer 上方）+ `src/windows/UpdateWindow.tsx`（独立「检查更新」弹窗） |

## 二、关键设计决策

| 决策 | 原因 |
|---|---|
| 更新逻辑全在 Rust 侧，前端走自定义命令 | 自定义命令**不走插件权限门**，capabilities 保持最小权限（仍只有 `core:default`），不用为 updater JS API 放权 |
| 状态存 `UpdateState` + 事件广播 | 托盘检查时面板多半是隐藏的；事件照收，面板重开后 `update_pending` 兜底查询，状态不丢 |
| 手动检查反馈用独立 `update` WebView 窗口，不用原生 dialog | native dialog 是纯文本，markdown（标题层级 / 加粗 / 行内代码 / 链接）全退化成难看字符；WebView 窗口能真正渲染 markdown + 自定义样式 |
| 窗口数据「先写 state 再 show + emit，前端 mount pull 兜底」 | Tauri 事件不重放：若窗口还没 listen 就 emit 会静默丢失。写 state 保证 mount 时一定拉得到，事件只用于已 mount 窗口的刷新 |
| markdown 渲染器零依赖自研 | 保持包体精简；CHANGELOG / release notes 只用小语法子集。React 元素组装、无 `dangerouslySetInnerHTML`，无 XSS 面 |
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
| `src-tauri/src/updater.rs` | 检查 / 待装状态 / 下载安装 + 启动静默检查 + 事件契约 + `check_and_prompt`（立即弹 `update` 窗口显示 checking、后台检查后刷新）+ `UpdateDialogState` / `update_dialog_data` + `include_str!` CHANGELOG markdown 摘要（关窗走前端内置 API） |
| `src-tauri/src/lib.rs` | 注册 updater + dialog 插件 + `UpdateState` / `UpdateDialogState` + 启动检查 spawn + `update` 窗口的 Space / 关闭行为 |
| `src-tauri/src/tray.rs` | 右键菜单「检查更新」项（异步派发，不阻塞菜单线程）→ 调 `check_and_prompt` |
| `src/components/AboutSection.tsx` | 设置页「关于」卡片的「检查更新」按钮 → `updateCheckDialog()` → 同一 `check_and_prompt` |
| `src-tauri/tauri.conf.json` | `bundle.createUpdaterArtifacts: true` + `plugins.updater`（endpoint + pubkey）+ `app.windows` 声明 `update` 窗口（固定 800×600 / 不可调整 / 无边框 / 透明 / 置顶 / center） |
| `src-tauri/capabilities/default.json` | `windows` 加 `update` label（否则新 webview 拿不到 `core:default`，invoke/listen 被拒） |
| `src-tauri/capabilities/update.json` | update 窗口额外授 `core:window:allow-hide`——前端 `getCurrentWindow().hide()` 关窗必需；`core:default` 只含查询类窗口权限，缺它时 hide 被静默拒绝（点了 X / 稍后没反应） |
| `src/App.tsx` | 按窗口 label 分发，`update` → `<UpdateWindow>`（跳过 mem/disk init） |
| `src/windows/UpdateWindow.tsx` | 检查更新弹窗：四态（检查中 / 新版 / 最新 / 出错）+ 下载进度 + markdown 说明；mount pull + `update://dialog` 事件刷新；固定窗口内顶栏/底栏不动、主体内容超长滚动；「已是最新」态紧凑（图标圈 36px、间距压缩）优先保证日志不滚动 |
| `src/components/Markdown.tsx` | 零依赖轻量 markdown 渲染器（React 元素组装，无 `dangerouslySetInnerHTML`，链接只放行 http(s)） |
| `src/components/UpdateCard.tsx` | 面板更新卡片：待更新 / 下载进度 / 重启提示 / 「已是最新」轻提示 |
| `src/lib/ipc.ts` | `update_check` / `update_pending` / `update_install` / `updateCheckDialog` / `updateDialogData` 封装（关窗走 Tauri 内置 `getCurrentWindow().hide()`，不占 IPC） |
| `.github/workflows/release.yml` | 签名 env + `includeUpdaterJson: true` + 私钥 preflight + `release-notes` job 把 latest.json 的 `notes` 改写为 CHANGELOG 本版段 |

### 弹窗里的「更新说明」从哪来（两种场景，两个数据源）

| 场景 | 说明来源 | 原因 |
|---|---|---|
| 已是最新版本 | 本地 `CHANGELOG.md` 当前版本段（`include_str!` 编译期嵌入），转成 markdown（`###` 小标题升为 `##`、`-` bullet 原样保留加粗/代码/链接） | 插件在「已是最新」时 `check()` 返回 `None`，把元数据全丢弃，弹窗拿不到任何 notes；本地 CHANGELOG 是唯一可靠来源，零网络、零新依赖，也不碰 D2 启动指标 |
| 发现新版本 | `update.body`（来自 latest.json 的 `notes` 字段） | 插件只在发现新版时返回 `Update`（含 body）。该字段历史上是静态下载表模板，`release-notes` job 现已在发布时把它改写为 CHANGELOG 本版段，弹窗/卡片才看得到真正的变更说明 |

说明以 markdown 原文交给 `update` 窗口渲染（不再截断成单行）；窗口固定 800×600，内容区超出滚动，长 changelog 完整呈现。

事件契约（Rust → 前端，`app.emit` 广播全部窗口）：

| 事件 | 负载 | 时机 |
|---|---|---|
| `update://available` | `{ status: "available", version, currentVersion, notes, date }` | 发现新版（启动静默 / 手动共用） |
| `update://check-result` | `{ status: "up-to-date" }` 或 `{ status: "error", message }` | 仅手动检查的无更新 / 出错回执 |
| `update://progress` | `{ downloaded, total }` | 下载中（total 可为 null） |
| `update://installed` | — | 安装完成，即将重启 |
| `update://dialog` | — | 检查结果已写入 `UpdateDialogState`，通知 `update` 窗口来 pull（仅手动检查；点击时也会发一次，让窗口 pull 到 checking 态） |

`update` 窗口另有一个 command：`update_dialog_data`（pull 最近一次结果）。关闭窗口直接用 Tauri 内置 `getCurrentWindow().hide()`——不依赖自定义 command 注册，避免 dev 只热重载前端时旧二进制报 `Command update_dialog_close not found`。窗口尺寸在 `tauri.conf.json` 固定为 **800×600 且不可调整**：顶栏与底栏固定，主体内容区 `overflow-y-auto`，短内容居中留白、超长 changelog 在窗口内滚动。

**「点了就弹」**：`check_and_prompt` 同步写入 `UpdateDialogPayload::Checking` 并 show 窗口（前端立即显示「正在检查更新…」转圈），检查在后台 spawn；结果回来后覆盖 state 并再发 `update://dialog`，窗口 pull 到结果态刷新。所以慢网络不会让点击「没反应」。payload 共四态：`checking` / `available` / `upToDate` / `error`（serde tag=status、content=data；`checking` 无 data）。窗口是 hide/show 复用、组件不卸载，关闭时前端把 payload 复位到 checking，避免下次显示闪旧结果。

## 五、产物形态

| 平台 | 升级产物 | 说明 |
|---|---|---|
| macOS | `Prune_*_aarch64.app.tar.gz` + `.sig`（x64 同理） | updater 用 tar.gz，不是 dmg |
| Windows | `Prune_*_x64-setup.exe` + `.sig` | NSIS 安装包直接复用 |
| 三平台共用 | `latest.json` | tauri-action 三个并行 job 各生成一份，自动合并进 Release |

`latest.json` 的 `platforms` 每一项里带对应安装包的 minisign 签名（`.sig`），客户端内置公钥在**下载安装包后**校验其字节——**与 Apple 代码签名是两套独立机制**，ad-hoc 签名 / 未公证不影响升级链路。注意 `latest.json` 文件本身不签名，`release-notes` job 改写它的 `notes` 字段不影响任何校验（只动文本，`platforms` 的 URL 与签名逐字保留）。

## 六、验证与发版纪律

1. **本地跑通链路**：`pnpm tauri dev` → 托盘右键「检查更新」/ 设置页「检查更新」→ **立即弹出独立 `update` 窗口**，先显示「正在检查更新…」转圈，检查完成后再刷新为结果（不等网络、点击即有回应）：已是最新显示「已是最新版本 + v<version> + 本版本更新说明（本地 CHANGELOG 的 markdown，可滚动）」；发现新版显示「立即更新 / 稍后」+ markdown notes（点更新走下载安装+重启，带进度条）。之所以用独立窗口而非面板卡片 / 原生 dialog：托盘点击时面板多半隐藏，只发事件用户看不见；原生 dialog 又渲染不了 markdown——WebView 窗口保证「点了必有回应 + 说明好看」，事件仍照发，面板开着时卡片同步更新。浏览器里可用 `?window=update` 预览结果态、`?window=update&checking=1` 预览转圈态。（latest.json 无内容时显示错误态，属预期——还没发过带升级产物的版本。）
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
