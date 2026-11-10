# Prune 项目 Review 报告（2026-11-10）

> 审查对象：`/Users/xuyi/Desktop/git/prune`，master @ 877be7e（tag v0.5.0）
> 规模：Rust 21,271 行（src-tauri + crates/*）、TS/TSX 13,170 行、受跟踪文件 281 个
> 方法：静态通读 + 定向取证 + 实跑门禁（未改动任何代码）

## 实测门禁结果（本机实跑，非推断）

| 项目 | 命令 | 结果 |
|---|---|---|
| Rust 测试 | `cargo test --workspace` | **280 passed / 0 failed / 2 ignored**（42 + 18 + 220） |
| Clippy | `cargo clippy --workspace --all-targets` | 0 error / 35 warning |
| 类型检查 | `tsc --noEmit` | exit 0，零报错 |
| 格式检查 | `cargo fmt --all -- --check` | **232 处差异**（整体失败） |
| 前端构建产物 | `dist/assets/index-*.js` | 428 KB，`devFixture` 已被 tree-shake |

## 结论摘要

| 级别 | 数量 | 说明 |
|---|---|---|
| **P0 blocker** | **0** | 未发现可绕过安全护栏的删除路径，无阻断发布的功能缺陷 |
| **P1 important** | 8 | CI 不跑测试、dev 分支伪造清理成功、CSP 关闭、版本漂移、release 脚本风险、14 处未处理 rejection、文档与实现矛盾、无 LICENSE |
| **P2 nice-to-have** | 30+ | 超大文件、死代码、重复实现、格式/lint 缺失、i18n 半套 |

**总判断**：安全护栏（产品第一卖点）是**真的写在代码层、且不可被上层绕过**的，不是 README 话术。最薄弱的环节是**工程流程**（CI 不跑任何测试、零前端测试、无 lint/format 门禁）——「代码可信度高，但保障它一直可信的机制不存在」。

---

## 一、安全护栏核验（最关键一节）

逐条核验 README:93-98 六条声明，**全部真实存在且有代码层强制**：

| # | 声明 | 代码位置 | 结论 |
|---|---|---|---|
| 1 | 路径白名单 | `crates/prune-core/src/disk/whitelist.rs:416-426` | ✅ 稳固 |
| 2 | 废纸篓而非 rm | `reclaim/mod.rs:344`（全仓唯一删除路径） | ✅ |
| 3 | Dry-run 前置 | `reclaim/mod.rs:321-323` | ⚠️ L1/L2 仅 UI 约定，非代码强制（见 §1.3） |
| 4 | 20 GB 熔断 | `reclaim/mod.rs:21, 332-336` | ✅ 执行时重算、不可配置、CLI `--yes` 也拦 |
| 5 | 永不提权 | 无 setuid / `footprint` 不带 `-a` / capabilities 仅 `core:default` | ✅ |
| 6 | Docker 永不执行 | `mcp.rs::docker_hints()` 仅返回字符串，全仓无 docker 子进程 | ✅ |

### 1.1 白名单亮点

- 37 条登记项全部是**相对 home 的相对路径**；`canonicalize` 消掉 `..` 与 symlink ⇒ 链接指向 home 外、`~/Caches/../../../etc` 均被拒；`starts_with` 按 Path 组件比较，无 `/Users/x` 误匹配 `/Users/xy` 问题。
- `is_report_detail_path()`（`reclaim/mod.rs:127-145`）三重条件：canonicalize 落在 home 内 + `file_name()` 恰为 `node_modules` + 精确命中 allowed 表。**「即便授权表被伪造，能删的也只有叫 node_modules 的目录」是本仓最漂亮的一处防御设计。**

### 1.2 熔断亮点

- **不信报告、执行前用 `tree_size` 重算**——报告可被魔改/过期，体积不能。
- `grep "breaker"` 只命中 `reclaim/mod.rs`，**无任何 settings/环境变量入口可放宽阈值**。
- CLI `--yes` + 200 GB → 捕获 `NeedsExplicitConfirm` 打印「即使 --yes 也拒绝执行」，退出码 1。全仓最值得表扬的「防自己人手滑」设计。
- 所有 `Mutex::lock()` 写成 `unwrap_or_else(|e| e.into_inner())`，中毒后恢复而非 panic。

### 1.3 缺口（均为 P2）

1. **L1/L2 不要求 confirmed**：`reclaim_execute` 可不经 `reclaim_preview` 直接调用完成删除。「先看清单再删」目前是 UI 自然流程而非代码不变量。建议 preview 发一次性 nonce 或 `ReclaimItem` 带 `previewedAt`。
2. **熔断边界是 `>` 而非 `>=`**：恰好 20 GiB 不触发。
3. **`preview()` 用常量而非 `limits.breaker_bytes`**（`reclaim/mod.rs:268`），与 execute 参数化不一致，未来的地雷。
4. **`kill_process` 无系统进程保护名单**：只拦 `pid<=1` 与自身，用户可杀 Finder。建议加 `["Finder","Dock","SystemUIServer","loginwindow"]` 拒绝表。
5. `reveal_path` / `clipboard_write` IPC 契约过宽（设计上可接受，记录在案）。

### 1.4 绕过面推演（全部被拒）

| 攻击 | 结果 |
|---|---|
| `/etc/hosts` | `NotWhitelisted`，文件未动（有验收记录 + 测试覆盖） |
| home 本体 | 拒绝（`execute_rejects_home_itself`） |
| symlink → /etc | canonicalize 后拒绝 |
| 伪造 allowed 表塞重要文件 | `file_name() != "node_modules"` 拒绝 |
| L4 / `--yes`+200GB / 篡改报告 bytes | 分别被 NotCleanable / 熔断 / 执行时重算拦截 |
| TOCTOU | 不存在路径返回 false；trash 对 symlink 本身操作；残余窗口极小 |

---

## 二、P1 问题明细（8 项）

### P1-1 CI 不跑任何测试（全仓最大工程缺口）

`.github/workflows/release.yml`（唯一 workflow）verify job 只做版本一致 + 签名 preflight + `pnpm build`；`.workflow/prune-ci.yml` 同样只 `pnpm build`。**283 个测试只在本地手动 `cargo test` 时执行**——任何人 push 破坏护栏的改动，CI 全绿。修复：verify job 加 `cargo test --workspace` / `cargo clippy -- -D warnings` / `cargo fmt --check` 三步。

### P1-2 dev 分支伪造清理成功

```ts
// src/stores/diskStore.ts:107-113
execute: async (confirmed) => {
  ...
  if (!isTauri()) {
    set({ lastReclaimed: {...}, preview: null });
    return;   // ← 什么都没发生，但 UI 显示「已清理 N 项 / X GB」
  }
```

对比同文件 `loadPreview`（89-91 行）有 `import.meta.env.DEV` 判断，**execute 少了**。`pnpm build` 产物在浏览器里打开，点清理显示成功实际没删。对一个以「不撒谎」为设计原则（三态语义、读不到≠0）的产品，这是最不该出现的偏差。改一行。

### P1-3 CSP 完全关闭

`tauri.conf.json:41` `"csp": null`。缓解因素：全仓 `dangerouslySetInnerHTML`/`eval`/`new Function`/`localStorage` 均为 0 处，当前无可利用注入点。但应用大量渲染系统字符串（进程名/路径/错误信息），一旦依赖投毒无第二道防线。建议：

```
default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: asset: http://asset.localhost; connect-src ipc: http://ipc.localhost
```

### P1-4 版本漂移

`package.json` / `tauri.conf.json` / `prune-app` = 0.5.0，但 workspace 与 crates 停在 0.1.0，且被测试写死（`prune-core/src/lib.rs:59` `assert_eq!(version(), "0.1.0")`）。连带 MCP `initialize` 响应的 `serverInfo.version` = 0.1.0。`bump-version.mjs` 不碰 workspace，漂移不会自愈。修法：crates 用 `version.workspace = true`，测试断言改 `env!("CARGO_PKG_VERSION")`。

### P1-5 release.mjs 吞提交失败

`git add -A && git commit ... || true` —— 提交因 hook/冲突失败时，后续 `git tag -f` 仍打在不含本次改动的 commit 上并推送。另外 `git tag -f` 强制覆盖已有 tag，与 updater 判断依据冲突，建议 tag 已存在即报错退出。

### P1-6 Token 泄漏面

`GITEE_TOKEN` 同时出现在 URL query string（`?access_token=...`）与 FormData body。query string 会进代理日志/ps 输出/Gitee 访问日志。表单里带就够。

### P1-7 14 处未处理 Promise rejection

`src/lib/ipc.ts` 无统一 catch/超时；`Island.tsx`（6 处）与 `MenuBarPanel.tsx`（8 处）的 `void invoke(...)` 只丢返回值不捕获 rejection，WKWebView 里静默丢失，用户表现为「点了没反应」。store 层错误处理反而是好的。修法：加 `.catch(() => undefined)` 或给 ipc.ts 加 `fireAndForget()` 包装。

### P1-8 文档与实现矛盾（对外诚信）

| 位置 | 文档说 | 代码是 |
|---|---|---|
| `README.md:13` | 徽章「状态-M0 验证中」 | 已发 v0.5.0，M17 完成 |
| `README.md:105` | `ignore` + `rayon` 并行 | 自建线程池 + `std::thread::scope`（lib.rs:7-8 明确写了不用 rayon 的原因） |
| `README.md:148` | M15 未完成 `[ ]`；`island.*` 8 个键 | 已发布（CHANGELOG 0.4.0）；实际 7 个键 |
| `prune-feature-spec-v2.md` §8 | SwiftUI + MenuBarExtra / Sparkle / ArgumentParser | Tauri 2 + 自研 updater + 手写 arg 解析（迁移前残留） |
| `prune-feature-spec-v2.md:275` | 「零埋点零遥测、**无网络请求**」 | M17 主动连 `1.1.1.1:443`/`223.5.5.5:443`/`www.apple.com:443` + `networkQuality`。**隐私声明是法律级文案，必须改** |
| `prune-tauri-technical-plan.md` 结论 5 | WebView 即用即毁 | m2-memory-plan.md D-M5 明写「本期不做」，三窗口常驻 |

另外：CHANGELOG「未发布」段下的 M17 功能实际已随 0.5.0 发布；0.5.0 条目混入 README 路线图不存在的 M21/M22 编号。

### P1-9 无 LICENSE 文件

仓库公开、有 Release 产物、有 updater 分发——「无许可证」在法律上等于「保留所有权利」。README 写了 MIT（暂定）但没有 LICENSE 文件。公开发布前硬缺口。

---

## 三、P2 问题清单

### 3.1 架构与超大文件（14 个 Rust + 5 个前端 > 500 行）

优先拆两个：`src-tauri/src/net_sampler.rs`（1205 行，采集+环缓冲+SQLite+解析四职责）与 `src/windows/MainWindow.tsx`（1199 行，6 tab 路由 + 9 个 useEffect）。其余：`prune-cli/main.rs` 1068、`disk/mod.rs` 1059、`NetBoard.tsx` 987、`sampler.rs` 981、`net/diag.rs` 966、`tray.rs` 949、`AppResourceGrid.tsx` 887、`island.rs` 869、`ReclaimBoard.tsx` 795 等。

### 3.2 耦合与双源真相

- 报告派生白名单的拼装写在壳层 `commands.rs:342-350`，CLI 只能传 `&[]`——两个入口的安全能力差异应显式建模而非靠传空数组。
- 岛几何常量双源：`island_geo.rs` 与 `src/panels/Island.tsx:61-66` 各存一份靠注释同步，迟早漂移。
- 采样时钟散落 6 个文件（10s/1s/2s/30s/60s/300s…），无统一 tick 注册表。

### 3.3 重复逻辑

- **两份 `home_dir()` 且平台优先级相反**（全仓唯一真正会出错的重复）：`paths.rs:15-27`（USERPROFILE 优先，注释解释了为何不能信 HOME——MSYS 会改写）vs `net/host.rs:71-76`（HOME 优先）。Windows 上「项目归属」与「清理白名单」可能用不同 home，`sanitize()` 拒绝判断也用错前缀。
- `splitSpeed()` 两份相同实现（`statsReadout.ts:311` vs `NetBoard.tsx:977`）+ `compactSpeed()` 第三种格式化 + `formatBytes` 三处各自可达。`statsReadout.ts` 已是共享读数层，组件却另抄一份。

### 3.4 测试缺口

- 零集成测试（无 `tests/` 目录），无「Tauri command → core → trash」端到端链路测试；IPC 层 `allowed` 拼装无覆盖。
- **零前端测试**：无 vitest/jest，`statsReadout.ts`（340 行纯函数、三态语义是产品红线）是最该有单测的文件。
- 2 个恒真断言（`island_geo.rs:191-192`，clippy 已报）——写了等于没写，制造「已覆盖」错觉。
- 正面：283 个 `#[test]`/35 个模块，测试比约 1:4，护栏负面测试组（reclaim/mod.rs:544-760）价值极高。

### 3.5 错误处理

- Rust 错误退化为字符串，前端无法区分 `NotWhitelisted`/`NeedsExplicitConfirm`/`NothingToDo`；建议 `{ code, message, detail }` 结构。当前「熔断二次确认」依赖 preview 恰好能工作，但 preview 后体积增长的场景会退化成展示原始字符串。
- 10 处 unwrap/expect，无一处用户输入触发，无 panic!。唯一建议修：`disk/mod.rs:734` 扫描线程 `join().expect(...)`（panic 会导致前端「报告不更新」无提示）。
- 日志 `eprintln!` 无级别，生产无法收集（本地工具可接受）。

### 3.6 前端质量

- tsconfig strict 全开 + `noUnusedLocals`，但缺 `noUncheckedIndexedAccess`（下一轮）。
- `any` 0 处、`@ts-ignore` 0 处；`ReclaimBoard.tsx:689` 的 `!` 断言建议删掉依赖第 697 行的收窄。
- **`AppResourceGrid.tsx:227-230` 每采样拍为死组件 `ResourceDetail`（零 import）做无谓 push**，内存里维持 60 点 × N 家族——修死代码即消除。
- 模块级 `initStarted` flag 永不复位：首次 init 失败后永远停在 error 态，只能刷新窗口；建议改 store 内部 `status` 状态允许重试。
- `memStore.init` 的 3 个 `listen` 未存 unlisten（模块单例下实际不泄漏，但与 diskStore 模式不一致）。
- `React.memo` 全仓 0 处；`NetBoard.tsx` 每 1s 整棵重渲染，1s 节拍可接受但 AreaChart 点数组每次重建需留意。
- 19 处 `as` 断言多为 `as const`/收窄，无 `as unknown as`。
- 正面标杆：`CountUp.tsx` 尊重 `prefers-reduced-motion`；`useAppHistory` 有 visibilityState 门控 + 卸载清理。

### 3.7 类型契约风险

`src/types/net.ts`（492 行）等是 Rust serde 结构的手工镜像，无 ts-rs/specta 生成校验——字段改名时编译器不报错，这类项目最常见的静默 bug 来源。

### 3.8 死代码（已逐个 grep 验证，合计约 1500 行，占前端 11%）

| 位置 | 行数 | 证据 |
|---|---|---|
| `AppMemoryList.tsx` | 385 | 无任何 import |
| `AppResourceCard.tsx` | 211 | 仅被 AppMemoryList 引用，整条链死 |
| `ThresholdBar.tsx` | — | 全仓无引用 |
| `AppResourceGrid.tsx::ResourceDetail` | ~160 | export 但零 import |
| `app_info` 命令 | — | 占位、前端零调用、且暴露错误版本号 0.1.0 |
| `island_reposition` 命令 | — | 前端零调用（Rust 内部另有路径） |

### 3.9 依赖卫生

- `objc2-quartz-core`（CALayer/CAMediaTiming features）全仓零引用，可删。
- `sharp`（原生模块）只用于一次性生成静态资源，拖慢每次 CI install；可移 optionalDependencies 或提交生成产物。
- 前端运行时依赖仅 4 个，极简，显著优点。

### 3.10 代码卫生

- 无 rustfmt.toml / .editorconfig / prettierrc / eslint 配置；232 处 fmt 差异（含两语句同行的硬证据）。**「要么统一一次并加 CI，要么永远放弃」**。
- 4 处 `eslint-disable` 指向不存在的 ESLint——纯装饰，误导。
- TODO/FIXME/HACK 全仓 0 处：没有沉积补丁，但已知妥协只存在作者脑中，单人维护有遗忘风险。
- MCP 加固点：stdio 无单行长度上限；`initialize` 应回自己支持的 protocolVersion 而非回显。
- i18n 半套：`uiText.ts` 只有 4 处引用，要么删掉要么真收口。
- a11y 细节：Island 动画无 prefers-reduced-motion 分支（CountUp 有，标准不一）；右键菜单纯鼠标操作。

---

## 四、值得肯定的部分

1. **安全护栏是真护栏**（§一全表），「防自己人手滑」的熔断设计与「伪造表也只能删 node_modules」的三重防御是亮点。
2. **prune-core 纯逻辑零 Tauri 依赖**——同类 Tauri 项目很少做到，CLI/GUI/MCP 三形态共享一个 crate。
3. **Tauri capabilities 最小权限**：仅 `core:default`，无 shell/fs/opener/dialog 插件权限；子进程全部固定绝对路径+固定参数，无 `sh -c`、无用户输入拼接。
4. **护栏负面测试组**（杀 home/symlink 逃逸/伪造表/L4/熔断）比「正常路径能跑通」有价值得多。
5. **踩坑记录写进模块头**：`net/ports.rs`、`footprint` 的 `-p`/`-i` OR 关系、M14 的 1457 倍误差事故——把事故写进代码是正确做法。
6. **`statsReadout.ts` 三态建模**把「读不到≠0」这条产品红线固化成类型。
7. **MCP server 质量好于预期**：handle_message 纯函数可测、标准错误码、参数 clamp、5 工具全只读且有测试断言。
8. **验收文档有真实数据**（27 项白名单命中、15.6s 全扫、`execute(["/etc/hosts"])` 拒绝实录）。

---

## 五、修复路线（按性价比排序）

| 序 | 动作 | 成本 | 收益 |
|---|---|---|---|
| 1 | release.yml verify 加 `cargo test` / `clippy -D warnings` / `fmt --check` | 3 行 | 283 个测试真正生效 |
| 2 | `diskStore.ts:110` 加 `&& import.meta.env.DEV` | 1 行 | 消除伪造清理成功 |
| 3 | `tauri.conf.json` 写 CSP | 1 行 | 补第二道防线 |
| 4 | crates `version.workspace = true` + 断言改 `env!` | 10 行 | 修 MCP 版本号 |
| 5 | 删 4 个死代码文件 + `ResourceDetail` + `app_info` | 删 900 行 | 消除每拍无谓开销 |
| 6 | `net/host.rs:71` 改用 `paths::home_dir()` | 5 行 | 消除 Windows 语义分歧 |
| 7 | 14 处 `void invoke` 加 `.catch` | 14 行 | 消除静默失败 |
| 8 | README 四处 + spec §8 + spec:275 隐私段 + CHANGELOG 归位 | 文档 | 对外诚信 |
| 9 | 加 LICENSE（MIT） | 1 文件 | 发布法律前提 |
| 10 | `cargo fmt --all` 一次性统一 + rustfmt.toml | 1 次提交 | 止住格式漂移 |
| 11 | release.mjs：commit 失败即退出 / tag 已存在报错 / token 移出 query string | 小 | 发布安全 |
| 12 | `statsReadout.ts` 建前端测试基线（vitest） | 半天 | 产品红线有回归保障 |

## 总评

`prune-core` 的护栏设计与测试质量、`net_sampler` 的踩坑记录、`statsReadout` 的三态建模、MCP 的只读约束——四处体现出明显高于同规模个人项目的工程判断力。真正的短板不在「写代码」，而在「保障代码持续正确」的机械层：CI 门禁、前端测试、版本单一来源、文档同步。这四项不需要设计能力，只需要一次投入。

---

*本报告为工程 review 参考，具体修复优先级请结合发布计划确认。*
