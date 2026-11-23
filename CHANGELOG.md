# Changelog

所有可见变更记录于此。版本号由 `scripts/bump-version.mjs` 统一维护（package.json / tauri.conf.json / Cargo.toml 四文件一致：根 workspace `version` + src-tauri `[package] version`）。

## Unreleased

### 端口页「局域网」子页下线（M22，[方案](./docs/m22-port-lan-dedup-plan.md)）

- 端口页收敛为单页（无分段胶囊），局域网唯一入口在设备页（一页两域：本机硬件 + 局域网）——此前端口页子页与设备页画同一份 lanStore 快照的重复入口移除
- 拓扑视图（M20/M20b）代码休眠保留：设备页维持拓扑入口隐藏，`LanTopology.tsx` 与 `lan.topo.*` 文案在库不删，后续恢复只需一处挂载点
- i18n 清理：`ports.subpage.*` / `ports.searchLanPlaceholder` 及零消费死键 `subPorts` / `subLan`（zh/en 配对删除）

## 0.17.0

### 外设页（M21，第 8 个一级导航）（[方案](./docs/m21-peripherals-plan.md)、[设计稿](./output/design/peripherals-eul-mockup.html)）

- 新增「外设」页（⌘8，与设备页边界：设备页 = 本机硬件 + 局域网，外设页 = 插上/连上本机的外部设备），三源独立采样零新依赖：蓝牙/USB 走 `system_profiler -json`（6 s 超时兜底），音频走 CoreAudio 手工 extern；15 s 低频 + 页面可见性门控（不可见零开销），单源失败保持上轮快照
- **AirPods 分件电量**：左耳/右耳/充电盒三环独立显示（IOPS 只有总百分比，分件电量只有 `SPBluetoothDataType` 给）；真机 JSON 结构固化为解析测试 ×3，电量 0/超范围/空串等脏数据一律丢弃
- **卡片化改版**（`7b5dca8`、`1060cc6`）：四段状态带（蓝牙/USB/音频计数 + 最低配件电量环，全页唯一百分比位）→ 每台设备一张卡（AirPods=分件最低环、键盘/鼠标=电量条、USB=总线供电 mA、音频=方向读数，无电量画 — 不画 0）→ 点击弹 430px 详情弹窗（类型/厂商/分件电量/采集时刻，Esc 关闭）
- 设置页新增「外设采样」硬件模块开关（`hw.periph.enabled`，默认开）；读不到电量的部件不画环，无设备的分区整块隐藏

## 0.16.0

### 端口页 eul 式重实现 + 局域网子页同步改版（[设计稿](./output/design/ports-eul-mockup.html)）

- 端口子页：五段状态带（监听·端口域色 / 对外暴露 / 关注 / 活跃连接 / CLOSE_WAIT——全是本页任务内读数，归属盲区降级为提醒带）+ 点阵 Hero（一端口一点，琥珀=对外暴露 / 绿=关注 / 灰=其他）+ 扫描口径块，「重新扫描」从隐形 ⌘R 变显性按钮；端口号升到 15px mono 成第一视觉锚
- 局域网子页：砍三张指标卡换四段状态带（在线 N/M · 暴露 · 新面孔 · 探测覆盖口径——0 探测如实显示「仅邻居表」不画 0）+ 设备点阵 Hero（本机绿 / 网关蓝 / 有入站黄 / 离线淡化）；扫描按钮是唯一执行入口，进入分段绝不自动扫描（本地网络权限弹窗纪律不变）
- 子页头重排：分段胶囊在左、搜索在右，placeholder 随子页切换；局域网子页新增搜索（IP / 主机名 / 厂商 / 类型词）
- 探针确认 / 暴露实测 / 白话解释 / 入站明细 / 基线漂移链路原样保留，PortList 瘦身回 500 行内

### 内存页 eul 式重实现（`c7a096b`）

- 六段状态带（已用/Swap 套环，压力三段电平条）+ 内存构成堆叠条 Hero + 双磁贴（交换空间阈值带、换页颠簸走势）+ 泄漏嫌疑带 + 换出大户表；左栏家族列表 300px
- 左栏家族（应用层级）可选中：右栏新增家族聚合详情（构成条 + 成员进程分摊列表），下钻链路 族 → 进程 → 返回 两级退出

### 存储页 eul 式重实现（`eedbf5d`、`d286ab9`）

- 四段状态带 + 可回收大数字 Hero（L1–L4 构成堆叠条，L4 斜纹不计入）+ 占用反查上移（卷芯片点选免输入）+ 分级清理卡片化；磁贴全宽堆叠、整页单滚动容器、底部操作栏 sticky 钉底

### 网络页重构（`764c333`）

- 诊断页并入测试页，网络页二级 tab 3 → 2；测试页站点区改整页滚动

### 设备页 eul 式重实现（`2006463`）

- 一页两域：7 段状态带（CPU 温度 / 电量环 / 磁盘读:写 / SMART 三态灯 / 在线 / 暴露）+ 域色设备点阵 Hero + 双磁贴（电源与配件、磁盘 I/O·体检）；进入页面绝不自动扫描（本地网络权限弹窗纪律不变）
- 台式机诚实口径：Mac mini 无内置电池时电量环自动降级为「最低配件」（MacBook 上为内置电池，含剩余时间/充电中）；SMART 不支持显示 — 不画 0；磁盘构成条只表达此刻读:写占比，无时序不画走势线
- 局域网在线/暴露数字上提进页面状态带，设备区内不再叠第二条状态带；拓扑视图入口暂隐藏（产品决策，组件保留），端口页局域网子页不受影响
- 删除被本版取代的 HardwareStrip / DiskHardwareStrip（`fba194a`，全仓零引用）

### 设置弹层 eul 式重实现（`f1c2fa4`）

- 单列 560px 滚动长条 → 780 宽双栏分组一屏收齐：左栏常规项（外观与语言 / 系统 / 硬件模块两列 5 开关 / 关于自身开销），右栏灵动岛域（主开关 + 三提报开关 + 六阈值滑杆单行化 + 端口探测）
- 保存状态收敛到弹层头唯一出口（自动保存 / 已保存 HH:MM / 失败红字）；每组 14px 图形锚点；说明文字降级 tooltip（文字三层红线）
- 新增 settings/kit 共享基元（SettingsGroup / ToggleRow / Slider / StatusDot / RuleLine）；体检报告并入系统组一行（CLI 版本点徽 + 生成按钮）
- 修复组卡 `overflow-hidden` 裁切语言下拉菜单的问题；修复内容超高时弹层无法滚动（`38d8d12`）

### 菜单栏面板 eul 式重实现（`46ec1eb`）

- 内存/存储/CPU 三行文字 → 三环 Hero（环色即负载色 ≥90 danger / ≥75 warn，swap、load 等细节进 tooltip，读不到显示 — 不画弧）；端口行 → 端口域色三段带（监听 / 关注 / 暴露，暴露 0 该段不渲染，未扫描不画 0 芯片）
- `portCounts` 纯函数收敛到 statsReadout，面板带与端口行共用同一口径（`af3acee`）
- 顺带修复本地服务行端口号重复显示（「:1420 :1420 node」→「:1420 node」）

### 修复

- 页级滚动容器统一加 `overflow-x-hidden`，杜绝真实数据长文案触发的横向滚动条（`a74d65b`）
- 内存/存储页若干布局截断修复（VictimRow 网格列、InfoTile 高度、展开区挤压）

## 0.15.0

### 端口行「入站来源」（[方案](./docs/f21-inbound-source-plan.md)）

- **零新增采集**：lsof 早已采到 ESTABLISHED 连接的对端地址，此前在解析层被丢弃，现在接住聚合——只统计「本端是监听端口」的连接（连出去的本端是临时端口，天然过滤），inbound 语义自动成立
- 四段分类（本机 / 局域网 / 公网 / 未知——解析失败如实标注，不塞进别的段谎报），外部来源 accent 高亮 + 行下明细（≤5 条，余量可算不谎称全部）；loopback-only 只进 tooltip——自己 curl 自己不算新闻
- 公网连入**不标红**：any 暴露面属正常态，不重复吓人；MCP `port_listeners` 载荷同批透传入站摘要

### 端口可信基线与漂移（[方案](./docs/f22-port-baseline-plan.md)）

- 基线键 (port, proto, comm) **不含 pid**（node 重生 pid 必变，入键则基线废掉）；三类漂移按信息量排序：暴露面变宽（warn 徽标）> 新监听（accent）> 消失（只进 CLI 统计不进 UI）
- CLI `prune port --drift [--rebase]`，GUI 端口行内徽标——**查询态呈现，不进灵动岛**（对齐「冻结第五类事件」裁决，不新增打扰型事件）
- 首装 / rebase 后不整表标「新」：seed 行按「天生老住户」落库 + 24 h 新鲜期严格小于判据（两条真机踩坑纪律，均带单测）；CLI 与 GUI 采样轮共享同一张基线表

### 磁盘页「Open Files 占用反查」+ 新子命令 `prune files`（[方案](./docs/f23-open-files-plan.md)）

- 子命令 11 → 12：`prune files <PATH> [--json]` 反查谁开着这个路径下的普通文件；磁盘 tab 顶部同款反查卡（卷下拉 + 手输兜底，TTL 5 min 缓存不反复付 300 ms 的 lsof）
- 只算普通文件（REG/VREG），目录与 socket 不算占用；**占用者列表无任何执行入口**——只读纪律，反查不做「解除占用/弹出」
- 盲区说明常驻不折叠：以当前用户权限运行，「没有可见的占用者」≠「没人占用」；macOS tREG 与 GNU tVREG 双兼容、lsof 真实路径（/tmp → /private/tmp）先 canonicalize 再前缀匹配

### 内存历史导出 CSV（[方案](./docs/f20-memory-report-plan.md)）

- `prune mem --csv <PATH>` 与内存页「导出 CSV」按钮走**同一聚合函数**，两侧导出逐字节一致；GUI 导出走原生 save 对话框，取消 = 无事发生
- (name, family) 聚合维度（进程重生的 pid 噪音不进报告）、峰值取原始样本不降采样、**空窗（gaps）进 CSV**——Prune 没跑的时段不让读者误以为「当时很干净」
- 窗口为序列库保留期 7 天；导出含峰值/均值/峰值时刻与换算 MB 列，供工单与 issue 附带

### 端口白话解释 `prune why`：GUI + CLI + MCP 三通道（[方案](./docs/f16-port-why-plan.md)）

- **三个通道同一份结论**：CLI `prune why <PORT>`、MCP `prune_why`（工具面 12 → 13）、GUI 端口行 hover「为什么？」——判定收敛在核心纯函数，排版层只搬运，不可能各说各话
- **(port, comm) 双匹配防误报**：普通进程占 5000 不会被说成 AirPlay；规则不命中走 Unknown 如实报观测事实，不编造用途；已采集证据上的静态推断标「疑似」（KnownService）
- 无监听严格三态：确认空闲 / Coverage 盲区 / TIME_WAIT，互不冒充；系统服务与冲突结论上色警示——杀掉会复活，指路设置页而不是进程列表

### 端口页「测一下」：暴露面可达实测（[方案](./docs/f17-exposure-reach-plan.md)）

- 对本机默认路由网卡地址发起一次 TCP connect，四态分层；Failed 与 NoIfaceAddr 诚实兜底——未映射的 io 错误不伪装成 Timeout，文案永不写「被拦截」类定因
- **用户显式触发**（行内按钮），绝不批量；caveat 永久标注「同机回连 ≠ 其他设备可达」；loopback 端口不出按钮——「本机可达」是 loopback 的定义使然，测了没有信息量

### 灵动岛「闲置 dev server 提报」（实验功能，默认关，[方案](./docs/f18-idle-server-plan.md)）

- 主判据 = 项目归属端口 established == 0 持续 30 分钟；conns 缺数宁漏勿误；复用端口守望同一节拍与快照，零新增采集
- 琥珀色第五类事件 + 点击深链端口页；**默认关闭**——遵守「冻结第五类事件」裁决，设置页标「实验功能」opt-in，默认值翻转以用户访谈为门禁

### 设置页关于区「自耗透明」（[方案](./docs/f19-self-footprint-plan.md)）

- 内存 / CPU / 线程 / 数据库四行读数，与内存页同一 probe 同口径，可实测核对一致性；缺数显「暂无」不显 0（深探失败不回退 RSS 冒充）
- 只展示数字不做自夸：数值随版本会变，除「不设上限承诺」外不写任何会被打脸的声明

### 「按应用」网速读数防抖（EWMA 平滑）

- **数字不再逐秒跳变**：nettop 按进程字节是突发性的（TCP 一次性灌接收缓冲），1 s 差分在「满速 ↔ 0」间来回跳；展示口径改为指数平滑（α=1/3，约 3 s 收敛），新流量首次观测取原始值不被平滑低估。「应用资源」网格与应用详情共用同一 store，一并受益
- **排名不洗牌**：相邻应用相差 < 5% 视为同档按名称排，不再随噪声互换位置；无流量的应用按平滑值衰减淡出列表，不再整行闪现/消失
- **只动展示层**：后端差分基线、今日流量、60 s 历史累计口径不变（在 prune_core），平滑不污染统计

### 设置页新增「开机自启动」（[方案](./docs/autostart-plan.md)）

- **登录后菜单栏自动待命**：开关打开后写入 macOS LaunchAgent plist（`~/Library/LaunchAgents/`），登录时自动拉起 Prune——守门员形态不变，不弹主窗口；关闭开关即移除 plist
- **官方 `tauri-plugin-autostart` 插件 + 自定义 command 收口**：capabilities 纪律不变（前端权限仍只有 core:default），IPC 走 `autostart_get` / `autostart_set` 两个 Rust 命令，与 updater「不走 JS ACL」同一约定
- **状态不落 settings.json**：`is_enabled()` 直接问系统（plist 是否存在），用户在「系统设置 → 登录项」里手动增删也能如实反映，两处管理不冲突
- **默认关**：常驻 App 开机拉起是侵入性行为，用户显式授权才开；开关乐观更新、失败回滚并在设置页报错

### `prune reclaim --emit-script`：打印候选删除命令，永不执行（[D-15 方案](./docs/d15-emit-script-plan.md)，DP-4 裁 A）

- **开一条「打印命令」的缝**：把 reclaim 预览的 L1 白名单项命令化成一段可审计的 POSIX sh 脚本输出到 stdout，执行权完全留在人手里——Prune 进程内不存在执行该脚本的代码路径，`emit_script.rs` 源码断言单测守门（禁词：进程启动 / 废纸篓原语 / 文件删除）
- 每条命令 `mv -n '<路径>' "$HOME/.Trash/"`：只进废纸篓、不用 `rm`、`-n` 不覆盖同名条目；路径经单引号转义，空格 / 引号 / `$()` / 反引号全部失效在引号内
- **逐项白名单复验**：emit 通道不豁免最后一道闸——复验不过的项不进脚本并 stderr 告警；每项注释带体积、分组与校验结果，脚本头自证生成时间 / 版本与边界声明
- `--emit-script` 与 `--yes` 互斥（同给用法错退出 2）；范围与 reclaim 一致只取 L1，L3 不进脚本、L4 永不提供删除命令

### `prune watch` 吃 CWD 项目的 `watch.ports`（[W2 收尾方案](./docs/wave2-cli-plan.md)）

- **修一个真实告警缺口**：watch 的端口关注层此前只含在线项目（对绑定进程的 cwd 解析后读配置）；在还没起服务的项目里跑 watch，该项目的关注端口起服务那一刻不会告警——`PortGuardMonitor` 的 NewListen 只看条目的 watched 标志
- 现在启动时从 CWD 向上找最近一份 `prune.toml` 取 `watch.ports` 并入关注层；人读横幅显示配置来源；`--json --once` 的 ports 段新增 `watchedExtra`（ports + source）。无配置目录下输出与 0.14.0 逐字节一致（顶层键集合不变，契约快照不受影响）
- **就近纪律**：就近的配置损坏（语法错 / 超子集）不向上降级到父目录——「子目录坏了却用父目录的配置」的优先级无法向用户解释，与「损坏整份忽略」一致

## 0.14.0

### 主窗口出现时显示 Dock 图标（[方案](./docs/dock-icon-plan.md)）

- **守门员形态不变，Dock 图标跟随主窗口**：启动仍是 Accessory 常驻菜单栏；打开主窗口时切 Regular 显示 Dock 图标，点红叉隐藏窗口后收回 Accessory。切换收口在唯一打开函数 `tray::show_main` 与唯一关闭路径 `CloseRequested`，面板 / 灵动岛 / 托盘 / 菜单所有入口自动生效；`AtomicBool` 去重防止连点导致图标弹跳，`RunEvent::Reopen` 兜底 Dock 图标点击
- **不用 tao 的 `set_dock_visibility`**：它底层走 `TransformProcessType`，快速切换会触发 macOS 残留多个 Dock 图标的系统级 bug（tao 源码注释承认并只能做 1 s 去抖规避），全程沿用启动代码同一机制 `set_activation_policy`
- **dev 裸二进制图标补齐**：`tauri dev` 跑的非 bundle 二进制切 Regular 后只能显示通用 exec 图标，`dock::set_app_icon` 启动时把 `include_bytes!` 进二进制的 icon.png 设给 `NSApp.setApplicationIconImage`；release 下重复设置同款图标无害
- **主窗口 960×640 → 1000×700**：`position_main` 定位兜底与三处引用注释同步

### 菜单栏面板贴合内容 + 原生主题底衬同步

- **贴高链路去掉呼吸余量**：前端只上报表面自然高（`offsetHeight`，外层 `p-2` 移除），Rust 侧不再叠 `PANEL_FIT_EXTRA_PX`，popover 初始高度 560 降级为「从未收到上报时」的异常兜底。历史底部遮挡的根因是测量容器被 `h-screen` 固定高 + flex-shrink 压缩——容器改为自然高布局
- **主题底衬由原生层铺**：popover 的箭头 / 边框 / 半透明底衬在 webview 之外，前端解析出 light/dark 后经 `panel_set_appearance` 告知原生侧，由 `frameView` 下插的实色层与 `NSAppearance` 同步，浅色主题不再露出系统灰底；内容圆角与卡片对齐 14px。`cargo test popover::` 5 用例含底衬色与 CSS token 一致性断言

### 检查更新：独立 WebView 窗口 + 四入口统一

- **点「检查更新」立即弹窗**：固定 800×600（后调为 600 高）不可调整，先显示「正在检查更新…」，后台完成后刷新结果；更新说明用自研 Markdown 组件渲染（React 元素组合，无 `dangerouslySetInnerHTML`，仅放行 http(s) 链接）。托盘菜单 / 设置页「关于」卡片 / 启动静默检查 / 面板更新卡片四入口共用同一套 `check_and_prompt`
- **修「弹窗关不掉」**：`update` 窗口缺 `core:window:allow-hide` 权限，前端 `getCurrentWindow().hide()` 被静默拒绝（`.catch` 吞错），点 X / 「稍后」都无反应——新建 `capabilities/update.json` 补权限；同时「已是最新」态紧凑化，800 宽下真实长 changelog 从滚动变完整展示
- **`latest.json` 的 `notes` 改写为本版 CHANGELOG**：tauri-action 生成的更新清单 notes 写死为静态下载表，release-notes job 在发布前改写成同一份 CHANGELOG 本版段——「发现新版本」弹窗看到的是本版变更而非下载表；只动 notes，`platforms` 下载 URL 与 `.sig` 逐字保留

### 设置页「体检报告」入口（[方案](./docs/w1-report-sample-plan.md)）

- 探测本机 prune CLI（PATH + brew / cargo 固定候选，要求 ≥ v0.13），spawn 五条只读 `--json` 命令（轻量命令并行、doctor 独占跑避免与 scan 互相挤爆磁盘超时），按样例脚本同一九段口径组装 MD（脚注溯源 / 0≠读不到 / 脱敏）写入 `app_data_dir/reports/`；未装 CLI 时卡片给出 brew 与仓库脚本两条替代路径
- 同口径的 `pnpm report:sample` 流水线：采集 → 组装（九段 + R1-R5 硬断言）→ 渲染 MD/HTML/PDF（Chrome headless，零新依赖）
- `cargo test report::` 4 用例：真实 fixture 全量组装、脚注双向一致、全降级保九段结构与「读不到+原因」、IP 掩码边界

### Homebrew 分发（[方案](./docs/d02-homebrew-plan.md)）

- **一个 tap 仓库两种形态**：cask `prune`（GUI + CLI 一体，CLI 由 `binary` stanza 从 `Contents/Resources/bin` 链接）与 formula `prune-cli`（仅 CLI，universal tar.gz）。brew 成为主安装路径（cask 下载不带 quarantine，ad-hoc 签名也能直接开），Releases 直下改为兜底；README Install 段重排
- **构建侧**：`prepare-cli-resource.mjs` 按 triple 编 CLI 并拷进 resources/bin，`tauri.macos.conf.json` 平台 overlay 串「编 CLI → 前端构建」，Windows / Linux 产物不变；release.yml 显式注入 `CLI_TARGET_TRIPLE`（不能只靠 `TAURI_ENV_TARGET_TRIPLE`，cross 编译缺位会把 arm64 塞进 x64 dmg），macOS 两腿 artifact 由 `cli-universal` job lipo 成 universal
- 发版后 `homebrew-tap` job 渲染并推送 tap；`brew-tap.mjs` 的 sha256 优先取 GitHub API digest、缺失才落盘实算

### CLI：Wave 2 主体（端口前置 / 项目账本 / `prune.toml` / `prune watch` / MCP 12）

- **`prune watch`（D-10）**：按需守护，四类频道 cpu / mem / port / disk（`--only` 选择），`--json` 出 JSONL、`--once` 单轮、`--fail-on-warn` 出现 warn 即退出码 10（CI 用）。判据复用 `prune_core::island` 里 GUI 用的同一批监视器——两处各写一份阈值逻辑半年后必然对不上，这是本模块最重要的纪律。三条边界写进行为本身：不装后台（无 launchd / 开机项 / 守护重启）、不写日志（stdout 就是出口，重定向由调用方决定）、只读（不删除、不 kill、不改配置）。端口首轮只建基线，之后只报变化：多持有者冲突与「关注端口被新占用」两类告警
- **`prune.toml`（D-09）**：项目根配置，**只能收窄，不能扩大**。允许的键就五个：`ignore.projects` / `ignore.paths` / `thresholds.deps_warn_gb` / `thresholds.stale_days` / `watch.ports`——配置面本身走白名单纪律；试图扩大范围的键解析期直接拒绝，语法超出子集报「第 N 行不支持该语法」整份忽略，键越界单键回落 + note，损坏不静默。只读项目根那一份，子包不生效；Prune 永不写项目文件——模板由设置页复制到剪贴板，用户自己落盘。消费方：扫描白名单、依赖账本、端口关注层（配置的关注端口空闲也显示）
- **MCP 8→12 工具（D-11）**：新增 `project_deps` / `build_artifacts` / `net_sites` / `swap_report`。只读断言不变——执行类动词按名显式拒绝（报「MCP 通道只读」而不是 unknown tool）。工具 schema 冻结进 `mcp-tools.v1.json` 快照 + 逐字段对账测试：改工具面必须同批更新快照，测试挡在门上
- **`prune projects`（D-08）**：项目依赖账本按天趋势（默认 30 天，`--days` 可调），读 GUI / 扫描写入的同一张 `project_day` 表——账本是历史记录不是现场重算，CLI 不触发扫描。库不存在 / 无记录时输出原因说明，不伪造空账本

## 0.13.1

### 修 0.13.0 的 Windows 构建失败（功能与 0.13.0 相同，0.13.0 未对外分发）

- **`panel_fit_height` 在 Windows 上编不过（4 个 E0433 / E0425）**：贴高命令直接引用了 `crate::popover`（整个模块 `#[cfg(target_os = "macos")]`）与 `crate::tray::PANEL_H`（同样 mac 门控）。本地只在 macOS 编，CI 的 Windows 腿才暴露。改法：popover 分支整段用 `#[cfg(target_os = "macos")]` 块包起来——编译期就要消失，不能写成运行期的 `if false`；降级路径（Windows 常态 / popover 初始化失败）统一走 `panel.set_size`。`tray::PANEL_W` 去掉 cfg 门（跨平台都要用），`PANEL_H` 保持 mac 专属
- **`net_diag` 两处 unused import**：`Read` / `Instant` 只被 macOS 的 `networkQuality` 路径使用，按平台收窄，Windows / Linux 不再报 warning
- 0.13.0 的 Release 因 Windows 腿失败而未发布（三条 bundle 全绿才会同步正文并发布，留草稿等于 updater 收不到这一版）：本版携带 0.13.0 的全部内容，直接替换

## 0.13.0

### Wave 1：契约 v1 · prune doctor · prune mcp install · CLI 参考（[方案](./docs/wave1-contract-doctor-plan.md)）

- **D-03 契约 v1（补齐 + 冻结）**：8 个命令级 `--json` 输出点全部带 `schemaVersion: 1`。新增 `docs_json` 模块作为唯一产出层（CLI 打印与快照测试共用同一批函数），补齐 `scan` / `mem` / `port`（默认分支）/ `app` / `lan` 五处裸直出；顶层平铺不引入 `{data}` 包装（已发布的 3 个输出点都是平铺，包装等于现在就破坏一次）。`lan` 缓存与 CLI 输出分离：缓存继续写裸 `LanSnapshot`（GUI/MCP 读），只有 CLI 打印包信封。新增冻结快照 `crates/prune-cli/cli-contract.v1.json`——由生产函数导出（`PRUNE_DUMP_CONTRACT=1 cargo test` 重写），测试逐字段比对；改契约从此是一个显式动作
- **退出码收敛（W1-DP-2）**：新增 `ExitCode` 枚举（0/2/3/4/10），usage 的退出码行由它派生而非手抄。给「3 权限不足」真实的产生条件：`port --json` 主输出为空且 `coverage.note` 根因是权限 → 3；采集失败 → 4；差额说明（部分不可见）仍是 0——数据事实不是进程失败。此前 usage 承诺了 3 但全仓没有任何 `return 3`
- **D-04 `prune doctor`**：四域体检，给出「结论 + 证据 + 下一步」而不是再抛一遍原始读数。磁盘实时扫（~15s，独立线程 + `--timeout`）、内存/端口/网络轻量探测，总预算 30s；判据全部复用既有口径（磁盘水位 15%/5% 与菜单栏 island 同判据、内存压力级别、端口冲突 `owner_count > 1`、网络 `NetDiag.status/verdict/captive`）。硬约束：任一域读不到 → `severity: "unknown"` + 原因，绝不显示 0；`--only disk,mem,port,net` 裁剪、`--timeout` 封顶 30s；全部选中域不可读时按根因给 3 / 4
- **D-05 `prune mcp install`**：把 MCP server 写进 Claude Desktop / Cursor / VS Code 的用户级配置。默认 dry-run（`--write` 才落盘）；VS Code 用顶层键 `servers` + `"type":"stdio"`（抄 `mcpServers` 的块是「合法 JSON、不报错、不加载」）；`command` 写 `current_exe()` 绝对路径（裸名是「装成功但加载失败」的头号原因）；七步算法：坏 JSON 一律拒绝（`--force` 也不覆盖）、父目录不存在视为「未检测到客户端」、幂等 noop、写前备份 `.prune-bak-<时间戳>`（0600）、原子写、写后读回校验（顶层其他键逐一相等）、失败回滚。路径由代码常量给出，不接受 `--path`
- **D-06 CLI 参考 + 门禁 D/E**：新建 `docs/cli-reference.md`（中文单语，与 CLI 文案约定一致）——命令总表、每命令节（语法/flags/JSON 顶层键/退出码/示例）、退出码总表、JSON 契约规则、MCP 工具指向冻结快照。`check-docs-honesty.mjs` 新增检查 D（cli-reference ↔ usage 双向对账：子命令双向 + flag ⊆ usage）与检查 E（退出码表 ↔ `ExitCode` 枚举 ↔ ALL 表双向相等），负向验证均能拦下漂移
- **README 开发者入口重排**：安装 → 从源码构建 → 最初两分钟 → CLI → MCP 挪进前 1/3（此前开发者要滑过 200 行产品文案才能看到怎么装怎么接）；CLI 段补 doctor 与 mcp install 入口 + cli-reference 链接；中英双版同步
- 版本 0.11.0 → 0.13.0（0.12.0 留给 D-01/02 签名与 brew）

## 0.11.0

### F15 站点连通性与传输速率（[方案](./docs/net-sites-plan.md)）

- **网络 tab 第三个二级页「站点」**：用户自己的站表（默认 10 个国外常用站：Google / YouTube / GitHub / Wikipedia / X / Cloudflare / Apple / Microsoft / Amazon / OpenAI），逐站测 **DNS → TCP → TLS → 首字节 → 传输** 五阶段。诊断页回答「网络哪一层慢」，这一页回答「这一站断在哪一步」——同一时刻实测就有五种形态（TCP 不通 / TLS 卡死 / 握手过但首字节不到 / 403 但站点活着 / 正常），一个红绿灯说不出来
- **卡片网格 + 详情弹窗**：一站一张卡（状态点 / 名称 / 三段读数 / 速率条），点卡片开弹窗看五段细节。行式布局在 960 窗口下把 10 行 × 5 个数挤成一张读不完的表；卡片把「十站总量」和「单站细节」分成两个层级
- **卡片只报状态，不报细节**：有读数的站显示读数，没读数的站只显示一句状态（`不可达：TCP` / `无响应` / `出错` / `403 · 站点拒绝`）。断点秒数、HTTP 码、curl 原文全部收进弹窗——实测把 `探测本身出错：write-out has 7 fields…` 塞进 238 px 的卡里会溢出到隔壁卡片上，十张并排时反而看不出「哪几站有问题」（鼠标悬停仍是那句短结论）
- **弹窗里的耗时瀑布**：按累计时刻把整轮耗时切成 DNS / TCP / TLS / 首字节 / 传输，按语义分三类着色（握手 / 等首字节 / 收数据），断点那一段提亮。**没等到读数的那一截单独画成「无响应等待」**——只画已知段会让 4 s 超时的站看起来只花了 16 ms。表格同时给**本段与累计**两列，因为阈值口径不同（TCP 判累计 300 ms，TLS 与首字节判本段 400 ms / 1 s），只给一列必然要额外解释
- **六态判定而不是「通/不通」**：`ok` / `slow` / `stalled`（握手完成、首字节不到）/ `refused`（4xx·5xx，链路是通的）/ `unreachable`（DNS·TCP·TLS 没打通）/ `error`（探测机制本身失败）。措辞只写观测事实，不写「被墙」——原因不可知，不猜
- **速率口径 = `bytes / (total − ttfb)`**，不用 curl 的 `%{speed_download}`（那是 `bytes / total`，把 DNS + 握手 + TTFB 都摊进分母：实测 apple 256 KB 首页报 763 KB/s，传输阶段实际 1.22 MB/s）。样本 < 200 KB 或传输 < 300 ms 时**不给数字**，只报「样本不足（体积 / 时长）」
- **超时退出 ≠ 无读数**：`--max-time` 截断时 curl 退出码 28 但仍输出已收字节（实测 206 646 B），有字节就判「慢」，只有 0 字节才判「不通」
- **零新依赖**：系统 `curl` + `-w` 取五个时刻（沿用 `networkQuality` / `route` / `netsh` 纪律）。`-q` 置首位（不让 `.curlrc` 改口径）、`--noproxy '*'`（不读环境代理，两台机器同一份配置量出同一结果）、不加 `-L`（3xx 本身是读数）、不加 `--compressed`（字节数才可比）
- **站表可编辑**：存 `settings.json` 的 `netSites`，保存时校验（只 http/https、拒 `user:pass@`、≤ 20 条、≤ 32 字符名、同址去重）；损坏或校验不过一律回落默认并**显式提示**，不静默覆盖用户文件
- **并发 4 + 单轮互斥 + 可取消**：10 站典型 3–8 s、流量 ≤ 约 20 MB；连点不会叠成多轮；取消只拦「还没开始的站」，在飞的那个最多再等 8 s（不杀子进程，收益不抵复杂度）。每完成一站发 `net-sites://progress`，卡片逐张点亮（开着的弹窗也跟着更新）
- **单站重测在弹窗里**（卡片保持「打开详情」单一语义），只改那一站并同步回填最近一轮的缓存——「复制结果」与屏幕上看到的一致
- **复制结果为 Markdown**：时间 / 站表逐行读数（含断点、HTTP 码、出口 IP、样本量）/ 口径脚注，Rust 侧原生写剪贴板（同诊断报告的通道）
- **curl 缺失就把整页换成说明**（capability 门控），不渲染带 0 的空表；浏览器预览给了一份「五种形态都出现」的 fixture
- **修一个把「不可达」误报成「解析失败」的解析 bug**：`%{remote_ip}` 在 TCP/TLS 没通时是空串，`-w` 行尾就是 `...\t000\t\n`，`trim_end()` 会把空字段连同分隔符一起削掉（8 段变 7 段）→ 整站被判 `error`。真机上 google / youtube / github / wikipedia / x 五站全中。改为不裁剪行尾、只按 `\t` 切分，并补了一条用真实输出做断言的回归测试
- 测试 475 → 499（core 404 → 428：+21 单测覆盖判定表全分支、阈值边界、解析容错（含空尾字段）与参数纪律，+3 报告渲染；另有 1 个 `#[ignore]` 真机冒烟直跑 curl）

## 0.10.0

### M2 激活闭环（2027 H1 Roadmap §4；北极星分子「首启→首清 24h 率」的基础设施）

- **`scan_log(ts, level)` 落库**：每轮扫描完成落一行（full/incr，append-only，不过期清理——`MIN(ts)` 就是「首扫时刻」，清了旧行分母就没了）；`meta.first_boot` 建库即写、只写一次（激活率分母候选①，不用文件 btime——WAL/备份都可能动文件，自己的 meta 表才是自己的口径）
- **`activation_report` 命令**：首启 / 首扫 / 首清时间戳 + 清理次数（`COUNT(DISTINCT ts)`，同一次清理各分组共享同一 ts）+ 累计字节。**零遥测**：数字只随「复制 6 个数字」出设备；「率」在设备外由时间戳算出，分母禁用下载数（roadmap §5.2 口径纪律）
- **激活链路（DP-3 裁 B，拒绝强制线性 onboarding）**：首扫完成页引导卡（只报 L1 零后悔体量）→ 一键🟢（走既有 dry-run → 白名单 → 废纸篓链路，只碰 L1）→ 成绩单弹窗即时呈现（本次 N 项 + 累计）；卡片在首次清理落定或用户关闭后退场，不在「惊讶」前设卡
- **征募点（RK-5 红线内）**：只在首扫结果页出现，只提供「复制 6 个数字」（磁盘用量比 / 可回收 / 累计清理 / 清理次数 / 首扫距今天数 / 24h 内首清 0|1），供 M-B 留存访谈（v0.10.0 发布后第 2–4 周）使用
- **D2 口径标注**：`run_startup_check` 文档注释标注为 D2 口径文件——调用时机即指标定义，任何改动必须同步 roadmap §5.1 并重设基线，不接受静默变更（RK-8）
- **R8b 归档字节熔断（顺带）**：归档与 reclaim 是同一产品两条删除路径，爆炸半径必须同一口径——单次体量超 20 GB 需**第二次**显式确认；`breaker_error` 纯函数 + 写文件前检查 + 无确认通道的 `create()` 超限一律拒绝；UI 超阈值弹警示框 + 勾选二次确认（负面测试：21 GiB 未确认拒 / 确认放行 / 19 GB 直通 / 恰 20 GB 不算超）

### 同批搭载

- **端口服务类型识别（前后端）**：`ServiceKind` 分类 + L0 静态推断（端口表 / 进程名 / argv，零网络副作用）+ L1 主动探针（Web HEAD / MCP initialize / DB·SSH banner，`(pid,port,start_time)` 缓存防重探）；端口页 Badge「推断 / 确认」双强度与 exposure 并列；确认细节（MCP `serverInfo` 名字 + 版本 / HTTP `Server` 头）摊开在行下展示
- **端口服务自动探测（设置项，默认关）**：开启后每轮端口扫描对关注层 TCP 端口自动握手，把「推断」升级为「确认」——它发包，与行内「探测」按钮同一纪律：用户显式授权才跑；只连 127.0.0.1，结果按进程缓存（对 MCP server 不重复建会话），系统进程与失败过的行不探

### README 双语重构（M3 分发入口前段 + GEO 批次二；纯文档，无功能变更）

- **`README.md` 改为英文默认版，新增 `README.zh-CN.md` 中文版**，两版顶部互挂语言切换；结构对齐（定位否定句 → 带时点的 n=1 实测表 → 谁适合谁不适合 → 六 tab → 护栏 → CLI/MCP → 安装 → 状态），事实两边逐条一致。此前仓库只有中文版，这是 GEO 交付里「零英文版 = 信源孤证」那条缺口的落地
- **定位陈述按九条事实基线重写**：显式否定「内存监控 / 系统清理」，避免 README 自称与产品实际能力漂移；Windows 一律带 `beta`；内存口径补「Windows 是近似而非等价」
- **改写残留清理（GEO S 阶段清单）**：「零误删」→「固定白名单 + 废纸篓可恢复」；「iStat / Stats 已足够好」→「Prune 不读任何传感器」的范围直陈；实测数字全部补 `n = 1` 时点语境（防被 AI 普适化引用）
- **修 F9 白名单计数漂移（68 ↔ 84）**：`mcp.rs` 的 `disk_scan` 描述不再硬编码条目数（改「白名单条目」），README 双版统一声明 **68 条**（53 一键 / 12 逐项确认 / 3 只给命令，可对回 `whitelist.rs` 的 `ENTRIES`）——此前 84 与代码矛盾，任何引用条目数的文案都会同时给出两个出处
- **门禁扩容**：`check-docs-honesty` 的 MCP 对账改为**中英双版各查一遍**（缺文件即红，声明段 × 工具清单 × 代码三方一致），排他 sweep 5 份（新增 `README.zh-CN.md`）；负向实测两种漂移都会报红（改计数 9 个 / 改工具名）
- **版本徽章改动态**：`status-v0.10.0` 硬编码淡出，改用 shields 的 `github/v/release` 与 `github/downloads`——README 版本号不再是需要人工跟随的第五个漂移源；正文只声明「源码版本 0.10.0」并指向 Releases 页
- **多端支持章节（新增）**：README 双版各加「平台支持」矩阵（macOS 参考平台 / Windows 10+ beta / Linux 源码可用）+「Windows 上具体差在哪」20 行差异表——白名单 52 ↔ 28 条、Private Bytes 不含换出量故受害者榜隐藏、按应用网速与磁盘三层不可用、事件流未实现、托盘改彩色帧、端口反查反而不漏但无「项目目录」列；正文中 4 处 macOS 专属表述就地标注（内存换出量、按应用 Top 8、灵动岛、Linux 无 GUI 构建），口径全部取自 `windows-support-plan.md` §5–§7.2 与代码分支（`footprint.rs` / `net_sampler.rs` / `appdisk.rs` / `workspace.rs` / `island_stub.rs` / `tray_icon.rs`）


## 0.9.4

### M1 诚信归真（2027 H1 Roadmap §4；发布门禁项，纯门禁修复、无新功能）

- **排他表述清理（DP-4 裁 A：全删，改可核验事实陈述）**：README「AI Agent 友好」段的排他句式（原文宣称该机制为本产品独有）删掉，改为四条代码层可验证约束的直陈——phys_footprint 口径已被 RAMWatcher 复制的教训：排他句式不可自证；`prune-feature-spec-v2.md` 的「该位置无人占用」判断改为带时点与出处的表述（2026-10 竞品扫描口径，随竞品动态失效）
- **新增文档诚信门禁 `scripts/check-docs-honesty.mjs`**（`pnpm check:docs`，已挂进 `pnpm build` 链与 CI verify）：三处对账——`run_tool` 分发臂 ↔ `tool_defs()` 声明 ↔ README「MCP 工具（N 个」清单，任一漂移即红；排他表述 sweep 四份公开文档（README / CHANGELOG / 两份 spec）；**M29 只读底线断言**——执行类动词的显式拒绝臂必须存在且必须 `Err(`，改成可执行通道即红（负向实测：拒绝臂改 `Ok(Value::Null)` 后两处报红，恢复后转绿）
- **双源发布归真（R15）**：Gitee Release 正文此前是 CHANGELOG 固定截 2000 字符（长版本段会拦腰截断）——改为取到下一个二级标题为止 + 拼接与 GitHub 同构的下载表（含 ad-hoc 首开提示）；新增 CI `release-notes` job，在三个 bundle 全部完成后单点 PATCH GitHub Release 正文（下载表 + CHANGELOG 本版段），消除矩阵三腿并发覆盖；GitHub 推送失败从一句「跳过」改为打印双源错位警示与补推命令
- **可重复发布验收记录**：v0.9.3 tag 已由 CI 一次产出三端全量产物（dmg×2 / exe / msi / app.tar.gz×2 + latest.json，见 Release 资产）；「连续 2 次干净环境一键出三端产物」以 v0.9.4 tag 起第二次发布后达成并回填本条
- README 状态徽章 `v0.8.4` → `v0.9.4`（M26 后又漂移了一个 Q，本次起由 bump 脚本口径覆盖，漂移检测见 check-docs-honesty 后续扩展位）

## 0.9.3

> v0.9.1 / v0.9.2 的 tag 均因 CI 门禁红灯、未产出任何安装包，内容合并到 0.9.3 一并发布（版本号不复用 tag）。两轮红灯全是同一类盲区：**本地 macOS 编译不到的 Linux 分支**。

### CI 门禁修复

- **Linux 上的 unused import（v0.9.1 红灯）**：`mem/system.rs` 的 `PressureMetric` 只在 macOS（内核压力等级）与 Windows（占用率分档）分支里构造，Linux 的 `read_pressure()` 直接返回 `None`，类型名根本不出现。原导入没有按平台 gate，`clippy -- -D warnings` 在 ubuntu runner 上直接报错。已改为 `#[cfg(any(target_os = "macos", windows))] use super::PressureMetric;`
- **跨平台测试写成了本机断言（v0.9.2 红灯）**：`mcp::tests::disk_scan_returns_structure_and_respects_keys` 用 `cursor-cache`（`Os::MacOnly`）做 key 过滤，却断言结果恰有 1 条；`resolve()` 在 Linux 上按 os 过滤后返回空数组，断言必然失败。注释本身写着「只断言结构，不断言机器状态」，实现与意图相反。现主用例改用 `Os::All` 的 `npm`，并新增 `disk_scan_filters_platform_specific_keys` 把「平台专属条目在异平台必须静默过滤成空」这条边界显式钉住
- **CI 一次性报全失败**：测试步骤加 `--no-fail-fast`。默认「一个测试二进制失败就中止」，而 Linux 专属失败本地根本复现不了（v0.9.2 那轮 prune-cli 的 bin 测试先挂，prune-core 的测试压根没跑到），逐轮修代价过高。退出码仍为非 0，门禁强度不变

### M30 修复：归档不再卡死（诊断 → [diag-m30-archive-freeze.md](./docs/diag-m30-archive-freeze.md)）

- **根因**：6 条 `archive_*` 命令是同步 Tauri command，跑在 macOS 主线程上——900 MB 依赖的 50–90 秒压缩把事件循环整个占死，界面定格 + 彩虹球。连「处理中…」都画不出来：那次 React 重绘同样要主线程。全仓只有这 6 条同步命令做重活，`reclaim_execute` / `app_disk` 早就是 `async + spawn_blocking`，M30 是唯一破例
- **命令全面异步化**：`archive_plan / create / restore / restore_preview / delete` 改 `async fn + spawn_blocking`，主线程占用从分钟级降到微秒级
- **进度可见**：新增 `archive://progress` 事件（阶段 + 已处理/总字节 + 文件数），按 200 ms 或阶段切换节流——十几万条目逐条发事件会把渲染线程打满，那是把「卡死」换成「卡顿」；前端进度条带阶段文案与剩余时间估算
- **可取消**：`archive_cancel` 置位原子令牌，打包 / 校验 / 解压每条目间检查。取消后原件不动、不留半成品、不记账；任务进行中对话框把「取消」换成「取消任务」，不再允许「关掉对话框假装取消」
- **中断安全**：归档改 `.part` 原子发布（写 `.part` → 校验 → rename），强杀 / 断电最多留 `.part`；`archive_list` 顺手清理超过 10 分钟的陈旧半成品。0.9.0 直写最终路径会留下「UI 看不见、删不掉、纯占空间」的孤儿归档
- **压缩参数**：level 6 → 1（`Compression::fast`）。真机 817 MB / 30,984 文件：归档整体 **11.1 s**、归档体积 291 MB（0.9.0 同口径 ≥ 50 s）
- **串行闸门**：同一时刻只允许一个归档任务，第二个请求立即返回而非排队（排队会让用户以为点了没反应）
- **顺带修掉一个真缺陷**：tar 0.4.46 的 `append_link` 不会替调用方设 `entry_type`，符号链接会落成 size=0 的普通文件——pnpm 的 `node_modules` 恢复出来是坏的。已按 `EntryType::Symlink` 显式标记，并用「链接目标逐字还原」测试锁住
- **不做**：zlib-rs 后端。实测打包反而慢 9%（12.1 s vs 11.1 s）、归档大 7.5%（313 vs 291 MB），只有解压快（6.4 s vs 9.5 s）——主路径是打包，不换
- 测试 441 → 447：进度阶段顺序、取消无残留、陈旧 `.part` 清理、pnpm 链接往返、恢复取消、事件字段契约

## 0.9.0

### M30 项目休眠归档（Pro 首个交付，F14）

- **新增「休眠归档」通道**（位于项目体检卡）：僵尸 / 偏胖 / 久未重装项目的 `node_modules` 打包为 tar.gz 存入受管归档区（`<config>/com.xuyi.prune/archive`），释放磁盘同时保留完整还原能力
- **安全顺序写死在核心层**：打包 → 重读归档校验（条目数 + 非压缩字节数必须与扫描一致）→ **校验通过才删原件**（走废纸篓）→ 写账本；任何一步失败都不丢原件，删除失败回滚账本与归档文件，不产生孤儿归档
- **语义边界**：只归档依赖目录——目录名必须是 `node_modules`、绝对路径、非 symlink；用户代码与配置永不进归档（与「备份工具」的分界线：归档损坏最多丢一次重装）
- **恢复**：先 dry-run 预览（目标 / 文件数 / 体积 / 冲突标记）→ 目标已存在必须二次确认（覆盖也走废纸篓）→ 解压 → 校验；校验失败把解压产物移入废纸篓
- **归档区可回收**：删除归档 = 归档文件移入废纸篓 + 移除账本条目，归档区不是只进不出
- 契约：6 条 `archive_*` IPC 命令，执行类一律要求 `confirmed`；核心 9 项单测（往返恢复 / 冲突拒绝与覆盖 / 账本篡改被校验发现 / 撞名拒绝 / 账本损坏不崩）
- 本期不做（方案 §4）：归档整个项目、自动休眠、归档加密/增量、CLI 子命令——语义与定位红线见 [f14](./docs/f14-m30-archive-plan.md)

> 编号说明：0.5.0 的托盘动画两条曾标 M21/M22，与后来的 M21 成绩单 / M22 侦探档案撞号，现改标 M11-b / M11-c（M11 托盘主题的子项）。局域网线（docs/m18–m20-*.md）在 README 记为 L1–L3，理由见 README 路线图脚注。

## 0.8.5

### M27 二期：AI 缓存白名单补 OpenAI 家族 / HuggingFace / Windows 侧（F11）

- 一期（Cursor 6 / Claude 3 / Ollama 2 + Xcode 补扫 2）已于 0.8.3 发布；本期按 DevCleaner 1.16 覆盖面补齐残余，**14 条全部本机实测后登记**（方案与实测数据见 [f11](./docs/f11-ai-cache-2-plan.md)）：
  - **OpenAI 家族 4 条 🟢**：ChatGPT 桌面 WebKit 缓存（128 MB）/ Codex 桌面 Sparkle+HTTP 缓存 / Codex 旧 bundle 缓存 / `Application Support/Codex/Cache`（23 MB，只登记 Cache 子目录——本体含 Cookies / Local Storage，红线）
  - **Codex CLI 🟢**：`~/.codex/cache`（应用目录 / 工具目录 / 插件目录缓存，两端通用）；`sessions` / `history.jsonl` / `config.toml` / `auth.json` 永不登记
  - **权重类 🟡 L3**：HuggingFace hub（176 MB 实测）+ LM Studio（官方默认目录，未装时自动跳过）——可重下但代价高，逐项确认，与 Ollama 模型同口径
  - **Cursor CLI 编译缓存 🟢**（`Library/Caches/cursor-compile-cache`，24 MB 实测）
  - **Windows AI 6 条 🟢**：Cursor（Cache / CachedData / GPUCache / logs）+ Claude（Cache / GPUCache），按 Electron 布局落 `%APPDATA%`，真机待验（路径偏差只会扫不出，无误删风险）
- **Atlas（ChatGPT 浏览器）整目录永不登记**：459 MB 里 458 MB 是 `browser-data/`（Chromium profile，含 login-staging 站点登录态），与 HTTP 缓存混放——目录粒度做不到「只删缓存不登出网站」，宁可错过；红线测试固化（`com.openai.atlas` / `AS/Codex` 根 / `.codex` 非 cache 子路径三者断言禁止）
- 红线单测扩展：AI 系条目 rel 禁含 `sessions` / `history` / `auth` / `Cookies` / `browser-data` / `conversations` / `ai-tracking` / `Local Storage` 等 13 个用户状态标记
- 中英 catalog 同步 14 个 `disk.cache.<key>`（check-i18n 门禁覆盖）；零代码改动扩散，登记即自动进入扫描 / 报告 / dry-run / 废纸篓 / MCP 全链路

### M28：Xcode 深扫专项残余（F12，与 M27 同版本并行）

- 开工核验：四组里 DeviceSupport / Archives / 模拟器旧运行时（含 `CoreSimulator/Devices` 整目录，key `simulator` L3）均已落地，**本机 3.5 GB 最大头已有覆盖**；本期补真正的残余 5 条（方案与实测见 [f12](./docs/f12-xcode-deep-plan.md)）：
  - **DerivedData 🟢**（本机 68 MB）：构建产物 + 索引，重新编译全部可再生，对标 DevCleaner Safe 类，L2 一键清理
  - **XCTest 测试设备克隆 🟢**（`Library/Developer/XCTestDevices`）：跑 UI 测试后可达数 GB 的知名膨胀项
  - **设备磁盘映像 🟢**（`DeveloperDiskImages`，17 MB）：连真机时下载，按 iOS 版本可重下
  - **Simulator 临时文件 🟢** + **Xcode 组件下载缓存 🟢**：即时 0 B 但随使用增长
- 红线测试：CoreSimulator 禁止登记文件路径（`RuntimeMap.plist` 等）与单设备 UUID 目录；Xcode `UserData` 根禁止登记（KeyBindings / xcuserstate 用户状态）；`simulator` 保持 L3 重置语义不许降级；新条目全部 `Os::MacOnly`
- 明确不做：`Devices/<UUID>` 单设备粒度（需 simctl 状态采集，超出纯路径机制）、`/Library/Developer/CoreSimulator/Volumes`（系统级 cryptex，非用户域）

### M29：MCP 只读深化 + 「AI 只读、人执行」信任叙事（F13）

- **新增 MCP 工具 `disk_scan`**：白名单逐条细分视图（只读，秒级）——84 条登记项的实时体积 + 级别 + 可选一级子目录占比（复用自建线程池并发测体积，CoreSimulator 3.5 GB 量级秒级）；与 `disk_report` 分工：那边回答全盘浪费结构（~15 s），这边回答「某条缓存现在多大、什么占的」。真机实测：DerivedData 68 MB 细分到单个项目 47 MB
- **新增 MCP 工具 `net_diag`**：端口暴露面诊断结论——直接给「谁在对外监听」的结论而不是原始条目；`exposure=any/interface` 都按局域网可达标 attention（附进程与项目归属、收口建议），权限差额与 TIME_WAIT 残留单独提示；结论拆成纯函数 `diag_conclusions` 单测。真机实测：抓到 ControlCenter AirPlay 5000/7000 与 rapportd 的 0.0.0.0 暴露
- **只读底线代码化**：MCP 分发层对 `reclaim` / `clean` / `execute` / `delete` / `remove` / `kill` / `write` / `trash` 等执行类请求**显式拒绝**并说明「清理只能由人在 GUI 确认」（不是笼统 unknown tool）；新增测试固化——工具名禁含执行词根、执行类请求必须报只读错，回退即红
- README 新增「AI Agent 友好（信任叙事）」段：Agent 能看（8 只读工具）、人才能删（GUI 三层护栏）、分发层显式拒绝、白名单兜底——四条全部代码层可验证
- 明确不做：MCP 写通道、`reclaim --yes` 的 MCP 包装、GUI 远程触发（方案 [f13](./docs/f13-m29-mcp-trust-plan.md)）

## 0.8.4

### M26 修复与诚信（工程门禁 + 发版安全 + 文档校准）

- **clippy 33 处告警清零**（21 类）：自动修复 22 处（`is_multiple_of`、`div_ceil`、冗余闭包、自动解引用、可折叠 if/match 等），手工处理 11 处——`format!` 套 `format!`、两处 doc 列表缩进、`sort_by` → `sort_by_key(Reverse)`、`get(&2).is_none()` → `!contains_key`、两处 `type_complexity` 抽类型别名（`BuiltMenu` / `AnchorMetrics`）、测试构造器显式 `allow(too_many_arguments)`；顺带把 `island_geo` 两条恒真断言改成 inline const 块（编译期校验，而不是运行时假断言）
- **CI 门禁补齐 clippy**：verify job 在 fmt / 单测之外新增 `cargo clippy -p prune-core -p prune-cli --all-targets -- -D warnings`（Linux 可编译范围；src-tauri 需 macOS 依赖，由本地同命令覆盖）
- **发版脚本三处加固**（`scripts/release.mjs`）：① `git commit` 不再挂 `|| true`——hook 拒绝或冲突时立即中止，避免 tag 打在不含版本变更的 commit 上；② tag 已存在直接报错退出，不再 `git tag -f` 覆盖（覆盖会让 updater 收到「同版本号、不同产物」）；③ 令牌移出 query string，改走 `Authorization: Bearer`，仅在请求头被拒时降级并明确告警
- **文档归真**：README 徽章 `v0.5.0` → `v0.8.4`、平台标注改为 `macOS 13+ · Windows 10+ beta`（W0–W3 代码层完成、真机走查待做）；路线图补 L1–L3 局域网线与 M26，并加编号重号说明；CHANGELOG 0.5.0 两处托盘条目改标 M11-b/M11-c
- 门禁实测：`cargo test --workspace` 426 passed / 0 failed / 2 ignored（49 + 18 + 359），`cargo clippy --workspace --all-targets -- -D warnings` 零告警，`cargo fmt --all -- --check` 通过

## 0.8.3

### 修复：拓扑视图极端密度下 IP 被画布裁切

- 50 台设备（网关 + 本机 + 5 台连了我 + 38 台其余）时，布局兜底分支先用「10 台/列」反推列数、再按该列数硬排，**没有回头校验宽度**——最右列 IP 被 `overflow-hidden` 裁成 `192.168.1…`，最左列只剩 `168.1.54`
- 兜底改为宽度硬约束优先：列数取「标签右沿仍落在画布内」的最大值，行数不设上限，装不下就加高画布（页面可滚）。45 台实测：右侧 3 列（13/13/12）+ 左侧 2 列，画布 876 高，零裁切；25 台常规密度回归无变化（画布仍 528）
- 文档：M20-b 方案补 §12 修正记录；M20 spec / plan 的 reduced-motion 归因纠正（Tailwind `animate-ping` 不带降级，实际由 index.css 全局 `prefers-reduced-motion` 兜底）

## 0.8.2

### 拓扑视图视觉重做（M20-b）· 恢复入口

- **布局改为轴向分层**：原先网关周围摊同心环，25 台时既叠压又看不出归属。现在是一条脊柱 + 两侧分支——`连我的设备 —— 本机 —— 网关 —— 连网关的设备`：网关居中、本机固定在其左侧 150px、`inbound` 命中的设备排在本机外侧、其余设备在网关右侧按列堆叠，列内按设备类型聚簇（域色连成段）。没有网关时本机顶替中心位
- **边从星形改三条规则**：`网关—本机`、`网关—连网关的设备`、`本机—连我的设备`（accent 虚线）；两端各退一个节点半径，线止于圆边界而不是从圆心穿出
- **标签策略**：默认只显示一行 IP（opacity 0.85，不再渲染「未知」这类零信息量文字），挂在**背向脊柱**的一侧；hover 才展开设备名并提亮该节点、其余降到 0.25。13 字符 IP 实测 74.4px，列宽给到 80 才不触发省略号（74 会截成 `192.168.1.2…`）
- **文字光晕**：网格密度下任何直连边都会斜穿别人的标签，用画布底色 `text-shadow` 把线从文字处「抠掉」，配上边线压到 0.35——文字是主角，线只做暗示
- **节点域色同源**：复用设备卡的 `tileTone`（路由/本机 accent、电脑手机平板 info、NAS 青、IoT 紫），未知类型降为 20px 小灰点（无图标），节点缩到 38/46px
- **脉冲收敛**：只让 `firstSeen` 最新的那一台 ping，扩散范围收在 tile 内；其余新设备用静态 accent 描边 +「新」badge（原先十几台同时脉冲，满屏鬼影）
- **尺寸自适配**：每列台数 4→10、列宽 140→124 依次试，判据含「最外一列的标签右沿也要落在画布内」；25 台 @1182 宽排成 3 列（7/7/6），画布 1182×528 零越界零叠压；802 窄窗自动变 2 列 ×10 行、画布增高到 702
- **拖拽**：位置记忆不变，但窗口变窄 / 画布变矮时把旧坐标夹回边界内（原先拖过的节点会留在画布外）；切视图来回仍不丢位置
- 入口恢复：`TOPO_ENABLED` 开关移除，顶部「卡片 / 拓扑」二态照常出现

## 0.8.1

### 局域网拓扑视图（L3）

- **「卡片 / 拓扑」二态切换**：局域网分段筛选条右侧新增视图切换，拓扑视图把扫描到的设备画成网关居中的星形拓扑图（新增 `LanTopology` 组件，零依赖自绘——SVG 边层 + HTML 节点层，复用现有 design tokens 与设备图标）
- **确定性星形布局**：网关居中、本机内圈（网关正下方 84px）、其余设备单环分布并按 IP 哈希抖动 ±10°——跨渲染不跳位；环半径随画布自适应
- **节点交互**：可拖拽（位置记忆、不回弹，拖过的不被重扫重排）、悬停高亮相连边、点击打开设备详情弹窗（与卡片视图共享，含 M19 加入/活跃时间）
- **信号可视化**：新设备 ping 脉冲环 +「新」badge；「连了我」设备的边为本机 accent 虚线（安全信号前置）；离线设备置灰 45% + 虚线边仍参与布局
- **边界兜底**：无网关时本机顶替中心位；画布点阵纹理 + 拖拽提示 + 条件渲染图例（对应元素不存在则不显示，不让图例撒谎）
- **走查修复（T5）**：环半径给本机标签留净空、中心位按半径反推（原先正下方节点压住本机标签、顶部「新」badge 被裁）；视图切换改常驻 + `hidden`（原先切回拓扑会丢拖拽位置）；拖拽夹紧改 58/44（原先小于节点半宽，边缘拖拽会裁标签）；画布与边线 token 化（浅色主题下不再是一块黑画布）
- build vs buy 裁决归档（spec §10）：不引 react-flow（+59KB gz / zustand 双拷贝 / 自带能力属明确不做，且不含布局算法）；重评估触发条件为节点上百或需要缩放平移

## 0.8.0

### 局域网设备时间线（L2：加入时间 / 最后活跃 / 新设备标识）

- **跨轮次设备记忆**：新增 `lan-history.json` 持久层（身份 + 时间 + 定位，隐私最小面不出本机）。每轮扫描后回填 `firstSeen` / `lastSeen` / `isNew` 三字段——Rust 侧算好，前端零推导；`#[serde(default)]` 兜旧版缓存，升级不丢快照
- **设备卡**：标题行追加「新」badge（accent 域色，与「自己 / 上联」同 token）；新增时间行只显示「活跃 x 前」——加入时间信息密度低且不变，只进详情弹窗（已确认决策，不做设置项）
- **详情弹窗**：新增「加入时间」「最后活跃」两行，相对时长 + `MM-DD HH:mm` 绝对时间双行展示；旧版缓存显示「—（旧版快照无记录）」
- **身份 key 升级（口径变更）**：跨轮次「同一台设备」从 M18 的「以 IP 为准」升级为**稳定 MAC 优先**（非随机化）；随机化 / 无 MAC 回退 `ip + 网段`——DHCP 换 IP 不再天天产生假「新设备」，这是新设备标识可信度的根基。UI 文案（footer / 随机 MAC 说明）同步修订
- **新设备阈值**：10 分钟（const）；`isNew` 以快照时刻为基准计算，重启后看缓存快照不失真；**首扫基线**规则——冷启动首轮全员不标新，notes 一句话说明已建立基线
- **历史回收**：30 天不活跃剔除 + 总量上限 1024（超出按 last_seen 最旧淘汰）
- **三端口径一致**：GUI 扫描 / CLI `prune lan`（输出 `[新]` 标记 + 加入/活跃时长）/ MCP `lan_devices`（缓存透传，字段自动带齐）共用同一份历史层；与 M23 `LanWatchMonitor`（会话内新面孔告警）定位不同不合并
- 相对时间锚定快照时刻而非墙钟（`formatAgo` / `formatDayClock`）：页面停留与重启后打开都不漂移

## 0.7.0

### 中英双语（i18n 全链路）

- **两套文案、一处来源**：前端 14 个命名空间（settings / net / disk / memory / resources / ports / lan / stats / island / panel / chart / nav / update / common）约 2000 条 key，中文为准、英文并列；Rust 侧同样一套表，但用**结构体常量**而不是字符串 key——Rust 没有 TS 的模板字面量类型，key 拼错只能运行时发现，结构体缺字段是编译错误
- **解析链**：settings.json 的 `locale`（auto / zh / en）→ macOS `defaults read -g AppleLanguages` 或 Windows `GetUserDefaultUILanguage` → 兜底 En。切换不重启：写盘 → 重建托盘菜单 → 广播 `locale://changed`，主窗口 / 面板 / 灵动岛同帧换语言
- **繁体落英文**（行为变更）：`from_tag` 收窄到 `hant` / `-tw` / `-hk` / `-mo`，且 `hans` 优先（`zh-Hans-HK` 仍判简体）。catalog 只有简体，给繁体用户简体不如给英文诚实；影响面不止设置页——托盘 / 灵动岛 / 报告走同一套判定，Rust `is_traditional` 与 TS `isTraditionalTag` 逐字一致、10 条用例两边共用
- **设置页语言下拉**：三颗胶囊改下拉，「跟随系统」带副标题说明系统语言被认成了什么（正常 / 「ja-JP · 暂用 English」/ 读取失败三种）。`LocalePayload` 扩到 `{current, source, system, autoResolved}`，三个通道一起改——只改一半会出现「主窗口对、设置页错」。顺手删掉缓存系统语言的 `OnceLock`：它让「跟随系统」在进程内永远读第一次的值，而代码注释早就承诺「重复点一次应当刷新」
- **入口不止一处**：⌘, 打开设置；菜单栏面板 header 齿轮经 `open_main(settings)` → `ui://settings` 直达。只开面板的用户原先到不了设置，语言切错了就再也走不回去
- **判定与文案分家**：诊断结论用 `VerdictKind` 8 变体断言「命中哪条规则」而不是中文整句，改文案不再挂测试；阈值（50 / 80 ms 等）从常量 format 进句子，避免「界面说 50 ms、代码按 80 ms 判」
- **Rust 侧文案**：`prune-core/src/i18n/{diag_text, report, err}` 三张表；报告布局只写一次（`render_with`），中英各一张静态片段表；`ReclaimError::text(locale)` 新增而 `Display` 保留中文给 CLI（`prune-cli` 冻结中文）
- **预览说明结构化**：`ReclaimPreview.notes` 从 `Vec<String>` 改为 `Vec<ReclaimNote>`（`scanPending` / `missingGroup` / `highRisk` / `scattered` / `rosettaPending` / `nothingToDo`），后端只下发 code + 参数，组名交给前端 `cacheLabel()` 渲染——后端 `label` 只有中文，拼好的句子在英文界面里必然露中文
- **门禁**：`scripts/check-i18n.mjs` 拦四类问题——源码硬编码中文、catalog 未被引用的 key、中英 key 不对齐、参数占位符不一致；`pnpm build` 把它串在前端构建前面，过不了就不出包
- **文案纪律**：emoji 留在代码里不进 catalog；语言名（中文 / English）恒为原文不翻译；产品名与专有名词不译

### 托盘双击直达主窗口 + 面板「去清理」主 CTA

- **双击 tray 图标 = 直接打开主窗口**：macOS 上 Tauri 不派发 `DoubleClick`（该变体仅 Windows），改为两次左键 Up 间隔 350 ms 内自行判定；第一击已弹出的面板先收起再开主窗口，三连击不会被算成第二次双击。Windows 的 `Click`×2 + `DoubleClick` 连发由「500 ms 窗口去重」合并；判定抽为纯函数 `within_window`（4 个单测，含时钟回拨）
- **面板底部「可回收 X GB → 去清理」主 CTA**：产品主张（可回收 GB = 首屏主数字）此前只是存储行的副读数——看得到数字、找不到动作。现升为 accent 强调按钮，直达存储 tab 清理流；文案受「不造假事实」红线约束：未深探显示「尚未深探 · 去扫描」、扫出 0 显示「暂无可回收 · 看明细」、扫描中显示「正在深探 · 看进度」，绝不把 null 渲染成 0 GB
- **高度预算零回归**：CTA 行沿用原「应用入口」的 30 px，面板 432 贴合高度不变；无泄漏条时内容零溢出（Playwright 实测 `scrollHeight - clientHeight = 0`），出现泄漏条时滚动（既有行为）；「应用 N」入口保留为同行弱链接
- 双击行为已列入 `docs/ns-popover-acceptance.md` 人工目测清单

### 应用历史深度 8 天 → 30 天（M25，对标 Vitals）

- **保留期 30 天**：`app_hourly` 小时聚合的保留期从 8 天扩到 30 天（原来为「7 整天 + 余量」设计）；采样、落库节拍（60 s flush）与增量语义不变
- **天粒度聚合下推 SQLite**：新增 `app_daily_get`——按天 GROUP BY 一条 SQL 完成（720 个小时桶 → ≤30 行），均值用 Σsum/Σn 重算（不是均值的均值）、峰值取 MAX、流量相加，与前端 `aggregateDays` 口径逐字段一致；3 个新单测（同天聚合 / 跨天分组与 since 过滤 / 家族隔离）
- **命令与窗口**：`app_history` 查询窗口 clamp 10 天 → 30 天；新增 `app_history_daily(family, days)`（clamp 30）
- **应用详情「历史走势」三档区间**：24 h / 7 天 / 30 天——30 天视图直接消费天桶（每根柱 = 一天），24 h/7 d 视图照旧前端聚合；覆盖率 30 天档按「有记录的天 / 30」计算；空态判断同时看小时与天数据
- **口径纪律不变**：历史仍是 RSS 口径（脚注写明，与 footprint 当前值不相减）；缺失小时/天画灰虚线不补 0；覆盖率 < 60% 角标转 warn
- **已知取舍**：fast 轮全量家族进账（本机实测 657 个家族 / 2 天 1.8 万行 / 库 9.4 MB），30 天外推 ≈ 27 万行 / ~140 MB SQLite——对清几十 GB 缓存的开发机可接受，如实记录不藏

### AI 应用缓存 + Xcode 补扫（白名单扩充，P0）

- **AI 应用缓存 11 条登记**（2026-11 竞品对齐，DevCleaner / CacheCleaner / Mole 均已支持，Prune 补齐）：
  - **Cursor** 六项 🟢：V8 代码缓存 / HTTP 缓存 / GPU 缓存 / 扩展安装包缓存 / 日志 / 系统缓存（本机实测新增 2.92 GB 可回收）
  - **Claude** 三项：桌面版 HTTP 缓存与系统缓存（macOS）+ Claude Code shell 快照（跨平台，官方可再生）
  - **Ollama** 两项：日志 🟢 + 模型权重 🟡（L3 逐项确认——可 `ollama pull` 重下但代价高）
- **Xcode 补扫 2 条**：Interface Builder 设备支持（🟢）+ Simulator 旧平台运行时（🟡，可重下但代价高）——DerivedData / DeviceSupport / Archives 等既有条目之外的缺口
- **用户状态红线（代码层约束）**：`state.vscdb`（AI 聊天记录，本机实测 18 GB）、`workspaceStorage`、`anysphere.cursor-agent-worker`、`.claude/projects` 永不登记——竞品指南同样强调「缓存 vs 工作区状态必须区分」；新增红线单测防回归
- 零代码改动扩散：白名单登记即自动进入扫描 / 报告 / dry-run / 废纸篓清理 / MCP 全链路；平台过滤照旧（AI 桌面端路径仅登记实测过的 macOS，Ollama 与 Claude Code 为 home 相对路径两端通用）

### 项目健康卡（M24，项目体检）

- **按项目的依赖账本**：把「55 个 node_modules 共 17 GB」拆开——每个项目一行：依赖体积、依赖上次安装（node_modules mtime）、项目最后活动（package.json / 锁文件 / `.git/index` 里最新那个）、锁文件并存情况；点击行在 Finder 里打开项目目录
- **结论只陈述事实**：僵尸依赖（依赖与项目**双双** ≥60 天没动）· 偏胖（≥1.5 GB）· 久未重装（项目还在动）· 健康；多锁文件（npm/yarn/pnpm/bun 两个以上）独立标记
- **零新增扫描**：复用扫描已产出的 node_modules 聚合表，每个项目仅 6 次 `stat`（4 个锁文件 + package.json + `.git/index`）；账本挂在 `DiskReport.projects`（旧报告反序列化为空）
- **不代删**：卡里不提供单项目清理入口，清理仍走「项目依赖」分组的逐项确认；不显示「预计可省」这类推断值——每个数字都能追溯到一次 stat
- 判定纯函数在 prune-core，9 个新单测（聚合 / 僵尸 / 偏胖 / 多锁 / 排序 / 相对路径 / mtime 缺失）

### 局域网新面孔提醒（M23，灵动岛第四类事件）

- **新设备提报**：邻居表里出现从未见过的在线设备 → 岛弹「新面孔 · 设备名 加入了局域网」，展开卡给 IP、设备类型词（手机 / NAS / 打印机…）与随机化 MAC 提示；点击直达端口页的局域网设备区
- **观察者定位**：60 s 一拍只读系统邻居表（macOS `arp -an` / Windows `GetIpNetTable2`），零主动探测、零 lsof——不是网络扫描器，M18 的边界原样继承
- **防刷屏四道闸**：基线不补报（启动时存量设备全是旧面孔）→ 终身制已知集（同 IP 只报一次，邻居表条目 expiry 来回晃不重报）→ 网段指纹变化重建基线（换 Wi-Fi 后「全是新面孔」不弹）→ 单轮 3 条上限余量合并 + 独立 4/h 频控
- **已知取舍**：设备身份键按 M18 spec §9 用 IP——DHCP 换址会误报一次、新设备拿旧 IP 会漏报一次（随机化 MAC 让 MAC 键不可靠，不猜）
- 设置面板灵动岛区新增「局域网新面孔」开关（默认开）；判定状态机在 prune-core 纯逻辑，8 个新单测

### 泄漏侦探档案：悬案卷宗（M22）

- **案件簿状态机**：泄漏嫌疑从逐轮快照串成有状态的案件——首次命中立案（`CASE-1111-03` 等宽编号，终身不变）、持续命中续期（增长峰值不回退）、嫌疑人离场（进程退出）后未复现自动归档、同应用另立新案判「疑似复发」；判定单元仍是单进程序列，多进程命中各立各的案
- **结案 ≠ 证明清白**：`Cleared` 只描述「嫌疑人离场且此后同应用未见新嫌疑」；措辞红线不变——永远不说「你的 XX 泄漏了」，证据数字（增长量 / 斜率 / R²）一字不减
- **卷宗卡重塑**：嫌疑人档案照（按 family 换真应用图标，无则字母徽标）+ 证物生长曲线（70 min footprint 序列，覆盖 GC 语言窗口）+ 侦查时长 + 档案室折叠区（结案陈词逐条展示）；点击行仍直达进程检视器，LeakEvidence 证据卡原样保留
- **采样误判自愈**：已结案卷宗的序列再次出现（同一 start_time 的进程不可能死而复生）自动重启侦查，不立新案
- 案件簿 `CaseBook` 在 prune-core（`leak/cases.rs`），9 个单测；内存态、Prune 重启即清零（档案描述「这一段试用期」）；零新权限、零新采样

### 剪除成绩单（M21，Wrapped 式年度浪费报告 MVP）

- **清理落库**：`reclaim_execute` 成功后把每个剪除项（分组 label + 字节）追加进本地 SQLite `cut_log`（append-only，留存 800 天）；失败项不计入——「剪掉了」以废纸篓里的东西为准；库打不开时静默跳过，不影响清理本身
- **总览页成绩单卡**：累计剪掉 X GB（CountUp）· N 次清理 · 自 YYYY-MM-DD 起 · 换算彩蛋（≥4 GB 换算高清电影 4 GB/部，否则换算照片 3 MB/张）· 受害者 Top 3（按累计字节降序，label + 字节 + 次数）
- **空态引导**：还没有剪除记录时显示「去存储完成第一次清理」，点击直达存储 tab
- 数据全部来自用户亲手执行的清理动作，零埋点、零网络；聚合查询 `cut_summary` / `cut_top` 在 prune-core（4 个单测）

### 机器宠物：小猫健康状态机（M20）

- **四态表情**：悠闲（默认，前跃按 CPU 快慢）/ 狂奔（CPU EMA ≥85% 持续 10 s，连续扑跳）/ 喘气（swap ≥80% 或内核压力 Warning+ 持续 30 s，趴下腹式呼吸）/ 炸毛（home 卷剩余 <10% 持续 60 s，弓背竖尾警戒）——不打开面板，瞄一眼猫就知道机器处境
- **全通道双滞回防抖**：每个状态进入 / 退出各配滞回线 + 连续计时（喘气要持续平静 15 s 才恢复，狂奔掉到 75% 以下才收，磁盘 >12% 持续 30 s 才解除）；中途一拍掉线计时清零
- **优先级裁决**：喘气 > 狂奔 > 炸毛 > 悠闲（内存危机最紧急）；高优先级解除后低优先级通道立即接管（喘气好了回到炸毛，因为磁盘还满着）
- **读不到不冒充**：swap / 压力 / 磁盘任一读不到，对应通道安静跳过，不会拿 0 误判「充裕」
- **tooltip 心情词**：`Prune · CPU 12% · 喘气`（只对猫皮生效，悠闲不标注）
- **新帧资产**：`scripts/gen-tray-cat-moods.mjs` 程序化生成喘气 4 帧 + 炸毛 4 帧（macOS 模板图 / Windows #34D399），与 gen-tray-cat.mjs 同族几何纪律（圆润轮廓、眼洞镂空、耳朵随头部定位）；狂奔复用前跃帧集零新资产
- 状态机 `PetMoodMachine` 在 prune-core（`pet.rs`），16 个单测；剪刀皮行为完全不变，「CPU 动画」开关仍一键关闭所有动画；不做进化形态（后续立项）

### 端口冲突保镖（M19，灵动岛第三类事件）

- **关注端口新占用提报**：`is_watched` 命中的端口（3000 / 5173 / 8080 等开发端口、项目 dev server、对外暴露面）从空闲变为被占，30 s 内从灵动岛弹出「:3000 被 node 占用 · prune 项目」；蓝 #0A84FF 强调色 + 插头图标，与 CPU/内存的橙/红区分
- **同端口多持有者冲突提报**：SO_REUSEPORT 双绑定 / 真冲突（`owner_count() > 1`）出现时提报「冲突」，优先于新占用事件；存量端口**突然**多出第二个持有者同样触发
- **项目归属直达**：展开卡显示 cwd 推断的项目名，点击直达端口 tab（其余事件仍开总览）；暴露面为 `any` 时加「对外可达」橙色提示（M7 安全语义同口径）
- **三道防刷屏闸**：① Prune 启动首轮只建基线，存量端口一条不弹；② 每端口 10 min 冷却，dev server kill→重启横跳不重复报；③ 端口事件独立 6/h 频控。端口转空闲即重新武装
- **常驻开销 0 起步**：30 s 一拍守望，灵动岛或保镖开关关闭时整拍跳过；端口 tab 活跃且快照 <20 s 时复用 PortState 快照（不起第二轮 lsof）；自扫时复用 PortState 的 ProjectCache（cwd 查询跨轮共享）。判定逻辑 `PortGuardMonitor` 在 prune-core，纯函数 + 10 个单测
- 设置：灵动岛区新增「端口冲突保镖」开关（`island.port_guard`，默认开）；`IslandSettings` 前后端同次变更同步加字段

## 0.6.0

### 局域网设备页卡片化重设计（L1 UI）

- **表格 → 卡片网格**：设备行改为自适应卡片（`minmax(330px, 1fr)`），三层分组——本机与网关置顶固定、在线设备网格、离线设备默认折叠（展开置灰展示，不占首屏）
- **设备类型 SVG 图标**：11 类设备（路由器/电脑/手机/平板/电视/音箱/打印机/NAS/摄像头/IoT/未知）线性图标 + 低饱和域色 tile（路由器系 accent / 电脑手机系 info / NAS cyan / IoT 紫），替换原单字符 `kindGlyph`
- **设备名前置**：mDNS hostname（去 `.local`）作为卡片主标题，查不到回退类型名，不编造；随机 MAC 设备显示「随机 MAC」并弱化
- **安全信号提权**：摘要行小字升级为指标卡 ×3（在线 N/M · 对我开放的端口 warn 色 · 扫描耗时）；「连了我」从设备列徽标升级为卡片级 warn 徽标 + warn 描边
- **筛选 chips**：全部 / 在线 / 连了我（带计数单选）；随机 MAC 设备数量汇总提示条
- **设备详情弹窗**：点击卡片弹出详情（状态、IP/MAC 复制、厂商、类型、主机名、全部服务、入站端口），Esc / 点遮罩 / 右键 / 关闭按钮四种方式退出；复制走 `writeClipboard` 三段兜底（Tauri 原生优先）
- 边界不变：进入分段不扫描（手动触发 + 首次 macOS 本地网络授权说明原样保留）；只改前端展示层，`lanStore` / Rust 侧零改动

## 0.5.0

### 网络诊断（M17）

- **分层探测**：DNS（getaddrinfo）/ 局域网网关（TCP）/ 出口层（1.1.1.1:443 直连，超时回退 223.5.5.5:443）三层测延迟，阈值透明（50/100/30/300ms）分层判定「慢在哪」；出口层探测失败记 note，不冒充成功
- **Captive portal 检测** + **手动测速**（系统自带 networkQuality，吞吐 + bufferbloat RPM）；Windows 无系统测速等价物，capability 门控整卡隐藏
- **诊断报告复制**：Rust 原生写剪贴板（macOS pbcopy / Windows Set-Clipboard），IPC 失败时前端红字反馈
- 诊断手动触发不常驻、不落库；不做切 DNS / Radar / 信道扫描（只读边界）

### 应用真实图标 + 应用详情页（7 天历史 / 诊断摘要复制）

- **真实图标**：从 `.app` 包内的 `.icns` 反推可复用尺寸（≥128px），用系统 `sips` 转 64px PNG 后转 base64 data URL；8 路并发、总预算 300ms、进程内 30min 缓存，前端按 family 缓存且失败记空串不重试。转码不可得时退回字母徽标（同一 family 稳定配色），应用卡片与详情页共用
- **小时级历史落库**：新增 `app_hourly` 表（family + 小时桶主键，RSS/CPU 的 sum·n·peak，上下行字节），快照轮内累加、跨小时或满 60s 落盘，缺失小时一律返回 `null` 而不是 0；保留 8 天自动裁剪，历史保留 192 小时，前端按 24h / 7d 聚合
- **口径不混比**：历史走势用 RSS（快照轮全量可得），当前读数仍是 footprint 口径，图下方脚注写明差异，不把两个口径画进同一条线
- **整卡进详情**：应用资源卡片去掉展开态，整卡可点，28px 图标 + 右侧 `›`；详情页含图标/路径/端口徽标/Finder/结束进程、CPU·内存·网络·磁盘四联卡、24h/7d 历史走势、进程表、端口 chips、工作区占用与泄漏证据
- **复制诊断摘要**：一键复制 Markdown 摘要（机器/时间/四域读数/历史覆盖度/Top3 进程），读不到的字段写「不可读（原因）」而非 0；剪贴板走 pbcopy / Set-Clipboard，IPC 失败时退回浏览器剪贴板

### 托盘鲸鱼皮 → 跳跃小猫皮（M11-c）

- 新增「小猫」托盘主题（取代鲸鱼）：**12 帧完整前跃弹道**——站立 → 蓄力下蹲 → 蹬地 → 上升 → 腾空顶点 → 下落 → **前爪触地（后爪悬空）** → 重心前移 → **后爪落地** → 起身回位，竖直方向按抛物线分布（顶点慢、两端快），CPU 压力 = 前跃频率（闲 5.8s 一跃 / 满载 0.52s 一跃，约 11 倍动态范围，满载约 23 fps）
- 造型参考用户指定的黑猫剪影：敦实身形、背部微拱、短粗腿、尾巴上扬卷成开口朝内的圆钩；整体圆润无尖角（圆尖耳、椭圆吻部、颈部椭圆消凹口），爪为「腿杆 + 圆脚掌」两段式，18pt 下有爪的重量感
- 落地严格分先后：前爪触地承重、后爪明显悬空，后爪才收拢着地——「先前爪再后爪」一眼可读
- 动作选型：前脚抬起带动身体前跃（用户指定），起跳/腾空/落地各有独立过渡拍
- 兼容：旧持久化 id `whale` 自动映射到小猫，已选鲸鱼的用户升级后不会被重置成剪刀
- 新脚本 `scripts/gen-tray-cat.mjs`（确定性渲染），鲸鱼资产与脚本移除（git 历史可找回）

### 托盘鲸鱼「游速看 CPU」体感重标定（M11-b）

- 游速动态范围 8 倍 → **约 15 倍**（闲 6.0 s/轮 → 满载 0.39 s/轮），相邻档位 ≈1.5 倍递减，每个 CPU 档肉眼可辨；测试门槛同步从 5 倍收紧到 10 倍
- 动画改读**新鲜 CPU**：新增 1 Hz 轻量 watcher（独立 sysinfo 实例，仅刷 CPU）+ EMA 平滑，负载变化约 1 s 跟手，不再等 10 s 快照；tooltip 仍显示与面板同源的 10 s 均值，两通道各司其职
- 帧资产升级「蹬水前冲」：尾鳍发力拍鲸鱼向前窜、回收拍滑行回落，高速档从「原地抖」变成肉眼可见的「游得急」（`gen-tray-whale.mjs` 确定性重生成，f0 静态图不变）

## 0.4.1

### 统计读数按下载 / 上传拆分（M16 v6）

- 主窗口总览：网络卡拆成「下载」「上传」两张卡，各带 24 px 主读数、独立归一化走势与方向专属脚注（近 5 min 均值 · 今日累计），峰值与会话累计移入悬停提示
- 菜单栏面板：网络行拆成「↓ 下载」「↑ 上传」两条标准行，各带迷你走势与近 5 min 均值；面板高度 400 → 432 贴合内容
- 两端读数同源派生（`netDirectionReadout()`），口径与空值语义不变：读不到显示「不可读 + 原因」，绝不显示 0；`peakAt` 为双方向共用时戳，不逐方向标注时间

## 0.4.0

### 灵动岛（M15）

- **CPU 暴走提报**：持续高 CPU（默认 90% / 60s）时从刘海里长出一条提报，悬停展开看 Top 3 进程，6 秒自走；判定用达标率 + 滞回 + 频控，不误报瞬时尖峰（`prune-core` 11 条单测）
- **内存异常提报（M15.6）**：内核压力 Critical 立即提报，Warning / swap 破阈值持续 30s 提报；展开态显示换出速率、压缩器占用与「换出受害者」Top 3；频控拆 per-event + 全局兜底，CPU 报满额度后内存事件仍能弹出
- **折叠态三段式**：图标 + 事件标签 + 读数（`⚡ CPU 87%` / `▐█▌ 换出 10.4G`），颜色之外的第二事件识别通道
- **盖全屏**：NSPanel + `fullScreenAuxiliary`，不抢焦点、不切 Space；窗口 380×220 开死，动画只变内部形状
- **几何自适应**：刘海宽高实测获取（不写死机型），插拔外接屏 2s 内自动重算落点
- **设置卡**：主窗口新增「设置」tab，阈值 / 持续窗口 / 频控全部可调（`island.*` 8 个键），无刘海屏明确提示
- 修复：透明窗口三层链（DOM → WebView `drawsBackground` → NSPanel）中间层断链导致「岛下一大片空白」

### 网络 tab（M14）

- 系统口径下载 / 上传速率 + 5 min 走势 + 网卡分布 + 按应用 Top 8
- 今日 / 近 7 天流量（`net_daily` 表，60 s 增量落库、90 天保留）
- 修复：总网速改用累计计数器差分，消除偶发读数跳变

### 菜单栏面板（M16）

- 面板改为单页统计（与主窗口同一读数层），主窗口统计页新增横跨两列的网络卡
- 修复：面板高度 560 → 400 贴合内容，不再有大片留白

### 其他

- 顶部托盘图标可换主题，鲸鱼娘 / 鲸鱼 Logo 切换
- 接入 GitHub 源自动升级（updater）
