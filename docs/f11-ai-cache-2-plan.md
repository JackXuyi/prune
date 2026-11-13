# M27 残余 · AI 缓存白名单二期（F11）方案

> 背景：M27 主体（Cursor 6 / Claude 3 / Ollama 2 + Xcode 补扫 2）已于 2026-11-11 落地（commit
> `210ef18`）并随 v0.8.3 发布，roadmap 文档滞后。本方案补齐对标 DevCleaner 1.16 覆盖面的
> 残余差距：**OpenAI 家族（ChatGPT / Atlas / Codex）、Codex CLI、HuggingFace、LM Studio、
> Windows 侧 AI 条目**。
> 数据来源：本机实测（2026-11-13，`du -sh` 逐目录）+ DevCleaner 字段指南口径对齐；
> 路径错误的后果是「扫不出」（`resolve` 按 `is_dir` 跳过），不会误删——但登记前仍逐个
> 实测内容性质，凡语义不明的一律不登记。

---

## 1. 本机实测（2026-11-13）

| 路径 | 体积 | 内容性质 | 判定 |
|---|---|---|---|
| `Library/Caches/com.openai.chat` | 128 MB | Cache.db + WebKit HTTP 缓存 | ✅ 登记 L2 |
| `Library/Caches/com.openai.atlas` | 459 MB | **458 MB 是 `browser-data/`（Chromium profile：`host/Default`、`login-staging`）** | ❌ **不登记**：含站点登录态，整目录删除=登出所有网站 |
| `Library/Caches/com.openai.codex` | 2.3 MB | Cache.db + Sparkle | ✅ 登记 L2 |
| `Library/Caches/Codex` | 744 KB | `Default/Cache/Cache_Data` 纯 HTTP 缓存（Cookies 在 AS/Codex，已核实） | ✅ 登记 L2 |
| `Library/Application Support/Codex/Cache` | 23 MB | Chromium HTTP 缓存子目录 | ✅ 登记 L2（只登记 Cache 子目录；AS/Codex 本体含 Cookies / Local Storage / Session Storage，红线） |
| `Library/Caches/cursor-compile-cache` | 24 MB | Cursor CLI node 编译缓存（按 node 版本分桶） | ✅ 登记 L2 |
| `~/.codex/cache` | 12 MB | codex_app_directory / apps_server_info / tools / remote_plugin_catalog，全目录缓存 | ✅ 登记 L2（Os::All，home 相对两端通用） |
| `~/.codex/sessions` `history.jsonl` `config.toml` `auth.json` `memories_1.sqlite` `logs_2.sqlite`(72 MB 文件) | — | 会话 / 凭据 / 配置 / 日志库（文件非目录，现机制只收目录） | ❌ 永不登记 |
| `~/.cursor/extensions` | 2.8 GB | 已装扩展本体。可再生但删除即丢扩展，破坏工作环境 | ❌ 不登记（注释说明） |
| `~/.cursor/projects` `ai-tracking` | 303 / 96 MB | 项目状态 / 行为追踪 | ❌ 不登记 |
| `~/.cache/huggingface/hub`（+xet） | 176 MB | 模型权重，可重下代价高 | ✅ 登记 L3（Os::All） |
| `~/.cache/lm-studio/models` | 本机未装 | LM Studio 官方默认模型目录 | ✅ 登记 L3（Os::All；未装时 resolve 自动跳过，不编造体积） |

## 2. 新增登记 14 条

| key | rel | level | os |
|---|---|---|---|
| `chatgpt-caches` | `Library/Caches/com.openai.chat` | L2 | Mac |
| `codex-app-caches` | `Library/Caches/com.openai.codex` | L2 | Mac |
| `codex-legacy-caches` | `Library/Caches/Codex` | L2 | Mac |
| `codex-app-cache` | `Library/Application Support/Codex/Cache` | L2 | Mac |
| `cursor-compile-cache` | `Library/Caches/cursor-compile-cache` | L2 | Mac |
| `codex-cli-cache` | `.codex/cache` | L2 | All |
| `huggingface-hub` | `.cache/huggingface/hub` | L3 | All |
| `lmstudio-models` | `.cache/lm-studio/models` | L3 | All |
| `win-cursor-cache` | `AppData/Roaming/Cursor/Cache` | L2 | Win |
| `win-cursor-cacheddata` | `AppData/Roaming/Cursor/CachedData` | L2 | Win |
| `win-cursor-gpu` | `AppData/Roaming/Cursor/GPUCache` | L2 | Win |
| `win-cursor-logs` | `AppData/Roaming/Cursor/logs` | L2 | Win |
| `win-claude-cache` | `AppData/Roaming/Claude/Cache` | L2 | Win |
| `win-claude-gpu` | `AppData/Roaming/Claude/GPUCache` | L2 | Win |

- Windows 侧路径依据 Electron/VS Code 系通用布局（VS Code 在 Windows 的缓存同样落
  `%APPDATA%\<app>\Cache` 系）+ DevCleaner 字段指南，**真机待验**；路径偏差的后果是扫不出，
  无误删风险（`is_whitelisted` 对不存在的 root 恒 false）。
- Claude Desktop Windows 的 GPUCache 与 Cache 同布局；`%APPDATA%\Claude` 本体（对话数据）
  不登记，与 macOS 同口径。

## 3. 红线清单（测试固化）

1. AI 系条目（key 前缀 `cursor-` / `claude-` / `codex-` / `chatgpt-` / `win-cursor-` /
   `win-claude-`，及 `huggingface-` / `lmstudio-`）的 rel **禁止**包含：
   `sessions` / `history` / `auth` / `Cookies` / `browser-data` / `conversations` /
   `drafts` / `workspaceStorage` / `state.vscdb` / `projects` / `ai-tracking` /
   `Local Storage`
2. **Atlas 整目录永不登记**（测试断言：不存在 rel 含 `com.openai.atlas` 的条目）——
   browser-data 与 HTTP 缓存混在同一目录，无法只删缓存部分（目录粒度机制做不到），宁可错过。
3. `Library/Application Support/Codex` 根目录永不登记（只允许 `/Cache` 子目录）。
4. 模型权重类（huggingface-hub / lmstudio-models / ollama-models）必须 L3——可重下
   但代价高，逐项确认；与 L2 缓存类分离。

## 4. 不做（本期明确排除）

- `~/.codex/logs_2.sqlite`（72 MB）：文件而非目录，白名单机制只收目录；且删日志主库
  文件对 Codex 运行的影响未实测，不赌。若未来支持文件级条目再评估。
- LM Studio 日志 / `.internal`：内部结构无官方文档，不编造路径。
- ChatGPT / Atlas 的 Windows 路径：无实测依据，不登记。
- Atlas：见红线 2。

## 5. 交付物与验收

1. `whitelist.rs`：14 条登记 + 头注释来源更新 + §3 红线测试 + 二期条目存在性/级别测试
2. `src/i18n/messages/namespaces/disk.ts`：zh / en 各 14 个 `disk.cache.<key>`
3. CHANGELOG 0.8.5 / README 路线图 M27 行补「二期」/ roadmap 文档状态修正
4. bump 0.8.5
5. 门禁：cargo test / clippy -D warnings / fmt / tsc / pnpm build（含 check-i18n 对齐校验）
