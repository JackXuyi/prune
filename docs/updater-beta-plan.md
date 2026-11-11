# 内测通道自动升级（双通道 Updater）

> 目标：在现有 GitHub 源自动升级链路（`docs/updater-plan.md`）上增加「内测（beta）通道」——内测用户一键加入后，客户端自动拉取 prerelease 版本更新；正式通道用户完全无感。

## 一、结论先行

| 问题 | 结论 |
|---|---|
| 内测版怎么发 | tag 带 `-beta.N` 后缀（如 `v0.7.0-beta.1`），Release 标记 prerelease，仍走 Draft → 试装 → Publish 纪律 |
| 内测清单放哪 | 固定滚动 tag `beta` 的 prerelease Release，endpoint 恒为 `releases/download/beta/latest.json`（先发布版本 Release，再手动同步 latest.json 过去） |
| 通道怎么存 | `settings.json` 新键 `updater.channel`（`stable` / `beta`），复用 `crate::settings` |
| 客户端怎么查 | beta 通道同时查 beta + stable 两个 manifest，semver 取更高者——自动解决「卡在内测版」「正式版发布后 beta 用户收不到」两个边角 |
| 正式用户会不会误升级 | 不会。`releases/latest` 永不指向 prerelease（GitHub 规则），stable endpoint 链路零改动 |

## 二、关键事实（调研结论）

| # | 事实 | 来源 / 影响 |
|---|---|---|
| 1 | GitHub `releases/latest` 的「latest」**排除 draft 和 prerelease** | GitHub Docs：内测版发成 prerelease 后，stable 端点 `releases/latest/download/latest.json` 天然不受影响 |
| 2 | tauri-action 的 updater 清单**固定叫 `latest.json`**，无 `updaterJsonPrereleaseNaming` 之类的预发布命名选项 | 源码 `src/upload-version-json.ts:51`（v0 分支已核实，action.yml 全量 inputs 已核对） |
| 3 | latest.json 内的产物下载 URL 是 **tag-based**（`releases/download/<tag>/...`） | 源码 `upload-version-json.ts:196`：把 latest.json 复制到另一个 Release 不需要搬产物，URL 依然有效 |
| 4 | tauri-plugin-updater 2.x 支持 `updater_builder().endpoints(...)` 运行时覆盖 endpoint | 插件文档 UpdaterBuilder：通道切换不需要改 tauri.conf.json，Rust 侧按通道选 URL 即可 |
| 5 | updater 版本比较走 semver：`0.7.0-beta.1 < 0.7.0`，且 `0.7.0-beta.1 > 0.6.0` | semver 规范：内测版可以正常从正式版升级，正式版发布后也能正常覆盖内测版 |
| 6 | 现有 verify 的版本一致性校验是字符串全等比较，`v0.7.0-beta.1 == v0.7.0-beta.1` 天然通过 | `.github/workflows/release.yml:64`，无需改 |
| 7 | 现有 `pnpm bump` 从 prerelease 版本出发会算错（`'0-beta.1' → Number → NaN`） | `scripts/bump-version.mjs:28`，需要补 |

## 三、整体链路

```
【内测发版】
pnpm bump 0.7.0-beta.1 → commit → push tag v0.7.0-beta.1
  → release.yml（prerelease 自动判定）→ Draft prerelease Release
  → 本地试装确认 → Publish
  → 手动触发 sync-beta workflow（输入 tag）→ latest.json 同步到滚动 beta Release
  → 内测客户端（beta 通道）拉 beta manifest → semver 更高 → 面板卡片 → 下载安装重启

【正式发版】（与现状一致，多一步同步）
pnpm bump stable（0.7.0-beta.3 → 0.7.0）→ push tag v0.7.0
  → Draft Release → 试装 → Publish
  → stable 用户照常升级
  → 手动触发 sync-beta（可选）→ beta 用户同步收到 0.7.0 正式版
```

### 通道端点

| 通道 | endpoint | 谁能拉到 |
|---|---|---|
| stable（现状，零改动） | `releases/latest/download/latest.json` | 所有默认用户；latest 永远是最新**正式**版 |
| beta（新增） | `github.com/JackXuyi/prune/releases/download/beta/latest.json` | 仅设置里打开内测通道的用户 |

> 「滚动 beta Release」= 一个固定 tag 为 `beta` 的 prerelease Release，里面只有一份 latest.json（每次同步覆盖）。它只是清单的固定地址，安装包本体留在各版本的正式 Release 里（tag-based URL 直接可下，见事实 3）。

## 四、关键设计决策

| 决策 | 原因 |
|---|---|
| 双 manifest 而不是单 manifest 加通道字段 | updater 插件只认「manifest 里的 version 比 current 高就升级」。单 manifest 塞 beta 版会让**正式用户也收到内测**；semver 后缀虽然让 beta.1 < 正式 0.7.0，但 `0.7.0-beta.1 > 0.6.0`，0.6.0 的正式用户会被升到内测版——不可接受 |
| 滚动 tag `beta` 托管清单 | GitHub 没有「指向最新 prerelease」的静态 URL；tag-per-beta 的下载地址随版本变化，静态 endpoint 无法表达。滚动 tag 是唯一不依赖自建服务器的方案 |
| 同步 latest.json 用独立 workflow（手动触发） | 发版保持「Draft → 试装 → Publish」纪律；Publish 之前 latest.json 里的产物链接不可下载，自动同步会误导 beta 用户。独立 workflow 在 Publish 后手动跑，语义清晰 |
| beta 通道同时查 stable manifest，取 semver 更高者 | 解决两个边角：① 内测迭代停在 beta.3，正式 0.7.0 已发布但还没同步 → beta 用户也能升级；② 用户从内测切回正式通道，0.7.0 > 0.7.0-beta.3 → 能直接升到正式版，不会卡死在内测版 |
| 通道状态存 `settings.json` | 复用 M13 收敛后的 `crate::settings`（app_config_dir 统一读写），零新依赖；格式上 `updater.channel` 与现有键风格一致 |
| 不支持降级 | updater 只升不降。内测版出问题时回滚 = 手动下载旧安装包重装，写入 UI 文案预期 |

## 五、模块设计

### 5.1 Rust 侧（`src-tauri/src/updater.rs`）

```rust
/// 通道端点（stable 端点保持 tauri.conf.json 里的默认，beta 端点运行时覆盖）
const BETA_ENDPOINT: &str =
    "https://github.com/JackXuyi/prune/releases/download/beta/latest.json";

pub fn channel_get(app: &AppHandle) -> Channel;   // 读 settings.json，默认 stable
pub fn channel_set(app: &AppHandle, ch: Channel); // 写 settings.json "updater.channel"
```

`check_and_notify` 改造：

1. 读当前通道。
2. **stable 通道**：走现有 `app.updater().check()`，逻辑不动。
3. **beta 通道**：`updater_builder().endpoints(vec![BETA_ENDPOINT]).check()` 与默认 `updater().check()` 并发查两份 manifest，semver 比较取更高者；payload 增加 `channel` 字段（`"beta"` / `"stable"`，标记这次升级来自哪个通道）。
4. 事件契约不变（四个事件照旧），仅 `update://available` 负载多一个 `channel` 键——前端旧字段全部兼容。

新增命令：

| 命令 | 返回 | 用途 |
|---|---|---|
| `update_channel_get` | `{ "channel": "stable" \| "beta" }` | 设置页开关初始化 |
| `update_channel_set` | 同上 | 开关切换；写盘成功后立即按新通道重新检查一次并广播 |

`lib.rs` 注册两个新命令，capabilities 不用动（自定义命令不走插件权限门，同现有设计）。

### 5.2 前端（设置页 + 更新卡片）

| 文件 | 改动 |
|---|---|
| `src/lib/ipc.ts` | `updateChannelGet` / `updateChannelSet` 封装；`UpdateStatus` 的 available 分支加 `channel?: "beta" \| "stable"` |
| 设置页更新区（SettingsBoard 更新段落） | 「内测通道」开关：关 = 正式版；开 = 抢先体验未发布版本。切换后触发一次检查，beta 发现的更新在卡片上带 `BETA` 小徽标 |
| `src/components/UpdateCard.tsx` | pending 来自 beta 通道时版本号旁渲染徽标；其余逻辑不变 |
| `src/i18n/messages/namespaces/update.ts` + settings 命名空间 | 新键（zh/en 两份，过 `pnpm check:i18n` 门禁）：通道开关文案、「内测通道已开启，检查更新中」「回滚需手动下载安装包」提示 |

### 5.3 发版脚本与 CI

| 文件 | 改动 |
|---|---|
| `scripts/bump-version.mjs` | ① 新增 `pnpm bump 0.7.0-beta.1`（现有前缀正则已放行，显式验证）；② 新增 `pnpm bump beta`：已有 `-beta.N` 后缀 → N+1，无后缀 → 报错提示先指定完整版本；③ 新增 `pnpm bump stable`：去掉后缀；④ 修复 `patch/minor/major` 从 prerelease 版本出发的 NaN（先剥后缀再算，结果**不带**后缀） |
| `.github/workflows/release.yml` | `prerelease: ${{ contains(github.ref_name, '-') }}`——tag 含 `-` 自动标记 prerelease，正式 tag 行为不变；verify 与签名/公证逻辑零改动 |
| `.github/workflows/sync-beta.yml`（新增） | `workflow_dispatch` + 输入 `tag`：① 校验该 tag Release 已 Publish（draft 直接失败）；② `gh release download <tag> --pattern latest.json`；③ `gh release upload beta --clobber latest.json`；④ 输出同步结果与 manifest 版本号。一次性前置：仓库里预创建 tag `beta` 的空 prerelease Release |

### 5.4 边界与不做

| 不做 | 原因 |
|---|---|
| Gitee 链路内测分发 | Gitee 无 updater，内测用户走 GitHub 通道即可；`pnpm release` 推 Gitee 的 tag 会带 beta tag，Gitee Release 页多一条 prerelease 无副作用 |
| 自动把正式版同步进 beta 通道 | 保持手动 sync-beta，与「试装后 Publish」纪律对齐；未同步时 beta 用户靠双 manifest 比对兜底也能收到正式版 |
| 降级 / 回滚按钮 | updater 只升不降；回滚 = 手动下载安装包 |

## 六、风险

| 风险 | 等级 | 缓解 |
|---|---|---|
| `beta` Release 被误删 → beta 通道 404 | 中 | 一次性配置后不再动它；sync-beta workflow 第一步探测其存在，缺失时明确报错并给出重建命令 |
| 内测版质量问题扩散 | 低 | 内测通道默认关闭，需用户显式打开；beta 版与正式版同签名链路，minisign 校验不放松 |
| 双 manifest 并发查询拖慢检查 | 低 | 两个请求各 ~几十 KB，tokio join 并发；任一失败不阻塞另一个 |
| 私钥泄漏面扩大？ | 无 | 通道机制不新增签名材料，复用同一把 minisign 私钥 |

## 七、验证清单

1. **bump 脚本**：`pnpm bump 0.7.0-beta.1` → 三处版本一致；`pnpm bump beta` → `0.7.0-beta.2`；`pnpm bump stable` → `0.7.0`；从 beta 版 `pnpm bump patch` → `0.7.1`（不带后缀）；`cargo check` 过（Cargo.lock 同步）。
2. **本地链路**：`pnpm tauri dev` → 设置页开内测通道 → 「检查更新」→ beta Release 无清单时报错信息明确；关闭通道 → 恢复「已是最新」。
3. **真实升级（beta → beta）**：装 v0.6.0 → `bump 0.7.0-beta.1` → push tag → CI 出包 → Publish → 触发 sync-beta → v0.6.0 开内测通道 → 卡片出现带 BETA 徽标 → 更新重启为 `0.7.0-beta.1`。
4. **隔离性**：另一台装 v0.6.0 且不开内测通道的机器 → 「检查更新」→ 仍是「已是最新」（ prerelease 不进 latest）。
5. **正式覆盖**：`bump stable` → 发 v0.7.0 → Publish → v0.7.0-beta.1 的机器（无论开关状态）都能升到 0.7.0。

## 八、待确认问题

| # | 问题 | 建议 |
|---|---|---|
| 1 | 内测版是否也走 Draft 试装纪律？ | 建议**保持**（你自己就是第一个内测用户）；确认后 sync-beta 的手动触发时机 = Publish 之后 |
| 2 | 通道开关放哪？ | 建议设置页更新区（与「检查更新」同屏）；如需面板 footer 常驻入口再加 |
| 3 | beta 版本号从哪个基线起步？ | 建议 `pnpm bump 0.7.0-beta.1`（下一个 minor 的第一个内测），CHANGELOG 同步建 0.7.0 段 |

## 九、实施排期（T1–T6）

| 阶段 | 内容 | 门槛 |
|---|---|---|
| T1 | `bump-version.mjs` 支持 beta 后缀 + `cargo check` 验证 | bump 四种用法全部正确 |
| T2 | Rust 侧：通道读写 + 双 manifest 检查 + 两个新命令 | `cargo check` / 单测过，事件负载带 channel |
| T3 | 前端：ipc 封装 + 设置页开关 + UpdateCard 徽标 + i18n 两份文案 | `pnpm build`（tsc + i18n 门禁）0 错误 |
| T4 | CI：release.yml prerelease 动态判定 + sync-beta.yml + 预建 beta Release | workflow lint / dry-run 通过 |
| T5 | 按第七节 1–3 项走一轮真实 beta 发版 | v0.6.0 → beta.1 真实升级成功 |
| T6 | 验证清单 4–5 项（隔离性 + 正式覆盖）+ 文档收尾（updater-plan.md 补一节「内测通道」） | 全部通过后合入 |

> 文档落点：本文为实施依据；完成后在 `docs/updater-plan.md` 末尾追加「内测通道」小节，`docs/release-github.md` 的触发方式表格补 prerelease 一行。
