# D-15 `--emit-script` 方案（DP-4 裁 A）

日期：2026-11-18 ｜ 决策：DP-4 由用户拍板 **A（开）**，推翻路线图 §9 的保守建议 B。
本方案同时是 A 选项的解释成本对冲：把「Agent 能看、人才能删」的边界写死在代码与测试里。

## 1. 背景

- 护城河口径：Prune 的 MCP/CLI 全只读，删除动作只能由人触发（`reclaim --yes`）。
- Agent 协作场景的真实需求：Agent 想替人准备删除动作，但工具不允许它执行。
  现状是 Agent 只能复述预览清单，人还要手工翻译成命令——缝隙在这。
- `--emit-script` 开的缝：**只打印**一段可审计的 shell 脚本，执行权完全留在人手里。
  Agent 可以拿文本、可以转交，但不能让 Prune 替它删。

## 2. 设计决策

| # | 决策 | 理由 |
|---|------|------|
| D1 | 挂在 `prune reclaim --emit-script`，与 `--yes` 互斥（同给报用法错退出 2） | 候选删除命令就是 reclaim 预览的命令化；互斥防「既打印又执行」的歧义 |
| D2 | 范围 = reclaim v1 同款：只取 L1 死重白名单项（`preview(&["yarn"])`） | L3 需逐项人工判断，L4 永不提供删除命令（docker_hints 已是先例） |
| D3 | 每条命令是 `mv -n '<path>' "$HOME/.Trash/"`，不用 `rm` | 与产品「只用废纸篓」护栏同源；`-n` 不覆盖防同名吞条目；可恢复 |
| D4 | 逐项**复验**静态白名单：`whitelist::is_whitelisted` 不过的项不进脚本、stderr 告警 | 「附白名单校验结果」不是贴标签，是真校验；报告过期/白名单变更时宁可少给 |
| D5 | 路径 shell 单引号转义（`'` → `'\''`），POSIX sh + `set -eu` | 路径来自磁盘扫描，空格/引号/UTF-8 都可能；脚本要能直接过 `sh -n` |
| D6 | 生成逻辑独立 `emit_script.rs`，纯字符串函数，零 IO 零进程 | 验收口径「单测断言无任何执行路径」落在模块源码断言上 |
| D7 | 脚本头注明生成时间、版本、来源白名单与「Prune 不执行本脚本」声明 | 脚本单独传播（贴给 Agent / 存文件）时仍自证来源与边界 |

## 3. 代码落点

- `crates/prune-cli/src/emit_script.rs`（新增）：`shell_quote` / `script_text(pv, home, now)` / `run()`（stdout 输出）；白名单复验在 run 中做，script_text 保持纯函数（测试不需要真实文件系统）。
- `crates/prune-cli/src/main.rs`：`Command::Reclaim { yes, emit_script }`、parse `--emit-script`、usage 行、`cmd_reclaim` 分发（emit 分支提前 return，结构上不接触 `execute`）。
- `docs/cli-reference.md`：reclaim 段补 `--emit-script`。
- `CHANGELOG.md`：Unreleased 记 D-15。

## 4. 边界与风险

- 脚本执行后 Prune 无感知（不记账本不回写状态）——定位是「交接物」不是清理通道，GUI/CLI 的账本以重新扫描为准。
- `mv -n` 在 GNU/BSD 语义一致（no-clobber），但跳过时静默：脚本注释声明「同名冲突会保留废纸篓旧条目，请人工核对」。
- L2 整库清理不进脚本（v1 CLI 本来就不开放）。
- 解释成本对冲：README/docs 明确「emit-script 输出的脚本由人审阅执行，Prune 进程内不存在执行该脚本的代码路径」。

## 5. 验收清单

- [x] A1 `prune reclaim --emit-script` 输出含 `set -eu`、每项一条 `mv -n` 与体积/校验注释
- [x] A2 `--emit-script` 与 `--yes` 同给退出 2
- [x] A3 含空格/单引号的路径在脚本中安全转义（`sh -n` 语法通过由转义函数保证）
- [x] A4 白名单复验不过的项不进脚本且有 stderr 告警
- [x] A5 单测断言 `emit_script.rs` 源码不含 `Command` / `trash::` / `execute(` / `process::`（无执行路径）
- [x] A6 空清单时输出说明性脚本头、不输出任何 mv 行，退出 0
- [x] A7 cargo fmt / clippy / 全仓测试绿
