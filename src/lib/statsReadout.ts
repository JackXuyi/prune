/**
 * 统计读数层（M16）：把五个域的原始快照翻译成「主读数 + 占比 + 次级读数 + 状态」。
 *
 * 为什么是纯函数：主窗口的统计卡与菜单栏面板的统计行必须说同一句话。如果两份
 * JSX 各自算口径，早晚会在某次改动里分叉——那时用户看到的是两块屏上不同的
 * 数字，而不是一个能立刻定位的 bug。所以口径只在这里定义一次，展示层只排版。
 *
 * 三条红线（沿用 M9 / M14，改这里之前先读 docs/m16-stats-unify-plan.md §8）：
 * - 读不到 = `unavailable`，不是 0
 * - 还没攒出基线 = `warmup`，不是 0
 * - `ok` 且数值为 0 = 真的没有，正常显示 0
 */
import type { VolumeInfo } from "../types/disk";
import type { CpuInfo, MemSnapshot, PressureLevel } from "../types/mem";
import {
  formatSpeed,
  weightedAverage,
  type NetTotalPoint,
  type NetTotalSample,
  type PortSnapshot,
} from "../types/net";
import { formatBytes, formatPercent, splitBytes } from "./format";

export type Tone = "accent" | "warn" | "danger" | "info" | "neutral";
export type ReadoutStatus = "ok" | "warmup" | "unavailable";

export interface Readout {
  /** 主读数数值（已格式化，不含单位） */
  main: string;
  unit: string;
  /** 主读数后面的弱化文本，如 `/ 16.0 GB`（共用分母单位时用） */
  mainTail?: string;
  /** 0–1；无占比语义的域为 null */
  ratio: number | null;
  /** 次级读数，按「短 → 长」排序；面板只取第 1 条，卡片全列 */
  sub: string[];
  status: ReadoutStatus;
  tone: Tone;
  /** 状态说明；status 非 ok 时必有值 */
  note?: string;
  /** 标签右侧的小状态（目前只有内存压力），其它域为 undefined */
  tag?: { text: string; tone: Tone };
}

export interface NetReadout extends Readout {
  downBps: number;
  upBps: number;
  /** 各自归一化的走势（主窗口双线用） */
  downSeries: number[];
  upSeries: number[];
  avgDownText: string;
  avgUpText: string;
}

/** 方向（M16 v6）：下载 / 上传各自成卡成行后，读数按方向派生 */
export type NetDirection = "down" | "up";

export interface NetDirectionReadout {
  dir: NetDirection;
  /** 原始 bps；0 在 ok 下是「真的没有」，warmup / unavailable 下不可信 */
  bps: number;
  /** 数字与单位拆开，给大号读数排版用 */
  main: string;
  unit: string;
  /** 该方向的走势序列（各自归一化，两处界面共用） */
  series: number[];
  /** 近 5 min 时间加权均值，已格式化，如 "90.7 KB/s"（不含箭头） */
  avgText: string;
  /** 今日累计，已格式化；today 为 null（未落库）时是 null —— 不是 0 */
  todayText: string | null;
  /** 本次会话单拍峰值，已格式化；无读数时为 null（时间不在此显示：peakAt 双方向共用） */
  peakText: string | null;
  /** 会话累计（自启动起），已格式化 */
  sessionText: string | null;
  status: ReadoutStatus;
  note?: string;
}

// ---------------------------------------------------------------- 内存

export function memoryReadout(snapshot: MemSnapshot | null): Readout {
  if (!snapshot) return pending("采样中…");

  const total = snapshot.physicalTotal;
  const used = snapshot.physicalUsedBytes;
  // 旧快照缺 physicalUsedBytes：这是「还没读到」，不能拿 0 顶上
  if (used == null || total <= 0) return pending("已用读数尚未就绪");

  const ratio = used / total;
  const usedPart = splitBytes(used);
  const totalPart = splitBytes(total);
  const swapRatio = snapshot.swap.total > 0 ? snapshot.swap.used / snapshot.swap.total : null;

  return {
    main: usedPart.value,
    unit: usedPart.unit,
    mainTail: `/ ${totalPart.value} ${totalPart.unit}`,
    ratio,
    tone: loadTone(ratio),
    sub: compact([
      `可用 ${formatBytes(snapshot.freeBytes)}`,
      swapRatio != null ? `swap ${formatPercent(swapRatio)}` : null,
    ]),
    status: "ok",
    tag: snapshot.pressure ? pressureTag(snapshot.pressure.level) : undefined,
  };
}

// ---------------------------------------------------------------- CPU

export function cpuReadout(cpu: CpuInfo | null | undefined): Readout {
  if (!cpu || cpu.usage == null) return pending("采样中…（约 10 s）");

  const usage = cpu.usage;
  const ratio = Math.max(0, Math.min(1, usage / 100));
  return {
    main: usage < 1 ? "<1" : String(Math.round(usage)),
    unit: "%",
    ratio,
    tone: loadTone(ratio),
    sub: compact([
      `${cpu.cores} 核`,
      cpu.load
        ? `load ${cpu.load.one.toFixed(2)} / ${cpu.load.five.toFixed(2)} / ${cpu.load.fifteen.toFixed(2)}`
        : null,
    ]),
    status: "ok",
  };
}

// ---------------------------------------------------------------- 硬盘

export function diskReadout(
  volume: VolumeInfo | null,
  reclaimable: number | null,
  known = true,
): Readout {
  if (!known) return pending("读取中…");
  if (!volume || volume.totalBytes <= 0) {
    return { ...pending("容量不可读（statvfs 失败）"), status: "unavailable" };
  }

  const used = Math.max(0, volume.totalBytes - volume.userAvailableBytes);
  const ratio = used / volume.totalBytes;
  const usedPart = splitBytes(used);
  const totalPart = splitBytes(volume.totalBytes);

  return {
    main: usedPart.value,
    unit: usedPart.unit,
    mainTail: `/ ${totalPart.value} ${totalPart.unit}`,
    ratio,
    tone: loadTone(ratio),
    sub: compact([
      // 可回收来自最近一次存储扫描，没扫过就整条不显示——不写「0」
      reclaimable != null ? `可回收 ${formatBytes(reclaimable)}` : null,
      `可用 ${formatBytes(volume.userAvailableBytes)}`,
    ]),
    status: "ok",
  };
}

// ---------------------------------------------------------------- 端口

export function portReadout(ports: PortSnapshot | null): Readout {
  if (!ports) {
    return { ...pending("尚未扫描"), status: "unavailable", note: "进入本页会自动扫一轮" };
  }

  const entries = ports.entries;
  const watched = entries.filter((e) => e.watched).length;
  const exposed = entries.filter((e) => e.exposure === "any").length;
  const conns = Object.values(ports.connsByPid).reduce((sum, c) => sum + c.established, 0);
  const closeWait = Object.values(ports.connsByPid).reduce((sum, c) => sum + c.closeWait, 0);

  return {
    main: String(entries.length),
    unit: "个监听",
    ratio: null,
    tone: "neutral",
    sub: compact([
      `关注 ${watched}`,
      // 暴露面 > 0 才提；为 0 时不必占一行
      exposed > 0 ? `对外暴露 ${exposed}` : null,
      `活跃连接 ${conns}`,
      closeWait > 0 ? `CLOSE_WAIT ${closeWait}` : null,
    ]),
    status: "ok",
  };
}

// ---------------------------------------------------------------- 网络

export function netReadout(
  sample: NetTotalSample | null,
  history: NetTotalPoint[],
): NetReadout {
  const downSeries = history.map((p) => p.downBps);
  const upSeries = history.map((p) => p.upBps);
  const avg = weightedAverage(history);
  const base = {
    downBps: sample?.downBps ?? 0,
    upBps: sample?.upBps ?? 0,
    downSeries,
    upSeries,
    avgDownText: `↓ ${formatSpeed(avg.downBps)}`,
    avgUpText: `↑ ${formatSpeed(avg.upBps)}`,
  };

  if (!sample) return { ...base, ...pending("采样中…") };
  if (sample.status === "warmup") {
    return { ...base, ...pending("采样中…（首拍只建基线，约 2 s）") };
  }
  if (sample.status === "unavailable") {
    return {
      ...base,
      main: "—",
      unit: "",
      ratio: null,
      sub: [],
      status: "unavailable",
      tone: "neutral",
      note: sample.note ?? "系统网卡计数器读不到",
    };
  }

  const down = splitSpeed(sample.downBps);
  return {
    ...base,
    main: down.value,
    unit: down.unit,
    ratio: null,
    tone: "accent",
    status: "ok",
    sub: compact([
      `近 5 min ${base.avgDownText} / ${base.avgUpText}`,
      sample.today
        ? `今日 ↓ ${formatBytes(sample.today.downBytes)} ↑ ${formatBytes(sample.today.upBytes)}`
        : null,
      sample.peakDownBps > 0 ? `峰值 ↓ ${formatSpeed(sample.peakDownBps)}` : null,
    ]),
  };
}

// ---------------------------------------------------------------- 方向读数（M16 v6）

/**
 * 单个方向的完整读数：总览页「下载 / 上传」两卡与面板两行共用，
 * 口径仍只在这里定义一次。输入是已有数据源，零新增 IPC、零新增采样。
 */
export function netDirectionReadout(
  sample: NetTotalSample | null,
  history: NetTotalPoint[],
  dir: NetDirection,
): NetDirectionReadout {
  const down = dir === "down";
  const series = history.map((p) => (down ? p.downBps : p.upBps));
  const avg = weightedAverage(history);
  const avgText = down ? formatSpeed(avg.downBps) : formatSpeed(avg.upBps);
  const bps = down ? (sample?.downBps ?? 0) : (sample?.upBps ?? 0);
  const today = sample?.today ?? null;
  const todayText = today
    ? formatBytes(down ? today.downBytes : today.upBytes)
    : null;
  const peakBps = down ? (sample?.peakDownBps ?? 0) : (sample?.peakUpBps ?? 0);
  const sessionBytes = down
    ? sample?.sessionDownBytes
    : sample?.sessionUpBytes;

  const make = (
    main: string,
    unit: string,
    status: ReadoutStatus,
    note?: string,
  ): NetDirectionReadout => ({
    dir,
    bps,
    main,
    unit,
    series,
    avgText,
    todayText,
    peakText: status === "ok" && peakBps > 0 ? formatSpeed(peakBps) : null,
    sessionText: sessionBytes != null ? formatBytes(sessionBytes) : null,
    status,
    note,
  });

  if (!sample) return make("—", "", "warmup", "采样中…");
  if (sample.status === "warmup") {
    return make("—", "", "warmup", "采样中…（首拍只建基线，约 2 s）");
  }
  if (sample.status === "unavailable") {
    // 读不到 ≠ 没有流量：不显示 0，main 用「—」+ 原因
    return make("—", "", "unavailable", sample.note ?? "系统网卡计数器读不到");
  }
  const speed = splitSpeed(bps);
  return make(speed.value, speed.unit, "ok");
}

// ---------------------------------------------------------------- 共用小工具

/** 占比条色调：满/紧张才变色，正常占用不制造焦虑 */
export function loadTone(ratio: number): Tone {
  if (ratio >= 0.9) return "danger";
  if (ratio >= 0.75) return "warn";
  return "accent";
}

/** 速率拆成「数值 + 单位」：主读数让数字自己占位，单位小一号 */
export function splitSpeed(bps: number): { value: string; unit: string } {
  if (!Number.isFinite(bps) || bps < 1) return { value: "0", unit: "B/s" };
  const units = ["B/s", "KB/s", "MB/s", "GB/s"];
  let v = bps;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return { value: v >= 100 ? String(Math.round(v)) : v.toFixed(1), unit: units[i] };
}

function pending(note: string): Readout {
  return { main: "—", unit: "", ratio: null, sub: [], status: "warmup", tone: "neutral", note };
}

function pressureTag(level: PressureLevel): { text: string; tone: Tone } {
  switch (level) {
    case "critical":
      return { text: "紧张", tone: "danger" };
    case "warning":
      return { text: "警告", tone: "warn" };
    default:
      return { text: "正常", tone: "accent" };
  }
}

function compact(list: Array<string | null>): string[] {
  return list.filter((v): v is string => v != null && v !== "");
}
