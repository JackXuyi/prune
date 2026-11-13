# Changelog

所有可见变更记录于此。版本号由 `scripts/bump-version.mjs` 统一维护（package.json / tauri.conf.json / Cargo.toml 四文件一致：根 workspace `version` + src-tauri `[package] version`）。

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
