# JetBrains 配方：把 Prune 读数接进 External Tools

> W2/D-12 编辑器配方包。External Tools 是 IDE 侧配置，Prune 不写任何文件——
> 下面是**你手工填**的三条配置，参数与 VS Code 配方口径完全统一。
> 命令与本版 CLI usage 逐字一致（由 `pnpm check:docs` 门禁检查 C 自动对账）。

## 前提：CLI 可用

```sh
cargo build --release -p prune-cli
ln -sf "$PWD/target/release/prune" ~/.local/bin/prune   # 确保 ~/.local/bin 在 PATH 里
prune --version
```

## External Tools 三条配置

打开 `Settings → Tools → External Tools → +`，依次添加：

### 1. 端口冲突

| 字段 | 值 |
|---|---|
| Name | `prune: 端口冲突` |
| Program | `prune` |
| Arguments | `port --conflicts --json` |
| Working directory | `$ProjectFileDir$` |

### 2. 项目依赖账本

| 字段 | 值 |
|---|---|
| Name | `prune: 项目依赖账本` |
| Program | `prune` |
| Arguments | `projects --json` |
| Working directory | `$ProjectFileDir$` |

### 3. 一轮读数

| 字段 | 值 |
|---|---|
| Name | `prune: 一轮读数` |
| Program | `prune` |
| Arguments | `watch --once --json` |
| Working directory | `$ProjectFileDir$` |

> 与方案 §9.2 的对应关系（一处偏差，已记录）：方案里的 `doctor --json`
> 因 `prune doctor` 尚未实现，用 `watch --once --json`（一轮拉全
> cpu/mem/port/disk 读数）替代。`doctor` 留给后续批次。

跑法：`Tools → External Tools` 菜单，或 `⇧⌘A`（Find Action）搜任务名。
输出进 Run 面板。

## 常见问题

- **Program 报找不到**：macOS 下 IDE 启动的进程 PATH 可能不含 `~/.local/bin`。
  把 Program 填成完整可执行路径（`echo ~/.local/bin/prune` 的展开结果）即可；
  注意这是你机器上的路径，不是配方的一部分。
- **想让 warn 当失败**：把「一轮读数」的 Arguments 改成
  `watch --once --json --fail-on warn`（退出码 10）。

## 退出码约定

| 码 | 含义 |
|---|---|
| 0 | 正常 |
| 2 | 用法错（参数拼错） |
| 3 | 权限不足 |
| 4 | 探测失败 |
| 10 | `watch` 达到 warn（仅 `--fail-on warn` 时出现） |
