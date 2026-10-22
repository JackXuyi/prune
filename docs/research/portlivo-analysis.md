# Portlivo 应用分析

> **结论先行**：Portlivo 是一个 macOS 菜单栏的「本地开发服务器管理器」。技术上没有魔法——它把 `lsof` 的裸输出，通过 **进程自省（libproc / KERN_PROCARGS2）** 翻译成有身份的对象（项目、命令、PID、来源应用），再叠加健康探针、日志、分享三条旁路。真正的护城河不是某个 API，而是**权限克制**：它明确不要辅助功能/全盘访问/屏幕录制，这个约束反向决定了整个功能边界。

---

## 1. 产品定位

| 维度 | 内容 |
|---|---|
| 形态 | macOS 菜单栏原生 App（SwiftUI），另有独立窗口形态 |
| 一句话 | Every dev server. One click away. |
| 痛点 | 本地端口是一团 `lsof -i` 输出：看不出属于哪个项目、打不开、杀不掉、端口被占 |
| 人群 | 同时跑多项目的 Web/PHP 全栈开发者（Vite / Laravel / Next.js / Docker 混跑） |
| 差异化 | 不是「端口列表」，而是「有名字的服务器 + 可执行操作 + 可分享」 |
| 商业化 | 一次性买断，7 天全功能试用；$8.99（早鸟）→ $19.99 / 3 台 $39.99 / 5 台 $59.99 |
| 版本 | v1.0.0，2026-10-17 发布 |

---

## 2. 功能图谱

| 层级 | 能力 |
|---|---|
| **感知层** | 2s 轮询扫描、按类型分组（Web / 数据库与容器）、LAN 暴露标记、Herd/Valet 站点、Homebrew 服务、Docker 容器 |
| **识别层** | 项目名 + 文件夹 + 命令 + PID + 运行时长、技术栈图标（Vite/Laravel/nginx/Postgres…）、来源应用（Cursor/VS Code/iTerm/Warp）、Agent 徽标（Claude Code / Codex） |
| **操作层** | 打开 / 复制 URL / 重启 / 杀进程 / 堆栈一键启停 / Stop Visible / 撤销 |
| **诊断层** | HTTP 健康探针（状态码着色 + 毫秒级耗时 + 自定义 /health）、CPU/内存趋势、实时日志 |
| **分发层** | LAN 地址 + QR 码、cloudflared / ngrok 公网 HTTPS |
| **效率层** | ⌥⌘P 全局面板、⌘K 命令面板、键盘优先、`devports` CLI、Shortcuts 自动化 |

---

## 3. 实现原理拆解

### 3.1 端口扫描（地基）

- **事实**：每 2s 扫描一次；宣称不需要 Full Disk Access / 辅助功能。
- **推断实现**：`lsof -nP -iTCP -sTCP:LISTEN`，或等价的 libproc 路线（`proc_listallpids` → `proc_pidfdinfo` / `PROC_PIDFDSOCKETINFO`）。
- **为什么能不要权限**：macOS 上非 root 的 `lsof` 只能枚举**当前用户拥有**的进程——而 dev server 恰好全是用户态进程。这是整个「零权限」叙事成立的前提。
- **LAN 徽标**：来自 socket 的绑定地址——`127.0.0.1:5173` 安全，`*:5173` 或 `192.168.1.24:5173` 即判定对外暴露。

### 3.2 进程画像（核心增值）

| 字段 | 推断来源 |
|---|---|
| 命令 + 参数 + 环境变量 | `sysctl KERN_PROCARGS2`，一次调用返回 exec_path、argv、env、cwd |
| 项目文件夹 → 项目名 | 进程的 CWD（同一 KERN_PROCARGS2）；技术栈图标由目录内 `package.json` / `composer.json` 推断 |
| 来源应用 | 沿 **PPID 链上溯**到首个带 bundle 标识的进程 → `NSWorkspace` 取名与图标；`T` 键前置用 `NSRunningApplication.activate()`，**不需要辅助功能权限** |
| Agent 徽标 | 祖先链中出现 `claude` / `codex` / `cursor-agent` 进程名 |
| Safe kill | kill 前**重新读取该 PID 的 argv/启动时间**与快照比对，防 PID 复用误杀 |
| 噪音过滤 | 进程名黑名单：Gradle / Kotlin daemon、各类 language server、系统守护进程 |

### 3.3 健康探针

- 对每个 Web 端口发 HEAD/GET，记录**状态码 + 响应耗时**；2xx 绿 / 3xx 青 / 4xx 橙 / 5xx 红。
- **推断用 Network.framework 的 `NWConnection`** 而非 `URLSession`：后者会走系统 PAC 代理配置，对 localhost 探测会失真，而 NWConnection 可显式绕过并精确计时。
- 支持把探针路径从 `/` 改成 `/health`。

### 3.4 预览缩略图

`WKWebView` 离屏渲染 + `takeSnapshot` 取图（CoreImage 生态里还有 `CIQRCodeGenerator` 直接生成分享 QR，无需三方库）。

### 3.5 实时日志（三条独立通路）

| 日志来源 | 通路 |
|---|---|
| `laravel.log` 等文件 | `DispatchSource` 文件监听 + 记录偏移量续读 |
| `docker logs -f` | 拉起子进程 + Pipe 增量读取 |
| Portlivo 自己启动的进程 | 启动时就持有 stdout 管道，天然可 tail |

着色 = 对文本流做 `INFO/WARN/ERROR` 正则匹配，无复杂解析。

### 3.6 堆栈发现（纯静态，无 AI）

指向一个文件夹后，按优先级嗅探约定文件：`package.json` 的 scripts、`composer.json` 的 `run dev` / `artisan serve`、`Procfile` / `Procfile.dev` / `bin/dev`（Rails）、`manage.py`（Django）、`go run`、docker-compose.yml。

### 3.7 分享与隧道

1. LAN IP：`getifaddrs` 取 `en0` 地址。
2. QR：`CIQRCodeGenerator` 编码 LAN URL。
3. 公网：拉起 `cloudflared tunnel --url http://localhost:5173` 或 `ngrok http 5173` 子进程，**从 stdout 正则抠出 URL**，App 退出时回收子进程。

### 3.8 端口冲突自愈（⚠️ 最值得质疑的一处）

官方文案：`npm run dev` in acme-web 启动失败 → 通知提示 PID 41877 占用 → 一键 **Kill Holder & Retry**。

**最可能的实现**：只对 **Portlivo 自己启动**的进程生效。它持有子进程 stdout → 正则匹配 `EADDRINUSE` / `address already in use` → 从当前快照反查占用者 PID → 弹通知 → 点击后 kill 该 PID，并用事先保存的原始 argv + env 重启（这也是 "Exact restarts" 复用的同一份数据）。

**为什么我怀疑它不通用**：在**没有辅助功能权限**的前提下，它读不到 Terminal.app / iTerm / Warp 里的输出。所以「你在外部终端手敲 `npm run dev` 撞端口」这条路径，除非它用了别的招（例如轮询发现某端口上有进程反复启动又快速消失），大概率覆盖不到。**这一点建议实测验证，是购买前最该确认的能力边界。**

---

## 4. 整体架构推演

```
[ 扫描引擎 ]  lsof / libproc  ──每2s──▶  新快照
                                          │
                                    diff（稳定 ID = pid+port，避免列表闪烁/丢失选中）
                                          │
                                          ▼
                              @Observable 状态容器
                                          │
        ┌─────────────────┬───────────────┼──────────────┬──────────────┐
        ▼                 ▼               ▼              ▼              ▼
   菜单栏面板        主窗口+检视器    ⌘K 命令面板    devports CLI   Shortcuts
   (⌥⌘P 热键)                                        (XPC 通信)    (AppIntents)
        │
        └── 旁路：HTTP 探针 / WKWebView 预览 / 日志 tail / kill·restart 子进程
```

- **稳定 ID 是关键工程细节**：2s 刷新下若用数组下标做身份，列表会闪烁、选中态会丢。必须按 `(pid, port)` 建键并 diff。
- **⌥⌘P 全局热键**：推断用 Carbon `RegisterEventHotKey`（不触发辅助功能授权），面板为自绘 `NSPanel(.nonactivatingPanel)` 以便接管完整键盘导航——SwiftUI 原生 `MenuBarExtra` 对键盘焦点控制不够。

---

## 5. 权限与信任设计（最值得学的一课）

| 项 | 选择 | 连带后果 |
|---|---|---|
| 权限 | **只要通知（且可选）** | 放弃「读取任意终端输出」，EADDRINUSE 只能管自己启动的进程 |
| 权限 | 不做 FDA / 辅助功能 / 屏幕录制 | 预览走 WKWebView 而非截屏，绕开屏幕录制权限 |
| 进程形态 | `LSUIElement` = 无 Dock 图标 | 需要窗口时切 `.regular` 激活策略 |
| 隐私 | 无账号、无埋点 | 只能用本地离线授权校验（推断为公钥签名验证，非在线激活） |
| 分发 | Developer ID 签名 + Apple 公证 | 更新由 Sparkle 2 一类框架签名下发 |
| 登录项 | `SMAppService`（macOS 13+ 现代 API） | — |

> **产品与技术的耦合点**：它把「权限克制」做成了首屏卖点，但代价是功能必须在约束内重新设计。这是独立开发者的典型取舍——用能力边界换信任，再用信任换转化率。

---

## 6. 官网技术栈（实测）

- **Next.js App Router + Turbopack**（HTML 中 `/_next/static/chunks/` 与 `turbopack-3-*.js` runtime chunk 为证）
- 字体自托管 woff2，无第三方 CDN / 分析脚本痕迹
- 落地页结构：Hero（含交互式产品模拟面板）→ Features → The window → devports CLI → Shortcuts → Specs → Pricing → FAQ
- 带 UA 识别：非 macOS 访问时提示「发到你的 Mac 再下载」
- 引流：Product Hunt（`?ref=producthunt`）

---

## 7. 存疑与局限

| # | 问题 | 判断 |
|---|---|---|
| 1 | 外部终端的端口冲突能否被捕获 | 大概率不能（见 3.8），需实测 |
| 2 | 2s 轮询的耗电与 CPU 开销 | 1.0 未见自适应节流/退避策略，这是同类工具的老问题 |
| 3 | 「Agent 徽标」的判定精度 | 若仅靠进程名匹配，Cursor 内嵌终端启动的普通服务可能误标 |
| 4 | 早鸟阶梯涨价（8.99→19.99） | 营销手法，非技术问题，但会影响购买时机判断 |

---

## 8. 三个可迁移的设计决策

1. **把 CLI 输出升级为有身份的对象**——这是产品价值的真正来源，而不是信息罗列。
2. **全局热键 + 命令面板 + 键盘单键（O/T/K）三层效率设计**，覆盖面从任意 App 到面板内再到行级操作。
3. **权限克制即卖点**：先划能力边界，再倒推功能设计，最后把边界写成信任证明。
