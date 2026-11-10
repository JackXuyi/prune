import { useEffect, useState } from "react";

import { EmptyState } from "./EmptyArt";
import { LanDeviceCard, LanDeviceDetailModal } from "./LanDeviceCard";
import { ChevronRightIcon, RefreshIcon } from "./icons";
import { formatClock } from "../lib/format";
import { phaseText, useLanStore } from "../stores/lanStore";
import type { LanDevice } from "../types/lan";

/**
 * 局域网设备分段（M18-C5，spec §11）：挂在「端口」tab 内。
 * 卡片化重设计：m18-lan-cards-redesign-plan.md。
 *
 * 两种状态 + 一条铁律：**进入分段不扫描**。macOS 15 起桌面 App 纳入本地网络
 * 授权，这是 Prune 第一次引入系统权限弹窗——只允许在用户点「扫描局域网」时
 * 触发，绝不在切 tab 时蹦出来。边界写进按钮下面的说明，不藏进设置。
 */

type LanFilter = "all" | "online" | "inbound";

export function LanBoard() {
  const snapshot = useLanStore((s) => s.snapshot);
  const scanning = useLanStore((s) => s.scanning);
  const progress = useLanStore((s) => s.progress);
  const error = useLanStore((s) => s.error);
  const init = useLanStore((s) => s.init);
  const scan = useLanStore((s) => s.scan);
  const [filter, setFilter] = useState<LanFilter>("all");
  const [showOffline, setShowOffline] = useState(false);
  const [selected, setSelected] = useState<LanDevice | null>(null);

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

  const devices = snapshot.devices;
  const onlineCount = devices.filter((d) => d.online).length;
  const inboundCount = devices.filter((d) => d.inbound.length > 0).length;
  const randomMacCount = devices.filter((d) => d.randomizedMac).length;

  // 分组：本机与网关置顶固定 → 在线设备网格 → 离线折叠（plan §3 信息架构）
  const match =
    filter === "all"
      ? () => true
      : filter === "online"
        ? (d: LanDevice) => d.online
        : (d: LanDevice) => d.inbound.length > 0;
  const pinned = devices.filter((d) => (d.isSelf || d.isGateway) && match(d));
  const onlineOthers = devices.filter(
    (d) => d.online && !d.isSelf && !d.isGateway && match(d),
  );
  const offline = devices.filter((d) => !d.online && match(d));
  const connectedMe = devices.filter((d) => d.inbound.length > 0);

  const exposed = snapshot.exposedPorts;
  const exposedHint =
    exposed.length > 0
      ? `${exposed.slice(0, 4).join(" / ")}${exposed.length > 4 ? " …" : ""}`
      : "我对同网段开放 0 个端口";

  return (
    <div className="mt-4">
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

      {devices.length === 0 ? (
        <EmptyState
          title="本轮没有发现设备"
          note="探测完成才是这个结论——不是没扫。如果邻居表为空，说明本机最近没有与网段内设备通信过。"
        />
      ) : (
        <>
          {/* 筛选条 + 重扫（plan §5：单选 chips，与端口页搜索语义独立） */}
          <div className="mt-3 flex items-center justify-between gap-3">
            <div className="flex items-center gap-1" role="tablist" aria-label="设备筛选">
              <FilterChip
                active={filter === "all"}
                onClick={() => setFilter("all")}
                label="全部"
                count={devices.length}
              />
              <FilterChip
                active={filter === "online"}
                onClick={() => setFilter("online")}
                label="在线"
                count={onlineCount}
              />
              <FilterChip
                active={filter === "inbound"}
                onClick={() => setFilter("inbound")}
                label="连了我"
                count={inboundCount}
                dim={inboundCount === 0}
              />
            </div>
            <button
              type="button"
              onClick={() => void scan()}
              title="重新扫描一轮（约 30 秒）"
              className="flex shrink-0 items-center gap-1 rounded-full bg-white/[0.05] px-2.5 py-1 text-[10.5px] text-ink-dim transition-colors hover:bg-white/[0.09] hover:text-ink"
            >
              <RefreshIcon size={10} /> 重扫
            </button>
          </div>

          {/* 指标卡 ×3（plan §3：核心安全信号从摘要行提权） */}
          <div className="mt-3 grid grid-cols-3 gap-2.5">
            <MetricCard
              label="在线设备"
              value={String(onlineCount)}
              unit={`/ ${devices.length} 台`}
              hint={
                snapshot.scanned > 0
                  ? `探测 ${snapshot.scanned}/${snapshot.total}`
                  : "只读邻居表"
              }
            />
            <MetricCard
              label="对我开放的端口"
              value={String(exposed.length)}
              unit="个"
              tone={exposed.length > 0 ? "warn" : undefined}
              hint={exposedHint}
            />
            <MetricCard
              label="扫描耗时"
              value={(snapshot.scanMs / 1000).toFixed(1)}
              unit="s"
              hint={formatClock(snapshot.at)}
            />
          </div>

          {randomMacCount > 0 ? (
            <p className="mt-2 rounded-lg bg-white/[0.03] px-3 py-2 text-[10.5px] leading-relaxed text-ink-dim">
              {randomMacCount} 台设备使用随机化 MAC——无法识别厂商，跨轮次聚合以
              IP 为准。
            </p>
          ) : null}

          {filter === "inbound" ? (
            connectedMe.length > 0 ? (
              <DeviceSection title="连了我的设备" devices={connectedMe} onSelect={setSelected} />
            ) : (
              <EmptyState
                title="没有设备主动连过我"
                note="入站连接（ESTABLISHED 且对端在网段内）为零。可能只是没有长期服务与这台机器保持连接——不代表本轮扫描没跑。"
              />
            )
          ) : (
            <>
              <DeviceSection title="本机与网关" devices={pinned} onSelect={setSelected} />
              <DeviceSection title="在线设备" devices={onlineOthers} onSelect={setSelected} />
              {filter === "online" && onlineCount === 0 ? (
                <EmptyState
                  title="没有在线设备"
                  note="扫描过了才是这个结论——邻居表里只有离线记录。"
                />
              ) : null}
              {offline.length > 0 ? (
                <div className="mt-3.5">
                  <button
                    type="button"
                    onClick={() => setShowOffline((v) => !v)}
                    className="flex w-full items-center gap-1.5 rounded-[10px] border border-dashed border-hairline px-3 py-2 text-left text-[10.5px] text-ink-faint transition-colors hover:text-ink-dim"
                  >
                    <ChevronRightIcon
                      size={11}
                      className={`shrink-0 transition-transform ${showOffline ? "rotate-90" : ""}`}
                    />
                    已离线 {offline.length} 台
                    <span className="text-[9.5px] text-ink-faint/80">
                      点击展开 · 置灰展示，不占用首屏
                    </span>
                  </button>
                  {showOffline ? (
                    <div
                      className="mt-2 grid gap-2.5"
                      style={{
                        gridTemplateColumns: "repeat(auto-fill, minmax(330px, 1fr))",
                      }}
                    >
                      {offline.map((d) => (
                        <LanDeviceCard
                          key={d.ip}
                          d={d}
                          dimmed
                          onSelect={setSelected}
                        />
                      ))}
                    </div>
                  ) : null}
                </div>
              ) : null}
            </>
          )}

          <p className="mt-3.5 text-[10px] text-ink-faint">
            {devices.length} 台设备 · 扫描 {(snapshot.scanMs / 1000).toFixed(1)} s ·{" "}
            {formatClock(snapshot.at)} · 本轮快照语义：跨轮次的「同一台设备」以 IP 为准（随机
            MAC 换网就换，聚合键不做 MAC 承诺）
          </p>
        </>
      )}

      {selected ? (
        <LanDeviceDetailModal d={selected} onClose={() => setSelected(null)} />
      ) : null}
    </div>
  );
}

function FilterChip({
  active,
  label,
  count,
  dim = false,
  onClick,
}: {
  active: boolean;
  label: string;
  count: number;
  dim?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      onClick={onClick}
      className={`rounded-full px-2.5 py-1 text-[10.5px] transition-colors ${
        active
          ? "bg-accent/[0.14] font-medium text-accent"
          : dim
            ? "text-ink-faint/60 hover:text-ink-dim"
            : "text-ink-faint hover:bg-white/[0.05] hover:text-ink-dim"
      }`}
    >
      {label} <span className="tabular-nums">{count}</span>
    </button>
  );
}

function MetricCard({
  label,
  value,
  unit,
  hint,
  tone,
}: {
  label: string;
  value: string;
  unit?: string;
  hint?: string;
  tone?: "warn";
}) {
  return (
    <div className="rounded-[10px] border border-hairline-soft bg-white/[0.03] px-3 py-2">
      <div className="text-[10px] text-ink-faint">{label}</div>
      <div className="mt-1 flex items-baseline gap-1">
        <span
          className={`font-mono text-[20px] leading-none tabular-nums ${
            tone === "warn" ? "text-warn" : "text-ink"
          }`}
        >
          {value}
        </span>
        {unit ? <span className="text-[10px] text-ink-faint">{unit}</span> : null}
      </div>
      {hint ? (
        <div className="mt-1 truncate text-[9.5px] text-ink-faint" title={hint}>
          {hint}
        </div>
      ) : null}
    </div>
  );
}

function DeviceSection({
  title,
  devices,
  onSelect,
}: {
  title: string;
  devices: LanDevice[];
  onSelect: (d: LanDevice) => void;
}) {
  if (devices.length === 0) return null;
  return (
    <div className="mt-3.5">
      <p className="text-[10.5px] text-ink-faint">
        {title} · {devices.length}
      </p>
      <div
        className="mt-2 grid gap-2.5"
        style={{ gridTemplateColumns: "repeat(auto-fill, minmax(330px, 1fr))" }}
      >
        {devices.map((d) => (
          <LanDeviceCard key={d.ip} d={d} onSelect={onSelect} />
        ))}
      </div>
    </div>
  );
}
