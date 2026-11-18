# F21 dev server 入站来源观测（Inbound Source Observation）

> 状态：方案（未实施）。P1，差异化最独家的一项。
> 竞品依据（2026-11 核实）：端口↔连接↔来源**三视图联合无竞品做**；
> 「谁在访问我的 localhost」完全空白。Burrow 等只做远程 IP 反解析（v2 边界已写死不做）。

## 一、背景

- dev server 跑起来后，`conns` 列只知道「有几条连接」，不知道**谁连进来的**：
  是自己浏览器、同一台机器的其它工具、局域网同事的真机调试，还是未知来源。
- 数据已在一跳之内：`ports.rs` 的 lsof 原文里 ESTABLISHED 行本就带 `local->remote`，
  当前解析只取 `->` 左侧计数（`parse_lsof_tcp` L192 起），右侧**被丢弃**。
  本方案 = 把已采集但未保留的证据接住，**零新增采集、零新增进程**。

## 二、成功标准

1. 端口行的连接数能看到来源构成：本机回环 / 局域网 / 公网 三段计数 + 展开明细。
2. CLI `prune port 3000` 输出来源段；MCP `port_listeners` 载荷带 inbound 摘要。
3. 零新增采集成本（同一轮 lsof），30 s 轮节奏不变。
4. 不发包、不反解析、不联网——纯读已采集文本。

## 三、不做什么

- **不做远程 IP 反解析 / 归属地**——v2 边界写死；反解析是发包 + 隐私双坑。
- **不做流量归因**（哪个来源占了多少字节）——lsof 给不了；nettop 维度归应用不归对端。
- **不做历史来源库**（「这个 IP 历史上连过几次」）——快照是当下的，历史是
  lan_history 的职责边界；混进来会模糊「观测 vs 记账」。
- **不做拦截/放行**——只读守门员纪律。

## 四、方案

### 4.1 核心层（net/ports.rs + net/mod.rs）

```rust
/// 连接对端的来源段分类（只分类，不定因）
pub enum PeerScope { Loopback, Lan, Public }
pub struct PeerEntry { pub addr: String, pub scope: PeerScope }
// PortSnapshot 扩展：
pub peers_by_port: BTreeMap<u16, PortPeers>,   // 仅 ESTABLISHED 且本端是监听口的行
pub struct PortPeers { pub loopback: u32, pub lan: u32, pub public: u32,
                      pub detail: Vec<PeerEntry> }   // detail 上限 32 条，超出只计数
```

设计决策：
- **只对「本端是监听端口」的 ESTABLISHED 行记录对端**：连出去的（浏览器→外网）
  本端是高端口、不在 listeners 集合里，天然过滤——inbound 语义自动成立，无需额外判断方向。
- **scope 判定纯函数**：`127.x/::1` → Loopback；RFC1918/链路本地 → Lan；其余 → Public。
  与 `BindScope::from_addr` 同文件放一个 `PeerScope::from_addr`，测试对表。
- **detail 截断 32 条**：压测/爬虫场景一条端口可能几百连入，明细是给人看的，
  计数才是口径；截断在计数旁标 `+N more`，不谎称「全部」。
- **UDP 不记对端**：lsof 对 UDP 无连接语义，`conns` 口径本来也只算 TCP。

### 4.2 消费面

- **前端（PortList.tsx）**：`conns` 列数字旁 hover tooltip 出「本机 3 · 局域网 1 · 公网 0」；
  `lan > 0` 时数字变 accent 提示「局域网有人连着」。展开行下子行列 detail（≤5 条 + more）。
  语义化提示克制：公网连入**不**标红——对暴露面为 any 的 dev server 这是正常态，
  风险判断已有 exposure 徽标承担，不重复吓人。
- **CLI**：`prune port 3000` 单端口视图追加来源段行（`入站：本机 3 · 局域网 1（192.168.1.8…）`）。
- **MCP**：`port_listeners` 载荷加 `inbound: {loopback, lan, public}` 摘要——agent 可答
  「现在有没有局域网设备在访问我的 dev server」。契约快照同变更（mcp-tools.v1.json +
  README 工具描述）。

### 4.3 兼容与序列化

`PortSnapshot` 是事件 `port://snapshot` 与 IPC 的载荷，加字段必须全链路同批：
core struct（serde camelCase）→ `types/net.ts` → 前端消费 → devFixture（`devPortSnapshot`
补样例）。老快照事件缺字段前端要兜底（`?? null`），因为订阅方可能先于 Rust 重启收到旧载荷。

## 五、里程碑

| 里程碑 | 内容 | 验证 |
|---|---|---|
| M-A | 解析层保留对端 + PeerScope 纯函数 + 单测（TCP_SAMPLE 扩展 `->` 样例） | cargo test |
| M-B | PortSnapshot 扩展 + CLI/MCP 消费 + 契约双快照同变更 | cargo test + check:docs |
| M-C | 前端 types/ipc/devFixture + conns 列 tooltip + 行下明细 | tsc + 真机 |
| M-D | README MCP 描述更新 + 门禁全套 | check:* 全绿 |

## 六、风险与对策

- **lsof 输出量增大**：只是字符串保留，无额外进程/无额外轮次；实测本轮 30–40 ms 不变。
- **IPv6 对端格式**（`[::1]:port`）：split_addr 已处理 v6 本端，对端复用同一解析 + 单测样例。
- **高频连入把 detail 打爆**：32 条截断 + 计数口径；tooltip 只出计数，明细在行下可折叠。
- **agent 误读「public > 0 = 被攻击」**：MCP 载荷 note 写死「公网连入对 any 暴露面属正常，
  是否收紧看绑定面，不由本字段判断」。

## 七、验收清单

- [ ] 单测：TCP_SAMPLE 增加 ESTABLISHED `->` 样例；scope 三段判定表；32 条截断 + more
- [ ] 真机：本机 curl dev server → tooltip 本机 +1；手机连同一 Wi-Fi 访问 → 局域网 +1 且 accent
- [ ] CLI `prune port <port>` 来源段输出；`--json` 键序进契约
- [ ] MCP `port_listeners` 带 inbound 摘要 + note；快照测试重导出
- [ ] 老事件载荷兼容：前端对缺字段不崩（`?? null`）
- [ ] 门禁全套绿；无任何新增网络发包路径（grep 可证）

## 八、实现记录（2026-11-18）

- 落点：`net/mod.rs`（PeerScope/PeerEntry/PortPeers + PortEntry.inbound，PeerScope 判定表测试）/ `net/ports.rs`（TcpScan.peers 原样保留 `->` 右侧、attach_inbound 纯函数回填）/ CLI 单端口视图「入站」段 / MCP port_listeners（entries serde 直传 inbound，工具描述更新 + 快照重导出）/ 前端 types + PortList（conns 列 tooltip + 外部来源 accent + 行下明细）+ devFixture 样例。
- 与方案的偏差：PeerScope 增加 `Unknown`（地址解析失败如实标注，不塞进别的段谎报——比原三段更诚实）；明细行展示对端地址不含端口（去重口径按地址）。
- 门禁：core 522 / cli 59 全绿（契约快照重导出）；tsc / check:i18n / check:docs 通过。
