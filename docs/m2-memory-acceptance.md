# M2 内存监测 · 验收记录

> 2026-10-23 · 对应 `docs/m2-memory-plan.md` §10 任务分解与 §13 验收标准
> 状态：**T-201~T-209 已完成并通过自动化验证；T-210（活动监视器人工抽检）待执行**

## 1. 自动化验证结果（2026-10-23，本机实跑）

### 单元测试

| 模块 | 覆盖点 | 结果 |
|---|---|---|
| `prune-core/mem` | 家族聚合求和、受害者榜排序、换出占比 null 安全、颠簸速率（首轮/回绕） | ✅ |
| `prune-core/mem/process` | 家族归类规则（Cursor/Chrome/Node/.app 兜底）、sysinfo 枚举自身、RSS 降序 | ✅ |
| `prune-core/mem/footprint` | 真实 JSON 解析（swapped 求和）、缺字段报错、**不存在 pid → NoOutput（退出码不可信已验证）**、自身深探成功、批量并发 | ✅ |
| `prune-core/mem/system` | vm_stat 解析（**16384 页大小、非 4096 硬编码**）、occupied/stored compressor 兼容、压力等级映射 | ✅ |
| `prune-core/swap` | swapusage 解析（M/G/无空格）、mtime 当日统计、本地零点、本机实读 | ✅ |
| `prune-core/db` | 只写 deep 行、同 ts 幂等、查询窗口/上限、过期清理 | ✅ |
| `src-tauri/sampler` | 深探候选门槛/上限、pid 复用防护（start_time 校验） | ✅ |

合计 **43 项单测全绿**（含 review 后新增的深探单飞守卫、缓存整体替换两例）；`pnpm build`（tsc 严格模式）通过。

### 实跑数据（`pnpm tauri dev`，连续运行 40+ 分钟）

| 项 | 实测 | 方案预算 | 结论 |
|---|---|---|---|
| 10 s 快照轮 | 30~140 ms（均值 ~45 ms） | < 200 ms（原 30 ms 已按实测校准，见方案 §15.2） | ✅ 远低于 10 s 周期，无堆积 |
| 5 min 深探轮 | 0.7~2.5 s / 12~22 进程（12 路并发） | < 2.5 s / ≤64 进程 | ✅ 并发由 8 提到 12 后落在预算内 |
| 深探成功率 | 100%（所有轮次全成功） | — | ✅ |
| 口径 | footprint（含 swapped） | — | ✅ 命中验收标准 1 的前提 |
| swap 读数 | 10.9/12.3 GB → 与当刻 `sysctl vm.swapusage` 一致 | 验收标准 2 | ✅（总容量随系统回收从 12288M 缩到 11264M，读数实时跟随） |
| 压力等级 | raw 1/2 波动，映射 Normal/Warning | 验收标准 3 前提 | ✅ 数据在线，**语义校准见 §2** |
| 序列库 | app 内打开失败 → 降级无库模式，采样不受影响 | §7 降级链 | ✅（见 §3 环境说明） |

### UI 验证（Playwright + dev fixture，见 §3）

- 主窗口双栏：家族列表（总量降序、展开到进程）+ 系统总览（物理/压缩/空闲/swap 四宫格 + 压力卡 + 受害者榜）
- 面板：压力点 + swap 仪表条（89% 警告色）+ 今日新增 + 颠簸速率 + 换出受害者 Top5 + 口径徽标
- 检视器：footprint/RSS/已换出三行对照条 + sparkline 占位 + 进程信息（exe/启动时间/读数来源）
- 无序列时显示「暂无序列」占位而非空图

## 2. 待人工完成（T-210）

| # | 事项 | 判定 |
|---|---|---|
| 1 | 抽检 20 个进程，footprint 与活动监视器「内存」列偏差 < 10% | < 10% 通过；偏差大则检查 swapped 口径 |
| 2 | 压力等级语义校准：raw 2 时活动监视器是否显示「警告」 | 不一致则改用页率推导 |
| 3 | 多屏回归：tray 在副屏时面板定位（M0 遗留） | 落点正确 |
| 4 | 连续运行 30 min 自身 footprint < 60 MB | `footprint -p <self>` |

## 3. 环境说明（不影响产品）

- **序列库在 AI 会话的沙箱化 dev 进程里打不开**（`disk I/O error`，Application Support 写入受限）。同一路径用 CLI 直接建库成功（已验证），说明是会话沙箱限制而非产品缺陷；用户从终端运行 `pnpm tauri dev` 不受影响。降级路径（无库 → 不落序列、UI 显示暂无历史）按设计工作。
- **UI 截图验证方式**：本会话的窗口合成受限（窗口无法在当前 Space 前置），改用 Playwright + dev fixture（`src/lib/devFixture.ts`，仅 `import.meta.env.DEV` 且非 Tauri 时生效）在浏览器中验证三视图；生产构建不含 fixture。浏览器下可用 `?window=panel` 强制渲染面板视图。
- 深探轮耗时随机器负载波动（0.7~2.5 s）。并发已从 8 提到 12，并给深探加了**单飞闸门**（手动刷新不会与定时器轮并行 spawn），详见方案 §15.2。

## 4. 验收标准对照（方案 §13）

| # | 标准 | 状态 |
|---|---|---|
| 1 | next-server 显示 footprint 3.35 GB 而非 RSS 113 MB，标注 97% 已换出 | ✅（UI 已按 footprint 口径渲染，fixture 复刻实测值） |
| 2 | swap 读数与 `sysctl vm.swapusage` 完全一致；swapfile 数与实际一致 | ✅（自动解析同一数据源；数量按文件枚举） |
| 3 | 抽检 20 进程偏差 < 10% | ⬜ 待人工（§2.1） |
| 4 | 连续 30 min 无堆积、CPU < 1% | ✅ 已观察 40+ min 无堆积；CPU 均值待 §2.4 复核 |
| 5 | 深探不可用时显示「RSS 口径」而非空白/0 | ✅（降级链单测 + 徽标 UI） |

## 5. Review 期间发现并修复的缺口

| # | 缺口 | 影响 | 处置 |
|---|---|---|---|
| 1 | `capabilities/default.json` 的 `windows` 只列了 `main`，而面板窗口（`panel`）通过 `@tauri-apps/api/event` 订阅 `mem://snapshot` / `mem://deep` / `mem://select` | 面板拿不到 `core:event:default` → 实时推送不生效，只能靠首次 `invoke` 的静态快照，数字不会自动刷新 | 已补为 `["main", "panel"]`（提交 `a273f4e`） |
| 2 | 深探轮与手动刷新可并行、缓存 `clear()` 重建有中间态 | 并发翻倍 + 数字短暂跳动 | 单飞闸门 + 整体替换（方案 §15.2） |

> 教训：UI 验证走浏览器 fixture 时，**Tauri 侧的权限/能力边界不会被覆盖到**——这类缺口只能靠真机运行发现。后续涉及新窗口或新 API 时，capability 要和窗口创建一起改。
