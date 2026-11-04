import { useState } from "react";

import { devAppDisk } from "../lib/devFixture";
import { appDisk, isTauri } from "../lib/ipc";
import { formatBytes, formatCpu } from "../lib/format";
import {
  BUNDLE_READONLY_HINT,
  CACHE_CLEAN_HINT,
  DATA_READONLY_HINT,
  diskEmpty,
  diskTotal,
  type AppDisk,
  type DiskLeaf,
} from "../types/appdisk";
import { connsOfPids, isSystemProcess, type ConnStats, type PortSnapshot } from "../types/net";
import { totalOf, type FamilyMem, type ProcMem } from "../types/mem";

/** 一格数据：标签在上、值在下。没有值就是「—」，不是 0 */
function Cell({
  label,
  value,
  hint,
  tone,
}: {
  label: string;
  value: string;
  hint?: string;
  tone?: "warn";
}) {
  return (
    <div title={hint} className="min-w-0">
      <div className="text-[9.5px] leading-tight text-ink-faint">{label}</div>
      <div
        className={`mt-0.5 truncate font-mono text-[12px] leading-tight tabular-nums ${
          tone === "warn" ? "text-warn" : "text-ink"
        }`}
      >
        {value}
      </div>
    </div>
  );
}

/** 磁盘一层：名称 + 体积 + 口径提示 */
function DiskRow({ label, leaf, hint }: { label: string; leaf: DiskLeaf | null; hint: string }) {
  return (
    <div className="flex items-baseline gap-2" title={hint}>
      <span className="w-8 shrink-0 text-[10px] text-ink-faint">{label}</span>
      <span className="min-w-0 flex-1 truncate font-mono text-[10px] text-ink-faint/70">
        {leaf ? leaf.path : "—"}
      </span>
      <span className="shrink-0 font-mono text-[11px] tabular-nums text-ink-dim">
        {leaf ? `${leaf.partial ? "≥ " : ""}${formatBytes(leaf.bytes)}` : "—"}
      </span>
    </div>
  );
}

/**
 * 家族资源卡（M8）：把一个应用的五个资源放在一张卡上。
 *
 * 数据来源与口径：
 * - 进程 / 内存 / CPU：来自内存快照（CPU 为单核口径，部分成员未知时是下界）
 * - 端口：M7 关注层口径（内部 IPC 随机端口不计入，否则每行都是噪音）
 * - 连接：M7 同一次 lsof 按 pid 聚合（ESTABLISHED 才算「活跃连接」）
 * - 磁盘：三层按需测量（不点就不 du），TTL 30 min
 *
 * **卡上没有任何删除按钮**：清理归「存储」域的白名单管，
 * 这里跨域放按钮迟早绕过白名单（方案 §7.2 / §11）。
 */
export function AppResourceCard({
  family,
  members,
  ports,
  conns,
}: {
  family: FamilyMem;
  /** 该家族的全部进程（用来取连接数与代表 exe） */
  members: ProcMem[];
  ports?: PortSnapshot | null;
  /** 由调用方算好的连接汇总；不传则从 ports 里自己算 */
  conns?: ConnStats;
}) {
  const [disk, setDisk] = useState<AppDisk | null>(null);
  const [measuring, setMeasuring] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const pids = members.map((p) => p.pid);
  const conn = conns ?? connsOfPids(ports?.connsByPid, pids);

  // 关注层端口数：只算 watched，与列表徽标同口径
  const familyPorts = (ports?.entries ?? []).filter(
    (e) => e.watched && e.listeners.some((b) => b.pid != null && pids.includes(b.pid)),
  );
  const exposed = familyPorts.filter((e) => e.exposure === "any").length;

  // 代表 exe：GUI 应用任一成员都在同一个 .app 内，取第一个有路径的
  const exe = members.find((p) => p.exe)?.exe ?? null;

  const cpuText =
    family.cpu == null
      ? "—"
      : `${formatCpu(family.cpu)}${(family.cpuCovered ?? 0) < family.processCount ? "（下界）" : ""}`;

  async function measure(force: boolean) {
    setMeasuring(true);
    setError(null);
    try {
      if (!isTauri()) {
        // 浏览器走查用样本：真实测量需要桌面端（du 是本机能力）
        setDisk(devAppDisk(family.name));
        return;
      }
      setDisk(await appDisk(family.name, exe, force));
    } catch (e) {
      setError(String(e));
    } finally {
      setMeasuring(false);
    }
  }

  const sysHint = isSystemProcess(family.name)
    ? "系统进程，不做结束与清理建议"
    : undefined;

  return (
    <div className="mt-1 mb-1.5 rounded-lg border border-hairline-soft bg-white/[0.02] px-3 py-2.5">
      <div className="grid grid-cols-5 gap-2">
        <Cell label="进程" value={String(family.processCount)} />
        <Cell
          label="内存"
          value={`${family.footprint == null ? "" : "≈"}${formatBytes(totalOf(family))}`}
          hint="footprint 口径，含被换出到磁盘的部分；未深探的子进程按 RSS 计入（下界）"
        />
        <Cell
          label="CPU"
          value={cpuText}
          hint="单核口径，可超过 100%；部分成员未知时是下界（不是 0）"
        />
        <Cell
          label="端口"
          value={familyPorts.length === 0 ? "—" : `${familyPorts.length}${exposed > 0 ? ` · ${exposed} 对外` : ""}`}
          hint="只统计关注层端口；Electron 的内部 IPC 随机端口不计入"
          tone={exposed > 0 ? "warn" : undefined}
        />
        <Cell
          label="连接"
          value={conn.established === 0 ? "—" : `${conn.established} 活跃`}
          hint="ESTABLISHED 连接数。连接多不等于流量大——流量口径本机没有公开 API"
        />
      </div>

      {conn.closeWait > 0 ? (
        <p className="mt-1.5 text-[10px] leading-relaxed text-warn">
          有 {conn.closeWait} 条连接处于 CLOSE_WAIT：对端已关、本端还没释放。
          这可能是泄漏，也可能只是慢关闭——按曲线看更准。
        </p>
      ) : null}

      <div className="mt-2.5 border-t border-hairline-soft pt-2">
        {disk == null ? (
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => void measure(false)}
              disabled={measuring}
              className="rounded-md bg-white/[0.05] px-2 py-1 text-[10.5px] text-ink-dim transition-colors hover:bg-white/[0.09] hover:text-ink disabled:opacity-50"
            >
              {measuring ? "测量中…" : "测量磁盘占用"}
            </button>
            <span className="text-[10px] text-ink-faint">
              只读，需现算（应用越大越久，最多 4 秒/层）
            </span>
          </div>
        ) : (
          <div>
            <div className="space-y-1">
              <DiskRow label="本体" leaf={disk.bundle} hint={BUNDLE_READONLY_HINT} />
              <DiskRow label="缓存" leaf={disk.cache} hint={CACHE_CLEAN_HINT} />
              <DiskRow label="数据" leaf={disk.data} hint={DATA_READONLY_HINT} />
            </div>
            {diskEmpty(disk) ? (
              <p className="mt-1.5 text-[10px] leading-relaxed text-ink-faint">
                {disk.note ?? "没有可归属的磁盘目录"}
              </p>
            ) : (
              <div className="mt-1.5 flex items-baseline justify-between gap-2">
                <span className="min-w-0 flex-1 text-[9.5px] leading-snug text-ink-faint">
                  合计 {formatBytes(diskTotal(disk))} · 按文件实占块统计，含 APFS 克隆共享部分，
                  不等于可释放空间
                </span>
                <button
                  type="button"
                  onClick={() => void measure(true)}
                  disabled={measuring}
                  className="shrink-0 text-[9.5px] text-accent hover:underline disabled:opacity-50"
                >
                  {measuring ? "测量中…" : "重新测量"}
                </button>
              </div>
            )}
          </div>
        )}
        {error ? <p className="mt-1.5 text-[10px] text-warn">{error}</p> : null}
        {sysHint ? (
          <p className="mt-1.5 text-[10px] text-ink-faint">{sysHint}</p>
        ) : null}
      </div>
    </div>
  );
}
