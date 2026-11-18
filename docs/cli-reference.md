# CLI 参考（v0.13 · 契约 v1）

> 本文档是 `prune` 命令行的唯一参考。**中文单语**——与「CLI 输出中文单语，不做 i18n」的既有约定一致（决策 W1-DP-6），不做英文版。
>
> 对账纪律：本文的命令与 flag 由 `pnpm check:docs` 的检查 D 与 `main.rs` 的 `usage()` 自动双向对账，退出码表由检查 E 与 `ExitCode` 枚举对账——文档先于代码写或代码先于文档写都会红。

## 命令总表

| 命令 | 一句话 |
|---|---|
| `prune scan [--json]` | 全盘扫描，输出可回收清单 |
| `prune reclaim [--emit-script] [--yes]` | 预览零风险清理项（默认 dry-run；`--emit-script` 打印可审计脚本永不执行；`--yes` 才真删） |
| `prune mem [--swapped] [--json]` | 一次性内存读数（footprint 口径） |
| `prune mem --watch [--interval N]` | 【已弃用】改用 `prune watch --only mem`（2027-09 移除别名） |
| `prune watch [--interval N] [--only cpu,mem,port,disk] [--json] [--once] [--fail-on warn]` | 按需守护：前台进程，Ctrl-C 即停，不装后台 |
| `prune port [PORT] [--json] [--all] [--conflicts]` | 谁占着这个端口（`--conflicts` 只看冲突端口） |
| `prune why <PORT> [--json]` | 端口白话解释：一句话说清谁占着、为什么杀不掉、去哪关 |
| `prune projects [--days N] [--json]` | 项目依赖账本（按天趋势，默认 30 天） |
| `prune doctor [--json] [--only disk,mem,port,net] [--timeout N]` | 四域体检：磁盘/内存/端口/网络的结论+证据+下一步 |
| `prune app <家族名> [--json]` | 一个应用的资源画像：进程/内存/CPU/端口/连接/磁盘 |
| `prune lan [--scan] [--json]` | 局域网设备与暴露面（默认只读邻居表；`--scan` 全段探测） |
| `prune mcp` | 启动 MCP stdio server（AI Agent 查询入口） |
| `prune mcp install [--client claude\|cursor\|vscode\|all] [--write] [--json]` | 把 MCP server 写进客户端用户级配置（默认 dry-run） |
| `prune --version` / `prune --help` | 版本 / 帮助 |

## 退出码总表

| 码 | 语义 | 产生条件 |
|---|---|---|
| 0 | 结论已出 | 包括「读到部分数据 + 在 `coverage.note` 里说明差额」——部分不可见是数据事实，不是失败 |
| 2 | 用法错 | 参数解析失败、必填参数缺失、`--only` 未知域 |
| 3 | 权限不足 | 主输出为空且根因是权限：lsof 被拒（如 `port --json` 在受限终端返回空清单） |
| 4 | 探测失败 | 主输出为空且根因不是权限：超时、命令不存在、输出无法解析（如 watch 四频道全不可读） |
| 10 | watch 达 warn | 仅 `prune watch --fail-on warn` 时出现 |

## JSON 契约（v1）

1. 每个 `--json` 输出的顶层必含 `"schemaVersion": 1`；
2. 其余键保持领域形状（**不**做 `{schemaVersion, data}` 包装）；
3. 新增字段允许；删字段、改类型、改语义 = 破坏性，必须提升 schemaVersion 并在 CHANGELOG 写明；
4. `--json` 输出永远是一个 JSON 文档（`watch` 事件流除外：JSONL，每行自带 `schemaVersion`）。

冻结快照：`crates/prune-cli/cli-contract.v1.json`（由生产函数导出，测试逐字段比对）。MCP 工具面是另一套快照：`crates/prune-cli/mcp-tools.v1.json`——两者版本互不影响。

---

## prune scan

全盘扫描白名单目录，输出按清理级别分组的可回收清单。顺手把项目依赖体积记进账本（`prune projects` 可读）。

- 典型耗时 ~15s（31 条白名单）；进度走 stderr，stdout 保持纯输出
- 退出码：0（扫描完成即成功；个别目录无权限跳过并说明，不算失败）

```sh
prune scan
prune scan --json | jq '.totals'
```

JSON 顶层键：`schemaVersion`, `scannedAt`, `scanElapsedMs`, `incremental`, `groups`, `dup`, `totals`, `volume`, `projects`, `configNotes`

## prune reclaim

预览零风险清理项（L1 死重条目）。**默认 dry-run**；`--yes` 才真删（移入废纸篓，可恢复）。

```sh
prune reclaim --level green          # 预览
prune reclaim --level green --yes    # 执行
```

flags：`--level`（v1 只支持 green）、`--yes` / `-y`、`--dry-run`（兼容别名）、`--emit-script`（与 `--yes` 互斥）

### `--emit-script`（D-15，DP-4 裁 A）

把预览清单命令化成一段**可审计的 POSIX sh 脚本**打印到 stdout——只打印，**永不执行**；执行权在人。典型用途：把脚本交给 Agent 转述或存档，由人审阅后自行运行。

```sh
prune reclaim --emit-script > prune-clean.sh   # 保存，人工审阅后 sh prune-clean.sh
```

脚本特征：

- 每条命令是 `mv -n '<路径>' "$HOME/.Trash/"`——只进废纸篓、不用 `rm`、`-n` 不覆盖同名条目；路径经单引号转义，空格/引号/元字符全部失效在引号内
- 每项附注释：体积、分组与**白名单校验结果**（✓ 静态白名单 L1 零风险）；白名单复验不过的项不进脚本并 stderr 告警
- 脚本头自证来源（生成时间、版本）与边界声明「Prune 进程内不存在执行它的代码路径」——由 `emit_script.rs` 的源码断言单测守门

范围与 reclaim 一致：只取 L1 死重白名单项；L3 需逐项人工判断不进脚本，L4 永不提供删除命令。

## prune mem

一次性内存读数（footprint 口径 = 物理常驻 + 已压缩 + 已换出）。

```sh
prune mem
prune mem --swapped                  # 多一列换出明细
prune mem --json | jq '.pressure'
```

JSON 顶层键：`schemaVersion`, `physicalTotal`, `swap`, `pressure`, `families`, `topSwapped`

`prune mem --watch` 已弃用：功能并入 `prune watch --only mem`，别名 2027-09 移除。

## prune watch

按需守护：前台进程，Ctrl-C 即停，不装后台、不写日志。

```sh
prune watch --once --json            # 单轮，CI 用（恰好一行 JSON）
prune watch --only mem,port          # 只跑两类
prune watch --fail-on warn           # 达 warn 退出码 10（CI 门禁用）
```

`--once` JSON 顶层键：`schemaVersion`, `ts`, `mode`, `cpu`, `mem`, `ports`, `disk`（按 `--only` 按需出现）
事件流（不带 `--once`）是 JSONL，每行自带 `schemaVersion`。

- **关注端口来源两层**：在线项目（对绑定进程的 cwd 解析后读各项目 `prune.toml`）+ **CWD**（启动时从当前目录向上找最近一份带 `watch.ports` 的 `prune.toml`——补「项目还没起服务」的告警窗口）。就近的配置损坏不向上降级到父目录
- 有 CWD 配置时：人读横幅显示来源；`--once` 的 `ports` 段多一个 `watchedExtra`（`ports` + `source`）。无配置时输出与无此机制的版本逐字节一致
- **系统通知未实现（有意）**：watch 的出口纪律是 stdout/stderr，重定向由调用方决定——通知是不可被 CI 消费的第二个出口。要提醒就消费 JSONL

## prune port

谁占着这个端口。口径纪律：「没查到」≠「端口空闲」——内核口径与不可见条数一起说；TIME_WAIT 不是占用。

```sh
prune port                           # 关注层端口
prune port 3000                      # 单端口完整解释
prune port --conflicts --json        # 只看冲突（同端口 ≥2 个不同 pid）
prune port --all                     # 含内部 IPC / 系统服务端口
```

默认分支 JSON 顶层键：`schemaVersion`, `port`, `entries`, `coverage`, `timeWait`, `timeWaitPorts`, `scanMs`
`--conflicts` 分支 JSON 顶层键：`schemaVersion`, `at`, `conflicts`, `coverage`

退出码特例：`--json` 主输出为空且 `coverage.note` 指明权限不足 → **3**；采集失败 → **4**；差额说明 → 0。

## prune why

端口白话解释（F16）：给一句**可直接转述的结论**而不是原始条目——系统服务（杀掉会复活，附系统设置关闭路径）、项目 dev server（附项目目录）、多持有者冲突、TIME_WAIT 残留、确认空闲。结论只产自 `net::why::why()` 纯函数，与 MCP `prune_why` 同源。

```sh
prune why 5000                        # AirPlay 占用 → 附关闭路径
prune why 5173 --json | jq '.headline'
```

结论类别（`kind`）：`systemService` / `devServer` / `conflict` / `knownService`（L0 静态推断，headline 标「疑似」）/ `timeWait` / `free` / `unknown`。

口径纪律：`free` 必须在 Coverage 无差额时才出现——「没查到」≠「端口空闲」；`unknown` 只报观测事实，不编造用途；本命令只解释、不给任何执行动作。

JSON 顶层键：`schemaVersion`, `port`, `kind`, `headline`, `evidence`, `actionHint`

退出码特例：与 `prune port` 相同（权限不足 → **3**；采集失败 → **4**；差额说明 → 0）。

## prune projects

项目依赖账本（按天趋势）。读 GUI/扫描写入的同一张表；缺失的日子不补 0，单点样本如实显示「数据不足」。

```sh
prune projects
prune projects --days 90 --json | jq '.projects[].deltaBytes'
```

JSON 顶层键：`schemaVersion`, `from`, `days`, `projects`（账本不可用时 `status: "unavailable"` + `note`）

## prune doctor

四域体检：磁盘/内存/端口/网络，给出**结论 + 证据 + 下一步**，总预算 30s。

```sh
prune doctor
prune doctor --json | jq '.findings[].severity'
prune doctor --only mem,port         # 跳过 15s 的磁盘扫描
prune doctor --timeout 10            # 单域超时上限（秒，封顶 30）
```

- 判据全部复用既有口径：磁盘水位 15%/5%（与菜单栏 island 事件同一判据）、内存压力级别、swap >70%、端口冲突 `owner_count > 1`、网络 `NetDiag.status/verdict/captive`
- **不读 `prune.toml`**：doctor 是机器级体检，阈值是机器级常量；项目级配置只影响扫描/账本/端口关注层，不影响 doctor 判定
- 任一域读不到 → `severity: "unknown"` + 原因，**绝不显示 0**
- `severity` 词表固定四个值：`ok` / `warn` / `bad` / `unknown`
- 退出码：0（结论已出，含 warn/bad）；全部选中域不可读时按根因给 3 / 4

JSON 顶层键：`schemaVersion`, `at`, `elapsedMs`, `findings`, `coverage`

## prune app

一个应用的五域资源画像：进程/内存/CPU/端口/连接/磁盘。

```sh
prune app cursor
prune app cursor --json | jq '.memoryBytes'
```

- 家族名模糊匹配（大小写不敏感子串）；零命中或多个家族同时命中 → 退出码 2 + 候选列表
- 内存 footprint 优先（CLI 不深探，未深探进程为 RSS 下界）；CPU 单核口径可 >100%

JSON 顶层键：`schemaVersion`, `family`, `processCount`, `memoryBytes`, `memoryExact`, `cpu`, `cpuCovered`, `conns`, `ports`, `disk`

## prune lan

局域网设备与暴露面。**默认无副作用**（只读 ARP 邻居表）；`--scan` 才发探测包。

```sh
prune lan
prune lan --scan --json | jq '.devices | length'
```

JSON 顶层键：`schemaVersion`, `at`, `iface`, `cidr`, `scanned`, `total`, `devices`, `exposedPorts`, `notes`, `scanMs`

## prune mcp

启动 MCP stdio server（AI Agent 查询入口）。12 个只读工具的清单与 schema 见冻结快照 `crates/prune-cli/mcp-tools.v1.json`——快照是唯一真源，文档不抄 schema。

```sh
prune mcp                            # 由客户端拉起；人不直接敲
```

## prune mcp install

把 MCP server 写进客户端**用户级**配置。默认 dry-run，`--write` 才落盘。

```sh
prune mcp install                    # 预览三个客户端各自将写什么
prune mcp install --client vscode --write
prune mcp install --json             # 结构化计划（CI/Agent 消费）
```

- `--client`：`claude` / `cursor` / `vscode` / `all`（默认 all）；只支持用户级配置——项目级会弄脏 `git status`，不做
- VS Code 用顶层键 `servers` 且条目带 `"type": "stdio"`（抄 `mcpServers` 的块是「合法 JSON、不报错、不加载」）
- `command` 写的是当前二进制的**绝对路径**（`current_exe()`）——写裸名是「装成功但加载失败」的头号原因
- 安全边界：写哪个文件由代码常量决定，不接受 `--path`；坏 JSON 一律拒绝（`--force` 也不覆盖）；`--write` 前先备份 `<file>.prune-bak-<YYYYMMDD-HHMMSS>`（0600），原子写 + 读回校验，不符即回滚
- 幂等：条目已等价 → `noop`，不产生备份噪音
- 卸载：手动删除配置里的 `prune` 键即可（本期不做 `--uninstall`）
