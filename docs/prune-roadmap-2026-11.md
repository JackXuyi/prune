# Prune 产品 Roadmap · 2026-11

> 依据：竞品全景档案（[competitive-landscape-2026-11.md](./research/competitive-landscape-2026-11.md)）+ 2026-11-13 最新竞品实测搜索 + 项目现状（v0.8.3 / 2026-11-10 工程review）
> 方法：竞品动态 → 现状盘点 → 差距分析 → 分阶段 Roadmap → 决策点

---

## 0. 结论先行

1. **最大的威胁不是清理赛道，是 Vitals**：两个月发了 9 个版本（1.4→1.11），新增 CLI、进程Inspector、端口/open files/Homebrew、Docker 容器、AI Agent 会话追踪与 AI plan 余额——**它正从监控侧向「开发者全套工具箱」扩张，且已把 storage map 列入 roadmap**。一旦落地，与 Prune 的磁盘侧正面相撞。Vitals 定价已降到 $29 买断，流速极快。
2. **清理赛道的入场券已变成「收费墙」**：DevCleaner 新装用户把 Warning/Danger 级清理转成了 Pro 功能（免费只剩 Safe 级），靠 Auto Clean / Smart Triggers / 项目休眠归档三件套收 $59 买断。**安全叙事不再是差异化，商业化节奏才是**。
3. **Prune 的窗口期**：三个独家能力（跨平台二进制探测 / 多包管理器重复存储 / phys_footprint 真实口径 + 换出受害者）仍无竞品跟进，但覆盖广度（AI 缓存、Xcode 深扫）是最易被追赶的短板，需在 Vitals storage map 落地前补齐并完成 v1.0 商业化卡位。
4. **Roadmap 主线**：先修工程与诚信欠账（M26）→ 补两个 P0 功能差距（M27/M28）→ 差异化加深而非追广度（M29）→ 商业化卡位（M30/v1.0）。**明确不追 Mole 式五合一广度**。

---

## 1. 竞品最新动态（2026-11-13 实测，增量于 11-11 档案）

### 1.1 DevCleaner —— 商业化收紧 + 渠道扩张

| 维度 | 最新状态 | 对 Prune 的含义 |
|---|---|---|
| 扫描面 | 30+ 工具链与 AI 应用（新增 Codex 存储路径指南） | 覆盖广度继续拉大 |
| **免费/付费切分** | **新装用户 Warning/Danger 级清理、自定义目录、项目产物清理均转 Pro**；免费仅剩 Safe 级 + 扫描 + Ghost Projects 发现 | 竞品验证了「深度清理可收费」；Prune 商业化可参考但不牺牲安全叙事 |
| 定价 | Pro Lifetime $59（早鸟 $29 已售罄）/ Yearly $19.99，14 天试用，3 台设备 | 价格锚点：$19.99–59 区间 |
| 新能力 | 桌面 Widget、**项目休眠归档**（打包单文件 + dry-run 还原 + 一键恢复）、batch archive | 休眠归档仍是独家付费锚点，Prune 未做 |
| Roadmap | 用户投票制：「按增长着色的 Disk map」「自定义目录 Git 仓库预警」 | 竞品在做磁盘可视化方向，Prune 刻意不做 treemap 的定位仍成立 |
| 渠道 | 已上 Mac App Store（2026-07）+ 支持 Linux 设备授权 | 渠道面扩张 |

### 1.2 Vitals —— 高速扩张，正面撞车临近 ⚠️

| 版本 | 时间 | 新增 | 与 Prune 的重叠 |
|---|---|---|---|
| 1.4/1.4.1 | 10-08 | MCP server（Claude/Codex/Cursor），退出进程需人工确认 | Prune MCP 已有，但 Vitals 的「AI 请求 → 人确认窗口」交互值得借鉴 |
| 1.5 | 10-11 | 颜色 + Neural Engine tab | — |
| 1.6 | 10-14 | **一年历史**（逐小时） | Prune 30 天（M25 已补大半差距） |
| 1.7 | 10-21 | **AI Agent 会话追踪 + AI plan 余额** | 差异化方向：竞品盯「AI 时代开发者资源」新叙事 |
| 1.8 | 10-31 | **`vitals` CLI** | Prune 单二进制双形态（GUI/CLI/MCP 同核）更优 |
| 1.9 | 11-06 | 进程 Inspector（签名/TeamID/沙箱/完整命令行） | — |
| 1.10 | 11-11 | 风扇控制 | — |
| 1.11 | 11-11 | **端口 + open files + Homebrew + Docker 容器** | 端口侧与 Prune 重叠加深 |
| Next | — | **Storage map（磁盘地图）** | **一旦落地即正面相撞** |

- 定价调整为 **$29 买断**（原 $39），Single/Duo/Family/Team/Business 五档 + 「一年更新」$15 档；9 月以来 72 个新功能、120 项新增。
- **判断**：Vitals 证明了「开发者视角监控」可以高速商业化，也在快速蚕食 Prune 的端口/进程领地。Prune 不可能在功能广度上跟进这种发布节奏，**必须靠独家探测能力 + 安全/口径叙事 + 更深的数据可信度建立壁垒**。

### 1.3 其余竞品（11-11 档案口径，无重大变化）

- **Mole**（tw93）：五合一 $19 买断，广度挤压仍在，无新动作。
- **CacheCleaner**：免费开源只读，定位互补不构成威胁。
- **CleanMyMac CLI**（MacPaw）：early preview，渠道与品牌威胁长期存在。
- **Mac Performance Monitor**：免费开源 MIT，泄漏判定逻辑与 Prune 等价，叙事不占优。

---

## 2. 项目现状盘点（v0.8.3 @ 2026-11-13）

### 2.1 已完成能力（实际状态，README 徽章停在 v0.5.0 有漂移）

| 域 | 状态 |
|---|---|
| 磁盘浪费探测 | ✅ 跨平台二进制探测（独家）+ 重复存储统计（独家）+ 🟢🟡🔴 分级 + 20GB 熔断 + 白名单 + 废纸篓 |
| 内存真相 | ✅ phys_footprint 口径（独家）+ swap 仪表 + 换出受害者 + 泄漏侦探案件簿（M22） |
| 监控统计 | ✅ 统计 tab 六卡 + 应用资源 tab + **应用历史 30 天**（M25，对标 Vitals） |
| 网络 | ✅ 网速/流量 tab（90 天日流量）+ 分层诊断 + RPM（M17） |
| 局域网 | ✅ 设备卡 + 时间线（M19）+ 星形拓扑（M20/M20-b）+ 新面孔灵动岛提报（M23） |
| 灵动岛事件流 | ✅ CPU 暴走 / 内存异常 / 端口冲突（M19）/ 新设备（M23）四类 |
| 特色叙事 | ✅ 项目健康卡（M24）+ 剪除成绩单（M21）+ 宠物状态机（M20）+ 菜单栏皮肤 |
| 多形态 | ✅ GUI / CLI / MCP（5 只读工具）单核三形态 |
| i18n | ✅ 中英双语全链路 + Rust/TS 同源门禁（0.7.0） |
| Windows | ⚠️ 部分：网络诊断/局域网/托盘/locale 双平台已适配，w1/w2 验收文档存在，但对外仍按 macOS 13+ 宣传 |
| Updater | ✅ 0.5.0 起接 GitHub 源自动升级 |

### 2.2 欠账（2026-11-10 工程review P1 项，均未修复）

| # | 问题 | 风险 |
|---|---|---|
| P1-1 | CI 不跑任何测试（283 个测试仅本地生效） | 护栏可被静默破坏 |
| P1-2 | dev 分支伪造清理成功（diskStore execute） | 违背「不撒谎」产品红线 |
| P1-3 | CSP 完全关闭 | 依赖投毒无第二道防线 |
| P1-4 | crates 版本漂移（MCP serverInfo 报 0.1.0） | 诚信细节 |
| P1-8 | README 徽章/spec 隐私声明「无网络请求」与实现矛盾 | **隐私声明是法律级文案，必须改** |
| P1-9 | 无 LICENSE 文件 | 公开发布法律前提 |

### 2.3 功能差距（竞品已做、Prune 未做）

| 差距 | 竞品 | 优先级判断 |
|---|---|---|
| AI 应用缓存（Cursor/Claude/Ollama/LM Studio） | DevCleaner / CacheCleaner / Mole 全支持 | **P0**：2026 增长最快的缓存类别，影响「首屏可回收 GB」的数字说服力 |
| Xcode 深扫（DerivedData/DeviceSupport/模拟器） | DevCleaner / CacheCleaner / Mole | **P0**：白名单成本低收益高 |
| 项目休眠归档（archive + dry-run 还原） | DevCleaner Pro 独家付费锚点 | P1：商业化 Pro 功能候选 |
| 自动清理 / Smart Triggers | DevCleaner Pro | **决策点**：与「人在回路」原则冲突，见 §5 |
| 磁盘 treemap | Mole / DaisyDisk / Vitals(roadmap) | 刻意不做（定位红线） |
| 广度（卸载器/软件更新/风扇） | Mole / Vitals | 刻意不做（不追广度） |

---

## 3. Roadmap 总览

| 阶段 | 里程碑 | 主题 | 对应竞品压力 |
|---|---|---|---|
| 立即 | **M26 修复与诚信** | 工程欠账清零 + 文档归真 | 发布与商业化的前提（✅ 0.8.4） |
| 近期 | **M27 AI 缓存白名单** | 补最大功能差距（P0） | DevCleaner/CacheCleaner/Mole 全支持（✅ 一期 0.8.3 / 二期 0.8.5） |
| 近期 | **M28 Xcode 深扫专项** | 白名单低成本高收益（P0） | 同上（✅ 随一期 0.8.3 落地） |
| 中期 | **M29 MCP 深化 + AI 信任叙事** | 只读 MCP 扩面 + 「AI 只读、人执行」差异化 | Vitals 1.4/1.7 agent 方向 |
| 中期 | **M30 项目休眠归档（Pro 候选）** | 商业化锚点功能 | DevCleaner Pro 独家 |
| 版本 | **v1.0 商业化卡位** | 定价/渠道/官网 | 抢在 Vitals storage map 落地前 |
| 长期 | **M31 Windows 完整化决策** | 二平台还是单平台深耕 | DevCleaner 已支持 Linux 授权 |
| 永不做 | — | treemap / 卸载器 / 通用套件 / 任意时间序列 | 定位红线 |

---

## 4. 分阶段细化

### M26 修复与诚信（1 个版本，全部小改动，见 review §五清单）

1. CI verify job 加 `cargo test` / `clippy -D warnings` / `fmt --check`（3 行）
2. `diskStore.ts` execute 加 `import.meta.env.DEV` 判断（1 行）
3. `tauri.conf.json` 写 CSP（1 行）
4. crates `version.workspace = true` + 测试断言改 `env!`
5. 删 4 个死代码文件（约 900 行）
6. 双 `home_dir()` 收敛、14 处 `void invoke` 加 `.catch`
7. README 徽章/spec §8/隐私声明「无网络请求」改写 + CHANGELOG 归位 + **补 LICENSE（MIT）**
8. `cargo fmt` 一次性统一 + rustfmt.toml

**验收口径**：review 报告 P1 全部归零；README 与 v0.8.3 实际能力一致。

### M27 AI 应用缓存白名单（P0 功能差距）

> **状态：已落地**——一期（Cursor 6 / Claude 3 / Ollama 2）随 0.8.3 发布（2026-11-11，
> commit `210ef18`）；二期（OpenAI 家族 / Codex CLI / HuggingFace / LM Studio /
> Windows AI 条目，14 条本机实测）随 0.8.5 发布，方案见 [f11](./f11-ai-cache-2-plan.md)。
> 本文保留原规划口径存档。

- 纳入 A1 扫描路径白名单：Cursor / Claude / Codex / Ollama / LM Studio 等 AI 应用缓存路径
- 分级规则：**模型权重与配置/会话分离判定**——权重 🟡（可重下）、含会话/凭据路径硬拒绝（继承白名单机制）
- CLI `prune scan --json` 与 MCP `disk_report` 同步透出新分组
- 对齐产品主张：让首屏「可回收 GB」反映 2026 年真实浪费结构

### M28 Xcode 深扫专项（P0，与 M27 同版本可并行）

- DerivedData / DeviceSupport / 模拟器 runtime / archives 四组，各自独立 🟢🟡 判定
- 模拟器旧 runtime 按可重下判定（对标 simctl 语义）
- 白名单登记 + 负面测试（继承护栏测试组模式）

### M29 MCP 深化 + AI 信任叙事（差异化主线，不追 Vitals 广度）

- 补只读 MCP 工具：`disk_scan`（细分视图）、`net_diag`（诊断结论）、`app_resources` 已有；**reclaim 在 MCP 通道保持不可执行**（底线不动）
- 借鉴 Vitals「AI 请求 → 人确认」交互：若未来开放任何执行通道，必须走 GUI 确认窗口
- README/官网新增「AI Agent 友好」叙事：Prune 是唯一把「AI 只读 + 人执行 + 白名单兜底」写进代码层的工具
- 触发条件：Vitals storage map 上线时，官网对比表同步更新

### M30 项目休眠归档（Pro 功能候选）

- 语义对齐 Prune 原则：archive = 移动到受管归档区（废纸篓语义之外、可完整还原），dry-run 还原预览，验证后才允许删原件
- 复用 M24 项目健康卡的「僵尸依赖」发现侧作为入口
- 归属商业化阶段：免费版保留「发现 + 账本」，归档/批量归档为 Pro

### v1.0 商业化卡位（建议在 Vitals storage map 落地前完成）

| 决策项 | 建议 | 依据 |
|---|---|---|
| 定价 | **$19–29 买断**（单授权）+ 可选「一年更新」档 | Vitals $29 / Mole $19 / DevCleaner $59——Prune 定位介于深度工具与套件之间 |
| 免费线 | 扫描 + 🟢 级清理 + 全部监控免费；🟡 级与归档为 Pro | 参考 DevCleaner 新切分，但**不把「安全清理」本身锁付费**（守住安全叙事） |
| 渠道 | 官网直售（Paddle/Lemon Squeezy 类）优先；App Store 后置（沙箱与权限模型冲突需评估） | DevCleaner 双渠道在先，MAS 审核对删除类工具不友好 |
| 叙事 | 「守门员 vs 万金油」+ 「口径不撒谎」+ 「零权限零遥测」 | 三个差异化全部仍无竞品跟进（11-11 档案 §5） |

### M31 Windows 完整化（决策点，不承诺排期）

- 现状：网络/局域网/托盘/locale 已双平台，但磁盘侧白名单、footprint（Windows 无等价物）、updater 均未验证
- 决策规则：v1.0 商业化后看 macOS 端转化数据——若开发者 Windows 占比可观再投入，否则单平台深耕（单平台是能力深度的护城河，DevCleaner 的 30+ 生态广度路线 Prune 不跟）

---

## 5. 关键决策点（需拍板）

| # | 决策 | 选项 A | 选项 B | 建议 |
|---|---|---|---|---|
| D1 | 自动清理/Smart Triggers 是否做 | opt-in 仅 🟢 级白名单内定时清理（DevCleaner 验证过付费需求） | 不做，坚守「人在回路」 | **先不做**；v1.0 后若 Pro 数据需求强，做「仅 🟢 级 + 每次记录进成绩单」的 opt-in 版本，安全叙事可兼容 |
| D2 | 商业化时点 | v1.0 即收费 | 继续免费积累口碑 | **v1.0 卡位**：Vitals storage map 落地前建立「深度清理付费」心智，晚收比早收难 |
| D3 | M30 与 v1.0 顺序 | 归档做完再商业化 | 先商业化、归档作为 Pro 首个交付 | **先商业化**：免费/付费切分可先上（🟡 级清理转 Pro），归档随后交付，避免重蹈 DevCleaner「切分引发老用户流失」的争议（其靠 legacy grandfathering 兜底） |
| D4 | Vitals 撞车应对 | 更新对比表强调口径/安全差异 | 部分跟进其功能 | **A**：Vitals 是 RSS 口径 + 需可选特权助手，Prune 的 footprint + 零权限是硬差异，不追其发布节奏 |

---

## 6. 风险登记

| 编号 | 风险 | 等级 | 应对 |
|---|---|---|---|
| RK-1 | Vitals storage map 落地 + 其发布节奏吸走「监控+清理」双需求用户 | 高 | v1.0 商业化卡位 + 官网对比叙事常备 |
| RK-2 | AI 缓存路径变更频繁（Cursor/Claude 迭代快） | 中 | 白名单机制天然支持热更新登记；M27 落地后建立版本跟踪清单 |
| RK-3 | P1 工程欠账期间发新功能，护栏回归风险 | 中 | M26 先行且是 M27/M28 的前置门禁 |
| RK-4 | 免费转付费引起存量用户反弹 | 中 | 学习 DevCleaner grandfathering 教训：存量用户保留既有能力 |
| RK-5 | 商业化拖慢个人项目发布节奏 | 低 | 每里程碑保持「可发布」状态，商业化组件（授权校验）独立可灰度 |

---

## 附：信息来源

- 项目内：[competitive-landscape-2026-11.md](./research/competitive-landscape-2026-11.md)（2026-11-11）、[project-review-2026-11-10.md](./project-review-2026-11-10.md)、CHANGELOG（0.4.0–0.8.3）
- DevCleaner 官网 / pricing 页 / Mac App Store（2026-11-13 实测：Warning/Danger 转 Pro、$59/$19.99、休眠归档、桌面 Widget、roadmap 投票）
- Vitals 官网 pricing / changelog / 作者博客 / Product Hunt（2026-11-13 实测：1.4→1.11 版本线、$29 定价、storage map roadmap、MCP 人确认交互）
- 本方案为产品规划参考，具体实施请结合发布计划与合规要求确认。
