# Footprint 竞品分析 · 2026-10

> ⚠️ **本文档推翻 `footprint-prd.md` §8「竞争与差异化」的结论。** 那节基于我此前对市场的假设，实测后不成立。§8 已加指向本文档的批注，待战略决策确认后一并修订。

---

## 0. 结论先行

**坏消息**：我在 PRD 里主张的三条差异化——开发者视角分组 / 泄漏嫌疑判定 / 安全回收——**已全部被现成产品覆盖**，其中一个还是**免费开源 MIT** 的，而且它的实现细节（含「永远说疑似」的措辞约束）与我的方案惊人一致。市场比假设的拥挤一个数量级。

**好消息**：真缺口确实存在，但**不在内存监控，在开发环境的可回收资源（磁盘侧）**。这需要产品转向，不是微调。

**最危险的对手不是 iStat Menus，是 Vitals** —— 它已经是「Portlivo + Footprint」的合体：App 级分组 + 30 天历史 + 内存增长告警 + dev servers 按项目分组并可停止。作者是独立开发者，72 小时卖了 800+ 份。

---

## 1. 竞品全景（五层）

| 层 | 代表产品 | 价格 | 与 Footprint 的关系 |
|---|---|---|---|
| **L1 内置** | Activity Monitor | 免费 | 被所有人当作"要超越的基线" |
| **L2 通用菜单栏监控** | iStat Menus、Stats、eul、MenuMeters、iStatistica、Orbl | 免费 ~ $14.99 | 红海，且已被免费开源吃透 |
| **L3 进程级诊断** | ProcXray | $15 一次性 | 进程画像做得比我们更细 |
| **L4 App 级 + 历史 + 开发视角** ⚠️ | **Vitals**、**Mac Performance Monitor** | $9→$29 / **免费开源** | **直接竞品，全面覆盖** |
| **L5 清理类（反面教材）** | CleanMyMac、Memory Cleaner X (Nektony)、MacFreeup、OnyX | 免费 ~ 订阅 | 不进入对比，只作为营销对立面 |
| 相邻 | FavTray | 免费 / Pro $4.99 起 | 端口 + 系统概览，**也在打 Portlivo** |

### 价格与定位表（2026-10 实测）

| 产品 | 价格 | 系统要求 | 开源 | 本地 | 关键能力 |
|---|---|---|---|---|---|
| Activity Monitor | 免费 | 全系 | — | ✅ | 五标签页，5s 刷新，压力图在底部 |
| **Stats** | **免费** | macOS 12+ | **MIT**，42k stars | ✅ | 9 模块 13 组件，空闲 50–80MB |
| eul | 免费 | — | MIT (SwiftUI) | ✅ | 极轻量，图形精美 |
| MenuMeters | 免费 | — | 开源 | ✅ | CPU/内存/磁盘/网络 |
| iStat Menus 7.20 | $11.99 单 / $14.99 家庭(5台) / 升级 $9.99 | macOS 14+ | ❌ | ✅ | 8 个菜单项，传感器最全，Setapp 内 |
| **ProcXray** | **$15 一次性**（3 台） | macOS 14+ | ❌ | ✅ | 进程树 / env / cwd / dylib / 签名校验 / 生命周期追踪 |
| **Vitals** | **$9 首发 → $29**（曾 $5，已售 1000 份） | macOS 15+ | ❌ | ✅ | **App 分组 + 30 天历史 + 增长告警 + dev servers 按项目可停** |
| **Mac Performance Monitor 2.2** | **免费** | Apple silicon + macOS 15+ | **MIT**（GRDB + Sparkle） | ✅ | **泄漏指示 + 压力事件 + 分组 + SQLite 历史 + `mpm` CLI + MCP server** |
| Orbl | 免费 + $39 一次性 | macOS 14+ | ❌ | ✅ | 壁纸仪表盘（不走菜单栏） |
| iStatistica | $9.99 一次性 | — | ❌ | ✅ | 仪表盘 + 小组件 |
| Sensei | $29/年 或 Setapp | — | ❌ | ✅ | 优化向，含清理 |
| FavTray | 免费 / Pro $4.99 起 | macOS 14+ | ❌ | ⚠️ 有服务端 | 端口一键 kill + 系统概览 |

> 另注：**macOS 26 Tahoe 已原生支持菜单栏图标管理**（⌘+拖拽重排、System Settings 菜单栏分区、可定制控制中心）——通用菜单栏管理器的空间被进一步压缩，但**不影响监控类**。

---

## 2. 三大威胁逐个拆

### 🔴 威胁一：Mac Performance Monitor（免费 · MIT · 最接近）

| 它的能力 | 我在 PRD 里写的对应项 | 判定 |
|---|---|---|
| 菜单栏 + 后台记录器 + 窗口三段式 | M1 架构 | 已覆盖 |
| 内存压力 + 最大消耗者 | M1 压力环 | 已覆盖 |
| **Leak indicators + pressure events** | **M2 泄漏嫌疑判定** | **已覆盖** |
| 「Process-growth checks reject stale readings and growth that has settled. Modest findings stay as quiet observations, **not a diagnosis of a memory leak**.」 | 「永远说疑似」措辞硬约束 | **完全一致** |
| 「High swap usage alone is not a reason to warn. Rules look for **continued growth or paging strain**.」 | **M2 交换颠簸告警** | **已覆盖** |
| Groups：把相关 app 与 helper 归组，跟踪合并占用 | 开发者视角分组 | 已覆盖（按 app 而非按类型） |
| SQLite 本地历史 + 保留策略 | 环形缓冲 | 更强（可回溯、可设保留期） |
| `mpm` CLI + **MCP server**（Claude Code / Codex / Cursor） | M3 `footprint` CLI | **更强，且已接 AI Agent** |
| 可选 Full Disk Access + **Full Coverage 特权助手** | 我的 D1 困境 | **它已经有第三条路** |

📌 **对我 PRD 的直接影响**：Mac Performance Monitor 用了**「免权限基础功能 + 可选特权助手增强」**的分层策略。这比我 D1 里的 A/B 二元选择（退回 RSS vs 上特权助手）更优 —— **这是 D1 的正确答案，见 §5**。

### 🔴 威胁二：Vitals（$9→$29 · 商业上最强）

```
Vitals = App 级分组 + 30天历史 + 增长告警 + dev servers 按项目 + 可停止
       = Portlivo 的核心功能 + Footprint 的 M1/M2
```

| 能力 | 说明 |
|---|---|
| App 分组 | ~1000 进程 → ~60 个 App 行，每 App 一行一数值 |
| **30 天 per-app 历史** | 12h / 24h / 7d / 30d 视图（**我们是 24h，差 30 倍**） |
| **内存增长告警** | 官方示例：「Slack 一小时内内存涨了 1.4 GB」——**这就是泄漏判定** |
| **Dev servers by project** | 按项目文件夹分组、列出端口、识别闲置（"storefront-web :4322 跑了 3 天几乎没用"）、询问后可停止 |
| 菜单项 | 每个读数可独立成为菜单项，值/图/两者 |
| 性能 | 无窗口时 5s 采样，约「0.1 个核的十分之一」 |
| 商业化 | 独立开发者 Atta（Fore Studio），72h 售出 800+，$5 首发 1000 份已售罄，$9 进行中 → $29 |
| 路线图 | **MCP server、CLI、AI agent 视图、存储地图与清理工具** —— 我们的 M3 在他路线图上 |

📌 **最刺眼的一点**：Vitals 的 dev servers 功能**直接是 Portlivo 的核心**。也就是说 **Portlivo 自己也在被 Vitals 蚕食**，而我们打算做的「内存守门员」只是 Vitals 的一个模块。

### 🟠 威胁三：ProcXray（$15 · 进程画像比我们深）

| 能力 | 说明 |
|---|---|
| 完整进程画像 | 进程树、命令行、**全部环境变量**、**工作目录**、Bundle ID、启动时间、公司名 |
| 监听端口 + 文件描述符 + 已加载 dylib | 比我们只做内存更宽 |
| 生命周期追踪 | 新进程绿色高亮、退出进程红色留存 |
| 代码签名与 entitlements 校验 | 安全向 |
| 实时正则全局搜索 | 进程名 / PID / 命令行 / 路径 |
| 定位 | 官网直接写「**Built for developers**」 |
| 内容营销 | 已发布《memory pressure vs memory leak》《How to Debug Memory Leaks on Mac Using ProcXray》 |

📌 我们 PRD 里的「进程画像（命令/路径/来源应用）」在 ProcXray 面前**毫无优势**，它连 env 都全给了。

---

## 3. 差异化被打脸清单（逐条对照）

| 我在 PRD 主张的差异化 | 现状 | 判定 |
|---|---|---|
| 把内存压力摆到菜单栏 | iStat Menus / Stats / Vitals / MPM / ProcXray **全部都有** | ❌ **不成立** |
| 交换颠簸（swapin 速率）作为核心指标 | MPM 2.2 已实现「continued growth or paging strain」 | ❌ **不成立** |
| 泄漏嫌疑判定（时间序列 + R² + 措辞克制） | MPM「leak indicators」，且用了同样的措辞克制；Vitals 有内存增长告警 | ❌ **不成立** |
| 开发者视角分组 | Vitals 按 App 分组、MPM 按 Group 分组 | ❌ **不成立** |
| 安全回收（僵尸进程、Simulator、DerivedData） | Vitals 覆盖「停 dev servers」；**DerivedData / 包管理器缓存 / Docker 层无人做** | ⚠️ **部分成立** |
| 24h 趋势 | Vitals **30 天**，MPM SQLite 可设保留期 | ❌ **反而落后** |
| `footprint` CLI | MPM 已发 `mpm` CLI，**还额外做了 MCP server** | ❌ **落后一代** |
| 零权限、无账号、无埋点 | MPM / Vitals / Stats 全部如此 | ❌ **不是差异化，是入场券** |

**结论：PRD §8 的「唯一护城河」三条，零条成立。**

---

## 4. 剩余真空分析（真正还有机会的地方）

挖遍 L1–L4，**只有一处没被覆盖**：

### 🎯 真空：开发环境的可回收资源（磁盘侧）

| 目标 | Vitals | MPM | CleanMyMac 类 | **Footprint 机会** |
|---|---|---|---|---|
| Xcode DerivedData | 路线图（未做） | ❌ | 通用清理，不认识开发目录 | ✅ |
| node_modules 体积与孤儿 | ❌ | ❌ | 通用 | ✅ |
| Docker images / volumes / build cache | 只读 | ❌ | ❌ | ✅ |
| iOS Simulator 设备与运行时 | ❌ | ❌ | ❌ | ✅ |
| pnpm / yarn / CocoaPods / Gradle 缓存 | ❌ | ❌ | 通用 | ✅ |
| 僵尸 dev 进程 + 其残留目录 | 部分 | ❌ | ❌ | ✅ |

**为什么这块没人做**：它是「磁盘空间」和「开发工作流」的交叉地带——系统监控工具不碰磁盘清理（怕变成 CleanMyMac 那种形象），清理工具不认识开发目录。**两边都不愿意跨界。**

**为什么它有价值**：开发者的 Mac 不是在「内存不够」，而是在「DerivedData 40GB + Docker 60GB + node_modules 一堆」，最终表现为磁盘满、备份慢、编译慢、Spotlight 卡。**痛点真实、可量化、可回收、不丢数据风险可控。**

### 次一档真空

| 方向 | 说明 |
|---|---|
| 极窄单用途定位 | 所有人都在做「仪表盘」，没人做「只回答一个问题」的工具。Portlivo 证明了窄而深能卖钱 |
| AI Agent 时代的资源归属 | MPM 2.2 才开始识别 Ollama / MLX / LM Studio；「哪个 agent 跑的哪个模型吃了多少」刚起步 |

---

## 5. 三条战略选项

| 选项 | 内容 | 评估 |
|---|---|---|
| **A · 不做** | 承认 M1+M2 已被免费开源覆盖，放弃 | 🟢 诚实选项。若目标是赚钱，继续做内存监控大概率打不过免费 + MIT |
| **B · 转向「开发环境资源回收」** ⭐ | 内存压力**只作为入口**，真正交付的是**可回收清单**（见 §4 真空） | 🟢 **推荐**。唯一真空，且可复用全部技术栈与进程画像能力 |
| **C · 极窄免费开源换心智** | 不求变现，做最窄最好的内存哨兵，换声誉与后续转化 | 🟡 可行，但要先接受不赚钱 |

### 若选 B，产品定义修订

| 项 | 原（PRD） | 修订后 |
|---|---|---|
| 一句话 | 你的 Mac 卡，是因为谁在漏内存？ | **你的 Mac 空间去哪了？把开发垃圾一次收干净** |
| 主轴 | 内存压力 + 泄漏判定 | **可回收资源清单**，内存压力作为顶部状态条 |
| 核心动作 | kill 进程 | **删除 / 清理**（带安全分级与预览） |
| 差异化 | 泄漏判定 | **只认识开发目录**：DerivedData / node_modules / Docker / Simulator / 缓存 |
| 风险 | 误报泄漏 | 🔴 **误删**。必须：预览 + 分级 + 永不默认勾选 + 可撤销 |
| 对手 | Vitals / MPM | **无人直接竞争**（CleanMyMac 是形象上的对立面，不是功能对手） |

> ⚠️ 选 B 必须接受的代价：「删除」是破坏性操作，产品责任数量级上升。**安全分级（🟢直接删 / 🟡需确认 / 🔴只给命令）必须前置设计，不能事后补。**

---

## 6. PRD 决策点修订

| # | 决策点 | 原建议 | **修订** |
|---|---|---|---|
| **D1** | 拿不到 `phys_footprint` 怎么办 | A 退回 RSS / B 上特权助手 | **D1-D 分层**：免权限基础功能 + **可选特权助手增强**（学 MPM 2.2）。基础功能绝不依赖权限，特权只用于增强项 |
| **D2** | 采样频率 | 10s / 60s | 维持，但**历史保留期需对齐竞品**：Vitals 30 天，我们 24h 会被直接比下去 → 建议 **7 天** |
| **D3** | 产品名 | Footprint | 若选 B，Footprint（内存足迹）不再贴切，需重命名 |
| **D4** | 是否做内存清理按钮 | 不提供 | 维持「不做 purge」，但选 B 后改为**做开发目录回收**（本质不同：purge 清缓存骗人，清 DerivedData 真的回收空间） |
| **D5** | 商业化 | 一次性买断 $9.99 → $19.99 | 需重估：Vitals $9 首发 + 免费 MPM 双重挤压，**$9.99 上限可能过高**，或考虑免费开源 |
| **D7** | 🆕 是否与 Vitals 正面竞争 | — | **不建议**。它是 Portlivo + Footprint 合体，且先发 800+ 份 |
| **D8** | 🆕 是否用 MCP server 换 AI Agent 心智 | — | MPM 已做。若要差异化，这是**唯一还没被商业产品占住**的高地 |

---

## 7. 待补充验证

- [ ] 实装 Vitals 与 Mac Performance Monitor 各跑 48h，对比：自身开销、压力读数与 Activity Monitor 的偏差、泄漏告警误报率
- [ ] 确认 MPM 的 `mpm` CLI 与 MCP server 是否覆盖我们设想的 CLI 场景
- [ ] 统计一台真实开发机上 DerivedData / Docker / node_modules 的合计可回收量（这是选 B 的规模依据，**若中位数 < 15GB，B 的价值存疑**）
- [ ] 确认 `~/Library/Developer/...` 与 `~/Library/Containers/com.docker.docker/...` 遍历的真实耗时与 TCC 边界

---

## 8. 一句话建议

> **别做内存监控了 —— 免费开源的 Mac Performance Monitor 和 $9 的 Vitals 已经把我们想做的全做了，而且做得更早。**
> 唯一还在的机会是**开发环境的磁盘回收**，那是监控工具不愿碰、清理工具看不懂的交叉地带。要转，就现在转；不转，就别开这个项目。
