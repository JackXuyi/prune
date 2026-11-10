import { useEffect } from "react";

import { EmptyState } from "./EmptyArt";
import { RefreshIcon } from "./icons";
import { formatClock } from "../lib/format";
import { phaseText, useLanStore } from "../stores/lanStore";
import { kindGlyph, kindLabel, type LanDevice } from "../types/lan";

/**
 * 局域网设备分段（M18-C5，spec §11）：挂在「端口」tab 内。
 *
 * 三种状态 + 一条铁律：**进入分段不扫描**。macOS 15 起桌面 App 纳入本地网络
 * 授权，这是 Prune 第一次引入系统权限弹窗——只允许在用户点「扫描局域网」时
 * 触发，绝不在切 tab 时蹦出来。边界写进按钮下面的说明，不藏进设置。
 */
export function LanBoard() {
  const snapshot = useLanStore((s) => s.snapshot);
  const scanning = useLanStore((s) => s.scanning);
  const progress = useLanStore((s) => s.progress);
  const error = useLanStore((s) => s.error);
  const init = useLanStore((s) => s.init);
  const scan = useLanStore((s) => s.scan);

  useEffect(() => {
    void init();
  }, [init]);

  if (scanning) {
    const pct = progress && progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : 0;
    return (
      <div className="mt-6">
        <p className="text-[11.5px] text-ink-dim">{phaseText(progress)}</p>
        <div className="mt-2 h-1 w-full max-w-[420px] overflow-hidden rounded-full bg-white/[0.06]">
          <div
            className="h-full rounded-full bg-accent/70 transition-[width] duration-300"
            style={{ width: `${pct}%` }}
          />
        </div>
        <p className="mt-2 text-[10.5px] text-ink-faint">
          一轮约 30 秒（mDNS 发现窗口）。期间不会访问任何设备上的服务。
        </p>
      </div>
    );
  }

  if (!snapshot) {
    return (
      <EmptyState
        title="还没有扫描过局域网"
        note="只探测本网段设备（TCP connect + mDNS / SSDP），不访问设备上的任何服务。首次扫描 macOS 会请求本地网络授权。"
        action={
          <div className="mt-1 flex flex-col items-center gap-1.5">
            <button
              type="button"
              onClick={() => void scan()}
              className="flex items-center gap-1.5 rounded-full bg-accent/15 px-3.5 py-1.5 text-[11.5px] font-medium text-accent transition-colors hover:bg-accent/25"
            >
              扫描局域网
            </button>
          </div>
        }
      />
    );
  }

  return (
    <div className="mt-4">
      {/* 摘要条（spec §11：一行小字） */}
      <div className="flex items-center justify-between gap-3">
        <p className="text-[11px] text-ink-dim">
          {snapshot.cidr ?? "网段未知"}
          {snapshot.iface ? ` · ${snapshot.iface}` : ""} ·{" "}
          {snapshot.devices.filter((d) => d.online).length} 台在线
          {snapshot.scanned > 0 ? ` · 探测 ${snapshot.scanned}/${snapshot.total}` : " · 只读邻居表"}
          {snapshot.exposedPorts.length > 0
            ? ` · 我对这些设备开放 ${snapshot.exposedPorts.length} 个端口（${snapshot.exposedPorts.slice(0, 6).join(" / ")}${snapshot.exposedPorts.length > 6 ? " …" : ""}）`
            : " · 我对同网段开放 0 个端口"}
        </p>
        <button
          type="button"
          onClick={() => void scan()}
          title="重新扫描一轮（约 30 秒）"
          className="flex shrink-0 items-center gap-1 rounded-full bg-white/[0.05] px-2.5 py-1 text-[10.5px] text-ink-dim transition-colors hover:bg-white/[0.09] hover:text-ink"
        >
          <RefreshIcon size={10} /> 重扫
        </button>
      </div>

      {error ? (
        <p className="mt-2 rounded-lg bg-danger/[0.08] px-3 py-2 text-[11px] text-danger">{error}</p>
      ) : null}
      {snapshot.notes.map((n) => (
        <p
          key={n}
          className="mt-2 rounded-lg bg-warn/[0.07] px-3 py-2 text-[11px] leading-relaxed text-warn"
        >
          {n}
        </p>
      ))}

      {snapshot.devices.length === 0 ? (
        <EmptyState
          title="本轮没有发现设备"
          note="探测完成才是这个结论——不是没扫。如果邻居表为空，说明本机最近没有与网段内设备通信过。"
        />
      ) : (
        <>
          <div className="mt-3 grid grid-cols-[150px_minmax(0,1fr)_130px_74px] items-center gap-2.5 border-b border-hairline-soft px-2 pb-2 text-[10px] text-ink-faint">
            <span>地址</span>
            <span>设备</span>
            <span>MAC</span>
            <span className="text-right">状态</span>
          </div>
          <ul>
            {snapshot.devices.map((d) => (
              <DeviceRow key={d.ip} d={d} />
            ))}
          </ul>
          <p className="mt-3 text-[10px] text-ink-faint">
            {snapshot.devices.length} 台设备 · 扫描 {(snapshot.scanMs / 1000).toFixed(1)} s ·{" "}
            {formatClock(snapshot.at)} · 本轮快照语义：跨轮次的「同一台设备」以 IP 为准（随机
            MAC 换网就换，聚合键不做 MAC 承诺）
          </p>
        </>
      )}
    </div>
  );
}

function DeviceRow({ d }: { d: LanDevice }) {
  const role = d.isSelf
    ? "本机"
    : d.isGateway
      ? "网关"
      : kindLabel(d.kind);
  const state = !d.online
    ? "不可达"
    : d.state === "stale"
      ? "最近在线"
      : "在线";
  // 随机 MAC → 厂商列整体省略（spec §11 字段取舍），加 `· 随机` 小标
  const identity = d.randomizedMac
    ? null
    : (d.vendor ?? null);
  const hostname = d.hostname?.replace(/\.local\.?$/, "") ?? null;
  return (
    <li>
      <div className="grid grid-cols-[150px_minmax(0,1fr)_130px_74px] items-center gap-2.5 rounded-md px-2 py-1.5 transition-colors hover:bg-white/[0.04]">
        <span className="flex items-center gap-2 font-mono text-[12px] tabular-nums text-ink">
          <span aria-hidden className="text-center text-[11px] text-ink-faint">
            {kindGlyph(d.kind)}
          </span>
          {d.ip}
        </span>
        <span className="flex min-w-0 items-baseline gap-2">
          <span className="text-[11.5px] text-ink-dim">{role}</span>
          {hostname ? (
            <span className="truncate text-[10.5px] text-ink-faint">{hostname}</span>
          ) : null}
          {d.inbound.length > 0 ? (
            <span className="ml-auto shrink-0 rounded-full bg-accent/[0.12] px-2 py-0.5 text-[10px] font-medium text-accent">
              连了我 {d.inbound.slice(0, 3).join(" / ")}
              {d.inbound.length > 3 ? " …" : ""}
            </span>
          ) : null}
        </span>
        <span className="font-mono text-[10.5px] tabular-nums text-ink-faint">
          {d.mac ?? "—"}
          {d.randomizedMac ? <span className="ml-1 font-sans">· 随机</span> : null}
        </span>
        <span className="text-right text-[10.5px] text-ink-faint">
          {state}
          {identity ? (
            <span className="ml-1 text-ink-dim">{identity}</span>
          ) : null}
        </span>
      </div>
    </li>
  );
}
