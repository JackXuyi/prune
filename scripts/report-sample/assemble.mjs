// W1-2 体检样例报告 · Step 2 assemble
// 九段模板 + 五条硬断言（R1-R5，见 docs/w1-report-sample-plan.md §3.2）。
// 违反任何断言 → 抛错，渲染失败。不引模板引擎，纯拼接。

import { hostname, userInfo } from "node:os";
import { writeFileSync } from "node:fs";

// ---------------------------------------------------------------- 工具

const GB = 1024 ** 3;
export function fmtBytes(n) {
  if (n === null || n === undefined) return null;
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`;
  if (n < GB) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / GB).toFixed(1)} GB`;
}

// doctor JSON 里 severity 为小写（serde 默认）：ok / warn / bad / unknown
const SEV_ORDER = { bad: 0, warn: 1, unknown: 2, ok: 3 };
const SEV_MARK = { bad: "🔴", warn: "🟡", ok: "🟢", unknown: "⚪" };
const SEV_NAME = { bad: "严重", warn: "注意", ok: "正常", unknown: "读不到" };

// net 域证据标签的人话对照（标签本身来自 doctor.rs，不新增口径）
const NET_LABEL = {
  dns: "域名解析",
  lan: "局域网",
  wan: "公网",
  jitter: "抖动",
  gateway: "网关",
};

const N_A = "该字段本版本不提供";

// ---------------------------------------------------------------- 来源账本（R1）

/** 每个进入报告的数字都经此登记：数字 → 命令 → JSON 路径 → 脚注 */
class Ledger {
  constructor() {
    this.sources = [];
    this.map = new Map(); // "cmd|path" -> 脚注编号
  }
  ref(cmd, path) {
    const k = `${cmd}|${path}`;
    if (!this.map.has(k)) {
      this.map.set(k, this.sources.length + 1);
      this.sources.push({ no: this.map.get(k), cmd, path });
    }
    return this.map.get(k);
  }
  /** 登记并返回脚注标记 [^sN] */
  mark(cmd, path) {
    return `[^s${this.ref(cmd, path)}]`;
  }
}

// ---------------------------------------------------------------- 组装

export function assemble(collected, { redact = true, pruneVersion = "unknown" } = {}) {
  const L = new Ledger();
  const it = collected.items;

  // 契约 JSON 也可能是"残缺"的（如 exit 3 时的降级文档）：缺关键字段一律按读不到处理，
  // 绝不让章节函数在 undefined 上崩溃——崩溃会把"失败是内容"变成"失败是异常"。
  const REQUIRED = {
    doctor: ["findings", "coverage"],
    scan: ["totals", "volume", "groups"],
    mem: ["physicalTotal", "swap"],
    port: ["conflicts", "coverage"],
    projects: ["projects", "days"],
  };
  for (const [k, keys] of Object.entries(REQUIRED)) {
    const v = it[k];
    if (!v?.ok) continue;
    const missing = keys.filter((key) => v.data?.[key] === undefined);
    if (missing.length) {
      v.ok = false;
      v.reason = `返回 JSON 缺少字段 ${missing.join("、")}${v.exitNote ? `（退出码 ${v.exitCode}：${v.exitNote}）` : ""}，按读不到处理`;
    }
  }

  const S = (key) => (it[key]?.ok ? it[key] : null);
  const CMD = (key) => it[key]?.cmd ?? key;

  const findings = S("doctor")?.data?.findings ?? [];
  const parts = [];

  parts.push(metaSection(it, S, CMD, L, { pruneVersion, waves: collected.waves }));
  parts.push(conclusionSection(it, S, CMD, L, findings));
  parts.push(diskSection(it, S, CMD, L));
  parts.push(memSection(it, S, CMD, L, findings));
  parts.push(portSection(it, S, CMD, L, findings));
  parts.push(netSection(it, S, CMD, L));

  // ---- 段 7：来源附录 ----
  parts.push(`## 7. 来源附录（每个数字可复核）

| 脚注 | 命令 | JSON 路径 |
|---|---|---|
${L.sources.map((s) => `| S${s.no} | \`${s.cmd}\` | \`${s.path}\` |`).join("\n")}
`);

  // ---- 段 8：口径说明（固定文案）----
  parts.push(`## 8. 口径说明

- **内存数字用的是 phys_footprint，不是 RSS。** phys_footprint 是进程占用的物理内存页合计，比"实际内存"（活动监视器口径）更大也更诚实——被压缩或换出的页都算在内。两套口径没有对错，但混用会得出错误结论。
- **0 ≠ 读不到。** 某项显示 0 或"无"时，如果当时没有读取权限，报告会明确写"读不到 + 原因"，绝不把"没读到"渲染成"没有问题"。
- **怀疑不下判决。** 体检只报告"读到什么、依据是什么、下一步可以查什么"，不自动删除任何东西，也不把猜测写成结论。
- **同一指标可能来自两条命令。** swap 使用率这类实时量在两次读取之间会变动（doctor 与 mem 的读取时刻相差数十秒）；每个数字都带自己的来源脚注，以脚注指向的命令为准，报告不合并、不取平均、不做四舍五入对齐。
- **🟢 正常 / 🟡 注意 / 🔴 严重 / ⚪ 读不到。** ⚪ 不是正常：它表示这一项在本机没有读到数据，原因附在条目里。`);

  // ---- 段 9：底线声明（固定文案）----
  parts.push(`## 9. 底线声明

- 本报告**完全在本机生成**，不连接网络服务，不上传任何数据。
- Prune **不会自动删除**任何文件；报告中所有"可回收"均指"确认后可手动移入废纸篓"。
- 报告默认开启脱敏：主机名、用户名、项目目录名、内网地址均已替换；局域网设备清单不在采集范围内。`);

  let md = parts.join("\n\n---\n\n") + "\n";

  // ---- R5 脱敏（默认开）----
  if (redact) md = applyRedaction(md, collected);

  // ---- 五条硬断言 ----
  assertHard(md, { L, findings, it, redact, collected });

  return { md, ledger: L };
}

// ---------------------------------------------------------------- 段 1 · 元信息

function metaSection(it, S, CMD, L, { pruneVersion, waves }) {
  const atRaw = S("doctor")?.data?.at ?? S("scan")?.data?.scannedAt ?? null;
  const at = atRaw ? new Date(atRaw * 1000).toLocaleString("sv-SE") : null;
  const phys = S("mem")?.data?.physicalTotal ?? null;
  const vol = S("scan")?.data?.volume ?? null;
  const machine =
    phys && vol
      ? `物理内存 ${fmtBytes(phys)}${L.mark(CMD("mem"), "physicalTotal")}，磁盘总容量 ${fmtBytes(vol.totalBytes)}${L.mark(CMD("scan"), "volume.totalBytes")}，当前可用 ${fmtBytes(vol.availableBytes)}${L.mark(CMD("scan"), "volume.availableBytes")}`
      : N_A;

  const status = Object.values(it)
    .map((v) => {
      const mark = !v.ok ? "✗" : v.degraded ? "⚠" : "✓";
      const secs = `${((v.ms ?? 0) / 1000).toFixed(1)}s`;
      const note = !v.ok ? ` ${v.reason}` : v.degraded ? ` 退出码 ${v.exitCode}（${v.exitNote}）` : "";
      return `${v.cmd.split(" ").slice(1, 2)[0] ?? v.key} ${mark} ${secs}${note}`;
    })
    .join(" · ");

  const method =
    waves === 2
      ? "分两波：scan / mem / port / projects 并行，doctor 独占跑（避免两轮磁盘遍历互相挤压触发超时）"
      : "5 条只读命令串行采集";

  return `# 开发机体检报告（样例）

> 本报告由 Prune CLI 在本机生成，未上传任何数据。结论口径见第 8 节，每个数字的来源见第 7 节。

## 1. 元信息

| 项 | 值 |
|---|---|
| 生成时间 | ${at ?? N_A} |
| Prune 版本 | ${pruneVersion} |
| 契约版本 schemaVersion | ${contractVersion(it)} |
| 采集方式 | ${method} |
| 采集状态 | ${status} |
| 机器形态 | ${machine} |
| 未覆盖域 | ${coverageGaps(it) || "无"} |`;
}

function contractVersion(it) {
  const ok = Object.values(it).filter((x) => x.ok);
  if (!ok.length) return "无法确认（所有命令未采集成功）";
  return `v${ok[0].data.schemaVersion}`;
}

function coverageGaps(it) {
  const d = it.doctor?.data;
  if (!d) return null;
  const gaps = Object.entries(d.coverage ?? {})
    .filter(([, v]) => v?.state !== "ok")
    .map(([k, v]) => `${k}（${v.state}${v.note ? `：${v.note}` : ""}）`);
  return gaps.length ? gaps.join("、") : null;
}

// ---------------------------------------------------------------- 段 2 · 结论卡

function conclusionSection(it, S, CMD, L, findings) {
  const reclaimable = S("scan")?.data?.totals?.reclaimable ?? null;
  // 结论卡只放"需要人看一眼"的结论：🟢 不占位（否则会把三条结论挤成两条噪音）。
  // 首句已经给了"可回收 xx GB"，对应 finding 让位，避免同一句话出现两次。
  const pool = findings.filter(
    (f) => f.severity !== "ok" && !(reclaimable !== null && f.id === "disk.reclaimable_high"),
  );
  const top = [...pool].sort((a, b) => SEV_ORDER[a.severity] - SEV_ORDER[b.severity]).slice(0, 3);
  const sentences = top.map(
    (f) => `${SEV_MARK[f.severity]} ${f.title}${L.mark(CMD("doctor"), `findings[id=${f.id}].title`)}`,
  );

  const head =
    reclaimable !== null
      ? `磁盘上可回收空间合计约 **${fmtBytes(reclaimable)}**${L.mark(CMD("scan"), "totals.reclaimable")}。`
      : "磁盘总量本机未采集成功，无法给出可回收合计。";
  const body = sentences.length
    ? sentences.join("；") + "。"
    : findings.length
      ? "四项体检均未触发阈值，没有需要立刻处理的问题（逐项数据见第 3–6 节）。"
      : "体检未产出结论（doctor 未采集成功，原因见第 1 节采集状态）。";

  return joinBlocks(
    "## 2. 结论卡",
    head,
    body,
    unreadableSentence(findings, CMD("doctor"), L),
  );
}

/** 组装一节：丢掉空行，避免出现连续空段 */
function joinBlocks(heading, ...blocks) {
  return [heading, ...blocks.filter((b) => b && b.trim())].join("\n\n");
}

function unreadableSentence(findings, cmd, L) {
  const u = findings.filter((f) => f.unreadable);
  if (!u.length) return "";
  return `以下结论建立在**未读到的数据**上，原因已注明：${u
    .map((f) => `「${f.title}」${L.mark(cmd, `findings[id=${f.id}].unreadable`)}`)
    .join("、")}。`;
}

// ---------------------------------------------------------------- 段 3 · 磁盘面

function diskSection(it, S, CMD, L) {
  const lines = [];
  const scan = S("scan");

  if (scan) {
    const t = scan.data.totals;
    lines.push(
      `- 分级合计：🟢 L2 ${fmtBytes(t.l2)}${L.mark(CMD("scan"), "totals.l2")}、🟡 L3 ${fmtBytes(t.l3)}${L.mark(CMD("scan"), "totals.l3")}、🔴 L4 ${fmtBytes(t.l4)}${L.mark(CMD("scan"), "totals.l4")}；可回收合计 ${fmtBytes(t.reclaimable)}${L.mark(CMD("scan"), "totals.reclaimable")}。`,
    );
    lines.push(
      `  - 分级含义：L2 = 可再生缓存（清理后由工具自动重建）；L3 = 需判断项（清理代价高，逐项确认）；L4 = 只给命令、不自动执行。可回收合计只含 L2 + L3，L4 不计入。`,
    );

    const groups = (scan.data.groups ?? [])
      .filter((g) => g.cleanable)
      .sort((a, b) => b.totalBytes - a.totalBytes)
      .slice(0, 5);
    if (groups.length) {
      lines.push(
        `  - 体积最大的 ${groups.length} 组：` +
          groups
            .map(
              (g) =>
                `${g.label} ${fmtBytes(g.totalBytes)}（${String(g.level ?? "?").toUpperCase()}）${L.mark(CMD("scan"), `groups[key=${g.key}].totalBytes`)}`,
            )
            .join("、") +
          "。",
      );
    }

    const dup = scan.data.dup;
    if (dup && dup.totalBytes > 0) {
      lines.push(
        `  - 重复存储：${dup.advice.replace(/。?$/, "；")}合计 ${fmtBytes(dup.totalBytes)}${L.mark(CMD("scan"), "dup.totalBytes")}（与上方分组重叠，不相加）。`,
      );
    }
  } else {
    lines.push(`- ⚪ 磁盘明细读不到：${it.scan?.reason ?? "未采集"}。`);
  }

  // 项目账本：0 / null 不混用——0 是"持平"，null 是"采样不足，算不出"
  const prj = S("projects");
  if (prj) {
    const list = prj.data.projects ?? [];
    const delta = (p) => (p.deltaBytes === null || p.deltaBytes === undefined ? null : p.deltaBytes);
    const moved = list.filter((p) => delta(p) !== null && delta(p) !== 0);
    const flat = list.filter((p) => delta(p) === 0).length;
    const unknown = list.filter((p) => delta(p) === null).length;

    if (moved.length) {
      const top = [...moved]
        .sort((a, b) => Math.abs(delta(b)) - Math.abs(delta(a)))
        .slice(0, 3);
      lines.push(
        `- 项目账本（${prj.data.days} 天）：` +
          top
            .map(
              (p) =>
                `${p.name} ${delta(p) > 0 ? "+" : "-"}${fmtBytes(Math.abs(delta(p)))}${L.mark(CMD("projects"), `projects[name=${p.name}].deltaBytes`)}`,
            )
            .join("、") +
          (list.length > 3 ? `（共 ${list.length} 个项目）` : "") +
          "。",
      );
    } else {
      lines.push(
        `- 项目账本（${prj.data.days} 天）：${list.length} 个项目${L.mark(CMD("projects"), "projects")}本期均无可判定变化（持平 ${flat} 个，采样不足 ${unknown} 个）。`,
      );
    }
    lines.push(
      `  - 账本口径：只记两次采样之间的体积差；采样不足的项目显示"数据不足"，不记 0。`,
    );
  } else {
    lines.push(`- ⚪ 项目账本读不到：${it.projects?.reason ?? "未采集"}。`);
  }

  return `## 3. 磁盘面\n\n${lines.join("\n")}`;
}

// ---------------------------------------------------------------- 段 4 · 内存面

/** swap 占比优先取 doctor 的判定（结论卡用的就是它），避免同一份报告出现两个占比 */
function swapPctFromDoctor(findings) {
  const f = findings.find((x) => x.id === "mem.swap_high");
  const m = f?.title?.match(/(\d+(?:\.\d+)?)\s*%/);
  return m ? `${m[1]}%` : null;
}

function memSection(it, S, CMD, L, findings) {
  const mem = S("mem");
  if (!mem) return `## 4. 内存面\n\n⚪ 读不到：${it.mem?.reason ?? "未采集"}。`;

  const d = mem.data;
  const pctFromDoctor = swapPctFromDoctor(findings);
  const pctOwn = d.swap?.total ? `${Math.round((d.swap.used / d.swap.total) * 100)}%` : null;
  const pct = pctFromDoctor
    ? `${pctFromDoctor}${L.mark(CMD("doctor"), "findings[id=mem.swap_high].title")}`
    : pctOwn
      ? `${pctOwn}${L.mark(CMD("mem"), "swap.used/swap.total")}`
      : N_A;

  const fam = [...(d.families ?? [])]
    .filter((f) => f.footprint)
    .sort((a, b) => b.footprint - a.footprint)
    .slice(0, 3);
  const topSwap = (d.topSwapped ?? []).slice(0, 3);
  const press = d.pressure?.level ?? N_A;

  return `## 4. 内存面

- 物理内存 ${fmtBytes(d.physicalTotal)}${L.mark(CMD("mem"), "physicalTotal")}；swap 已用 ${fmtBytes(d.swap?.used)}${L.mark(CMD("mem"), "swap.used")} / 共 ${fmtBytes(d.swap?.total)}${L.mark(CMD("mem"), "swap.total")}，占比 ${pct}；内存压力 ${press}${L.mark(CMD("mem"), "pressure.level")}${d.pressure?.metric ? `（口径 ${d.pressure.metric}）` : ""}。
- 占用最大的 ${fam.length} 个进程族：${fam.map((f) => `${f.name} ${fmtBytes(f.footprint)}${L.mark(CMD("mem"), `families[name=${f.name}].footprint`)}`).join("、") || "本版本不提供"}。
  - 其中被换出到 swap 最多的：${topSwap.length ? topSwap.map((p) => `${p.name}（${p.family}，pid ${p.pid}）已换出 ${fmtBytes(p.swapped)}${L.mark(CMD("mem"), `topSwapped[pid=${p.pid}].swapped`)}`).join("、") + "。" : "本版本不提供。"}
  - 进程族名取可执行文件的家族名，不含路径；单个进程的路径不在报告中出现。`;
}

// ---------------------------------------------------------------- 段 5 · 端口面

function portSection(it, S, CMD, L, findings) {
  const port = S("port");
  const lines = [];
  if (port) {
    const c = port.data.conflicts ?? [];
    const cov = port.data.coverage ?? {};
    if (c.length) {
      lines.push(
        `- 🔴 冲突端口 ${c.length} 个：` +
          c
            .map((x, i) => `${x.port ?? "?"}（${x.owners?.map?.((o) => o.name).join(" / ") ?? "归属未读到"}）${L.mark(CMD("port"), `conflicts[${i}]`)}`)
            .join("、") +
          "。",
      );
    } else {
      lines.push(`- 🟢 冲突端口：未发现${L.mark(CMD("port"), "conflicts")}。`);
    }
    lines.push(
      `- 监听可见性：可归属 ${cov.attributed ?? N_A}${L.mark(CMD("port"), "coverage.attributed")} / 共 ${cov.listenTotal ?? N_A}${L.mark(CMD("port"), "coverage.listenTotal")} 个监听${cov.note ? `（${cov.note}）` : ""}。差值即当前用户无权限看到的监听——它们不在本报告的任何结论里。`,
    );
  } else {
    lines.push(`- ⚪ 端口面读不到：${it.port?.reason ?? "未采集"}。`);
  }

  // doctor 的 port 判定已在上面用端口 JSON 表达过：只在它给出非 ok 结论时追加，避免重复一行
  const pf = (findings ?? []).filter((f) => f.domain === "port" && f.severity !== "ok");
  lines.push(...pf.map((f) => renderFinding(f, CMD("doctor"), L)));

  return `## 5. 端口面\n\n${lines.join("\n")}`;
}

// ---------------------------------------------------------------- 段 6 · 网络面

function netSection(it, S, CMD, L) {
  const d = it.doctor?.data;
  const nf = (d?.findings ?? []).filter((f) => f.domain === "net");
  const state = d?.coverage?.net?.state;
  const lines = [];
  if (nf.length) {
    lines.push(...nf.map((f) => renderFinding(f, CMD("doctor"), L, NET_LABEL)));
    lines.push(`  - 网络域只测"能不能出去、出去多快"，不扫描局域网设备，也不记录公网 IP。`);
  } else if (state === "ok") {
    lines.push(`- 🟢 网络域未产出异常结论${L.mark(CMD("doctor"), "coverage.net.state")}。`);
  } else {
    lines.push(
      `- ⚪ 网络域读不到（state=${state ?? "未采集"}）：${it.doctor?.reason ?? "见第 1 节采集状态"}。`,
    );
  }
  return `## 6. 网络面\n\n${lines.join("\n")}`;
}

function renderFinding(f, cmd, L, labelMap = {}) {
  const ev = (f.evidence ?? [])
    .map((e, i) => {
      const label = labelMap[e.label] ?? e.label;
      return `${label} ${e.value}${L.mark(cmd, `findings[id=${f.id}].evidence[${i}]`)}`;
    })
    .join("，");
  const un = f.unreadable
    ? ` ⚪ 读不到的原因：${f.unreadable}${L.mark(cmd, `findings[id=${f.id}].unreadable`)}`
    : "";
  const next = (f.next ?? []).length ? `（下一步：${f.next.join("；")}）` : "";
  const evidence = ev ? `${ev}。` : "（doctor 未附证据项）";
  return `- ${SEV_MARK[f.severity]} **${f.title}**：${evidence}${un}${next}`;
}

// ---------------------------------------------------------------- R5 脱敏

/** A..Z, AA, AB …（超过 26 个项目时不能用 charCode 硬转，否则出现 "项目k" 这类乱码） */
function labelFor(i) {
  let s = "";
  let n = i;
  do {
    s = String.fromCharCode(65 + (n % 26)) + s;
    n = Math.floor(n / 26) - 1;
  } while (n >= 0);
  return s;
}

function applyRedaction(md, collected) {
  let out = md;
  const user = (() => {
    try {
      return userInfo().username;
    } catch {
      return null;
    }
  })();
  const host = hostname();

  // 1) 用户名与 home 路径
  if (user) out = out.replaceAll(`/Users/${user}`, "~").replaceAll(user, "用户");
  // 2) 主机名
  if (host) out = out.replaceAll(host, "开发机 #1");

  // 3) 项目目录 → 项目 A/B/C：按首次出现顺序定号（不按体积，换机器也稳定），
  //    替换时按长度降序，避免 "a/b" 先吃掉 "a/b/c"。
  const names = [];
  for (const p of collected.items.projects?.data?.projects ?? []) {
    if (p.name && !names.includes(p.name)) names.push(p.name);
  }
  const labelByName = new Map(names.map((n, i) => [n, `项目 ${labelFor(i)}`]));
  for (const name of [...names].sort((a, b) => b.length - a.length)) {
    out = out.replaceAll(name, labelByName.get(name));
  }

  // 4) 内网地址（网关等）：只保留前两段，后两段掩码
  out = out.replace(
    /\b(10\.\d{1,3}|192\.168|172\.(?:1[6-9]|2\d|3[01]))\.\d{1,3}\.\d{1,3}\b/g,
    "$1.x.x",
  );

  return out;
}

// ---------------------------------------------------------------- 硬断言

const BANNED = [
  /性能提升/, /提升性能/, /优化/, /加速/, /一键/, /智能/,
  /独家/, /最强/, /最佳/, /最先进/, /业界领先/, /绝对安全/, /百分百/,
];

const PRIVATE_IP = /\b(?:10|192\.168|172\.(?:1[6-9]|2\d|3[01]))\.\d{1,3}\.\d{1,3}\b/;

export function assertHard(md, { L, findings, it, redact, collected }) {
  const errors = [];

  // R1：只要报告里出现了任何读到的数据，来源账本就必须非空；
  // 全机读不到时账本可以为空——那正是"诚实报告 0 ≠ 读不到"该有的样子。
  const hasData = Object.values(it).some((v) => v.ok);
  if (hasData && L.sources.length === 0) {
    errors.push("R1 有命令返回了数据，但来源账本为空——数字未经溯源登记。");
  }
  // R1：脚注双向一致——正文每个 [^sN] 都要有附录行，附录每行都要被正文用到
  const used = new Set([...md.matchAll(/\[\^s(\d+)\]/g)].map((m) => Number(m[1])));
  for (const n of used) {
    if (n < 1 || n > L.sources.length) errors.push(`R1 正文脚注 [^s${n}] 在来源附录中不存在。`);
  }
  for (const s of L.sources) {
    if (!used.has(s.no)) errors.push(`R1 来源附录 [^s${s.no}]（${s.cmd} ${s.path}）未被正文引用。`);
  }

  // R2：unreadable 非空的 finding，报告中必须出现其原因（禁止静默）
  for (const f of findings) {
    if (f.unreadable && !md.includes(f.unreadable)) {
      errors.push(`R2 finding「${f.id}」声明 unreadable 但报告未渲染原因。`);
    }
  }
  // 采集失败的域，报告必须出现原因（失败是内容）
  for (const [key, v] of Object.entries(it)) {
    if (!v.ok && !(v.reason && md.includes(v.reason))) {
      errors.push(`R2 命令 ${key} 采集失败但报告未渲染原因（"${v.reason}"）。`);
    }
    // 以非 0 退出码返回但仍有数据（部分不可读）：退出码含义也必须出现在报告里
    if (v.ok && v.degraded && v.exitNote && !md.includes(v.exitNote)) {
      errors.push(`R2 命令 ${key} 以退出码 ${v.exitCode}（${v.exitNote}）返回，报告未标注。`);
    }
  }

  // R3：违禁词 0 命中
  md.split("\n").forEach((line, i) => {
    for (const p of BANNED) {
      if (p.test(line)) {
        errors.push(`R3 第 ${i + 1} 行违禁词（/${p.source}/）：${line.trim().slice(0, 80)}`);
        break;
      }
    }
  });

  // R4：元信息禁止编造——不允许出现 undefined/NaN/null 占位
  if (/undefined|NaN|\bnull\b/.test(md)) {
    errors.push("R4 报告出现 undefined/NaN/null——元信息缺字段必须写「该字段本版本不提供」。");
  }

  // R5：脱敏后不得残留用户名、主机名、项目目录名、内网地址
  if (redact) {
    let user = null;
    try {
      user = userInfo().username;
    } catch {}
    for (const token of [user, hostname()].filter(Boolean)) {
      if (md.includes(token)) errors.push(`R5 脱敏失败：报告中残留敏感串「${token}」。`);
    }
    for (const p of collected?.items?.projects?.data?.projects ?? []) {
      if (p.name && md.includes(p.name)) {
        errors.push(`R5 脱敏失败：报告中残留项目目录「${p.name}」。`);
      }
    }
    const ip = md.match(PRIVATE_IP);
    if (ip) errors.push(`R5 脱敏失败：报告中残留内网地址「${ip[0]}」。`);
  }

  if (errors.length) {
    if (process.env.REPORT_DEBUG) writeFileSync("/tmp/debug-report.md", md);
    throw new Error(`组装断言未通过（共 ${errors.length} 条）：\n  - ` + errors.join("\n  - "));
  }
}
