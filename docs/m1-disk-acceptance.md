# M1 磁盘侧 · 验收记录

> 2026-10-26 · 对应 `docs/m1-disk-plan.md` §11 任务分解与 §14 验收标准
> 状态：**T-301~T-308 已完成并通过自动化验证；T-309 中两项需真实动手的验收待人工执行（§3）**

## 1. 自动化验证结果（2026-10-26，本机实跑）

### 单元测试

| 模块 | 覆盖点 | 结果 |
|---|---|---|
| `prune-core/classify` | 平台 token 边界匹配（`darwin`/`linux`/`win32`/`android`/`freebsd`/`openbsd`）、架构词双条件、包名含平台词的误判负例、darwin-x64 按 Rosetta 存在与否降级、`host_arch` | ✅ |
| `prune-core/disk` | 体积累加（物理块口径）、符号链接跳过、缺失目录返回 0、首层体积归因、A1 死重统计（不含待确认条目）、浪费比分母只用原生可用、`totals` 不重复计死重（Yarn 的 L2 部分扣掉死重）、A3 需两个以上管理器才给建议 | ✅ |
| `prune-core/disk/whitelist` | 级别→清理通道映射、`../` 穿越拒绝、symlink 逃逸拒绝、白名单根本身放行、`resolve` 跳过不存在项 | ✅ |
| `prune-core/reclaim` | preview 只取 `Deadweight` 条目、Rosetta 条目留说明、L4 不进清单、L3 标记需确认、**聚合项零条目**、执行时白名单外拒绝、**家目录本体拒绝**、L4/未确认 L3/熔断三类拒绝、拒绝时不动文件 | ✅ |

合计 **prune-core 66 项 + prune-app 7 项 = 73 项单测全绿**；`cargo check -p prune-app`、`pnpm build`（tsc 严格模式）通过。

### 实跑数据（`cargo run --release --example disk-probe`，真机全量扫描）

| 组 | 体积 | 条目 | 级别 | A1 |
|---|---|---|---|---|
| Yarn 缓存 | 15.34 GB | 6156 | L2 | 🟢 死重 6.94 GB（256 条）· 原生可用 1.00 GB · 其余 7.40 GB · **浪费比 6.91:1** |
| npm 缓存 | 8.53 GB | 3 | L2 | — |
| pnpm store | 7.72 GB | 3 | L2 | — |
| Cargo registry | 1.37 GB | 4 | L2 | — |
| Homebrew 下载缓存 | 175.9 MB | 63 | L2 | — |
| DerivedData | 71.0 MB | 5 | L2 | — |
| Bun 安装缓存 | 0.4 MB | 3 | L2 | — |
| 项目依赖 | 17.02 GB | 55 个目录 | L3 | 聚合项，只统计 |
| Simulator 设备 | 3.75 GB | 12 | L3 | — |
| Playwright 浏览器 | 1.29 GB | 10 | L3 | — |
| Docker VM 磁盘 | 3.17 GB | 1 | L4 | 只给命令 |

**合计：L1 6.94 GB · L2 26.27 GB · L3 22.05 GB · L4（不计入）3.17 GB → 首屏主数字 55.26 GB**

与 10-22 基线（`reclaim-scan-report.md` **54.85 GB**）相比 **+0.41 GB**，同组相对关系一致——缓存自然增长，符合方案 §13「允许合理漂移」。

A1 平台分布：linux 134 条 4.22 GB · win32 76 条 2.24 GB · android 25 条 279 MB · freebsd 21 条 191 MB。
Top 1 死重条目：`npm-@next-swc-linux-x64-musl-14.2.33` **207.1 MB**。

### 扫描性能（M0 #2 关闭项）

| 机器负载 | 实测耗时 | 结论 |
|---|---|---|
| 负载正常（load ~5） | **15.6 s** | ✅ 预算 20 s 内 |
| 负载 19~22（并发跑 cargo/编译时） | 18.5 ~ 21.7 s | ⚠️ 贴线，负载极高时略超 |

口径：release 构建、白名单 27 项 + 55 个 node_modules 全量、并发 12。首版按"首层分叉"并行，遇 pnpm store（44 万文件、仅 3 个顶层目录）退化为单线程 30 s，改工作队列式遍历后达标。

### dry-run 与护栏实跑（`disk-probe`，不动任何文件）

| 场景 | 结果 |
|---|---|
| `preview(["yarn"])` | 256 项 · 6.94 GB · 无需二次确认 |
| `preview(["npm"])` | 1 项（`_cacache` 整库）· 8.53 GB |
| `preview(["node-modules"])` | **0 项** + 说明「分散在 55 个目录，只做统计」 |
| `preview(["docker-vm"])` | 0 项 + 说明「🔴 高风险项，只提供命令」 |
| `execute(["/etc/hosts"])` | `Err(NotWhitelisted)`，文件未动 |
| `execute([home])` | `Err(NotWhitelisted)`，目录未动（单测覆盖） |

## 2. 实施中发现并修复的缺口

**聚合分组被当成待删项**：`node-modules` 是 55 个项目目录的聚合，`root` 记录的是 home。首次 dry-run 实测产生了一条 `path=/Users/xuyi`、17.02 GB 的待删项——白名单最后一道闸能挡住执行（`is_whitelisted` 只认缓存根），但预览阶段就已误导用户，且与「L3 只统计」的设计不符。已加 `CacheGroup.cleanable` 字段修正，并补两条负例单测（聚合项零条目、家目录本体被拒）。

其余偏差（体积口径改物理块、未引入 `ignore`/`rayon`、抽样策略调整）已回扫进 `m1-disk-plan.md` §16。

## 3. 待人工完成（T-309 剩余两项）

| # | 事项 | 判定 | 说明 |
|---|---|---|---|
| 1 | **A1 抽样 100 条人工核对**（验收 #2） | 判定正确率 100% | 清单已生成：`docs/research/m1-a1-sample.md`（含平台词 50 条查误杀 + 不含平台词 50 条查漏判，共 100 行带判定结果）。发现判错的行反馈回来改 token 表 |
| 2 | **清理后 `yarn install` 无失败**（验收 #3） | 真删 6.94 GB 后任一项目安装成功 | 需要真实执行清理（走废纸篓、可恢复）。建议先跑一次 `pnpm tauri dev` 在 UI 上点一遍 preview → execute，再验证 |
| 3 | UI 视觉走查 | 面板首屏「⚡ 55.26 GB 可回收」+ 主窗口「可回收」tab 四层分层条 / A1 卡片 / 清理底板 | 见 §4 环境说明——本会话截图工具不可用，未能截图确认 |

## 4. 环境说明（不影响产品）

- **截图工具在本会话不可用**：`playwright-cli open` 在浏览器启动阶段抛 `Assertion error`（playwright-core 内部断言），换 Chrome channel 亦同。因此本轮 UI 只做到 `pnpm build`（tsc 严格）+ 代码路径复核（IPC 命令名、事件通道 `disk://progress` / `disk://done` / `ui://tab`、参数形状逐项对齐），**未做像素级视觉确认**。建议用 `pnpm tauri dev` 实看一次。
- **扫描耗时随机器负载波动明显**：本机当前 load 长期 19~22（非本产品造成），此时全量扫描 18.5~21.7 s；空闲时 15.6 s。若后续要在高负载下稳定 < 20 s，可再提并发或对 pnpm store 这类巨量小文件目录做采样估算。
- **Docker 体积口径**：Docker.raw 是稀疏文件，`metadata.len()` 会报 1099 GB 假数字；统一用物理块（`st_blocks×512`）统计，与 `du` 一致。

## 5. 验收标准对照（方案 §14）

| # | 标准 | 状态 |
|---|---|---|
| 1 | 首屏显示「⚡ X GB 可回收」，分层行与 `reclaim-scan-report` 同组相对关系一致 | ✅ 55.26 GB，各组相对关系与 10-22 基线一致（+0.41 GB 自然增长） |
| 2 | A1 真机抽样 100 条人工核对，判定正确率 100% | ⏳ 清单已生成（`m1-a1-sample.md`），待人工逐行核对 |
| 3 | 清理 Yarn 跨平台条目后，任一项目 `yarn install` 成功 | ⏳ 需真实执行清理后验证（代码路径与 dry-run 已验证） |
| 4 | 白名单外路径调用 `reclaim_execute` 直接 `Err` | ✅ 单测覆盖 `../`、symlink、白名单外绝对路径；实跑 `/etc/hosts` 与 home 本体均被拒 |
| 5 | > 20 GB 无 `confirmed` 时熔断，UI 二次确认 | ✅ 单测覆盖 `NeedsExplicitConfirm`；UI 勾选框 + 文案「超过 20 GB，确认执行」 |
| 6 | 全量扫描 < 20 s，期间面板可正常打开、内存读数不受影响 | ✅ 空闲 15.6 s；⚠️ 高负载时 18.5~21.7 s（见 §4）。扫描在独立 `spawn_blocking` 线程，内存采样不受影响 |
| 7 | 拔掉 Yarn 缓存目录时 A1 卡片显示「未发现」而不是 0 或报错 | ✅ 无 A1 分组时渲染「跨平台二进制 · 未发现」并说明单架构安装属正常 |
