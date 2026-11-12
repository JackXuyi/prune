import { useEffect, useState } from "react";

import { EmptyState } from "./EmptyArt";
import { LanDeviceCard, LanDeviceDetailModal } from "./LanDeviceCard";
import { LanTopology } from "./LanTopology";
import { ChevronRightIcon, RefreshIcon } from "./icons";
import { formatClock } from "../lib/format";
import { useI18n } from "../i18n/store";
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
  const { t, locale } = useI18n();
  const snapshot = useLanStore((s) => s.snapshot);
  const scanning = useLanStore((s) => s.scanning);
  const progress = useLanStore((s) => s.progress);
  const error = useLanStore((s) => s.error);
  const init = useLanStore((s) => s.init);
  const scan = useLanStore((s) => s.scan);
  const [filter, setFilter] = useState<LanFilter>("all");
  const [showOffline, setShowOffline] = useState(false);
  // 视图二态（M20 spec §1.2）：卡片网格 / 拓扑画布。会话内记忆即可，不落 localStorage
  const [view, setView] = useState<"cards" | "topo">("cards");
  const [selected, setSelected] = useState<LanDevice | null>(null);
  /** 拓扑入口开关（M20-b 视觉优化未落地前先隐藏，代码全保留） */
  const TOPO_ENABLED = false;

  useEffect(() => {
    void init();
  }, [init]);

  if (scanning) {
    const pct = progress && progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : 0;
    return (
      <div className="mt-6">
        {/* spec §4.5：进度必须可见——阶段文案在左、{done}/{total} 在右，绝不只转圈 */}
        <div className="flex w-full max-w-[420px] items-center justify-between">
          <p className="text-[11.5px] text-ink-dim">{phaseText(progress, locale)}</p>
          {progress ? (
            <p className="font-mono text-[9.5px] tabular-nums text-ink-faint">
              {progress.done}/{progress.total}
            </p>
          ) : null}
        </div>
        <div className="mt-2 h-[3px] w-full max-w-[420px] overflow-hidden rounded-full bg-chip">
          <div
            className="h-full rounded-full bg-domain-net transition-[width] duration-300"
            style={{ width: `${pct}%` }}
          />
        </div>
        <p className="mt-2 text-[10.5px] text-ink-faint">{t("lan.scanningNote")}</p>
      </div>
    );
  }

  if (!snapshot) {
    return (
      <EmptyState
        title={t("lan.empty.neverScannedTitle")}
        note={t("lan.empty.neverScannedNote")}
        action={
          <div className="mt-1 flex flex-col items-center gap-1.5">
            <button
              type="button"
              onClick={() => void scan()}
              className="flex items-center gap-1.5 rounded-full bg-accent/15 px-3.5 py-1.5 text-[11.5px] font-medium text-accent transition-colors hover:bg-accent/25"
            >
              {t("lan.scan")}
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
      : t("lan.metric.noExposed");

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
          title={t("lan.empty.noDevicesTitle")}
          note={t("lan.empty.noDevicesNote")}
        />
      ) : (
        <>
          {/* 筛选条 + 视图切换 + 重扫（plan §5：单选 chips，与端口页搜索语义独立）。
              chips 只属于卡片网格——拓扑模式下隐藏，避免「点了没反应」（spec §1.2） */}
          <div className="mt-3 flex items-center justify-between gap-3">
            {view === "cards" ? (
              <div className="flex items-center gap-1" role="tablist" aria-label={t("lan.filter.aria")}>
                <FilterChip
                  active={filter === "all"}
                  onClick={() => setFilter("all")}
                  label={t("lan.filter.all")}
                  count={devices.length}
                />
                <FilterChip
                  active={filter === "online"}
                  onClick={() => setFilter("online")}
                  label={t("lan.filter.online")}
                  count={onlineCount}
                />
                <FilterChip
                  active={filter === "inbound"}
                  onClick={() => setFilter("inbound")}
                  label={t("lan.filter.inbound")}
                  count={inboundCount}
                  dim={inboundCount === 0}
                />
              </div>
            ) : (
              <span />
            )}
            <div className="flex shrink-0 items-center gap-2">
              {/* M20 拓扑入口暂隐藏（视觉优化 M20-b 未落地，先不出入口）。
                  代码全保留：组件 / view state / topo 分支均可直接工作，
                  恢复入口只需把 TOPO_ENABLED 改回 true。 */}
              {TOPO_ENABLED ? (
                <div className="flex items-center gap-1" role="tablist" aria-label={t("lan.topo.view")}>
                  <ViewChip
                    active={view === "cards"}
                    onClick={() => setView("cards")}
                    label={t("lan.topo.cards")}
                  />
                  <ViewChip
                    active={view === "topo"}
                    onClick={() => setView("topo")}
                    label={t("lan.topo.view")}
                  />
                </div>
              ) : null}
              <button
                type="button"
                onClick={() => void scan()}
                title={t("lan.rescanTitle")}
                className="flex shrink-0 items-center gap-1 rounded-full bg-chip px-2.5 py-1 text-[10.5px] text-ink-dim transition-colors hover:bg-chip-hover hover:text-ink"
              >
                <RefreshIcon size={10} /> {t("lan.rescan")}
              </button>
            </div>
          </div>

          {/* 指标卡 ×3（plan §3：核心安全信号从摘要行提权） */}
          <div className="mt-3 grid grid-cols-3 gap-2.5">
            <MetricCard
              label={t("lan.metric.online")}
              value={String(onlineCount)}
              unit={t("lan.metric.onlineUnit", { n: devices.length })}
              hint={
                snapshot.scanned > 0
                  ? t("lan.metric.probed", {
                      scanned: snapshot.scanned,
                      total: snapshot.total,
                    })
                  : t("lan.metric.neighborsOnly")
              }
            />
            <MetricCard
              label={t("lan.metric.exposed")}
              value={String(exposed.length)}
              unit={t("lan.metric.exposedUnit")}
              tone={exposed.length > 0 ? "warn" : undefined}
              hint={exposedHint}
            />
            <MetricCard
              label={t("lan.metric.elapsed")}
              value={(snapshot.scanMs / 1000).toFixed(1)}
              unit={t("lan.metric.elapsedUnit")}
              hint={formatClock(snapshot.at)}
            />
          </div>

          {randomMacCount > 0 ? (
            <p className="mt-2 rounded-lg bg-chip px-3 py-2 text-[10.5px] leading-relaxed text-ink-dim">
              {t("lan.randomMacNote", { n: randomMacCount })}
            </p>
          ) : null}

          {/* 拓扑画布（M20）：全量设备直渲（离线置灰），不做筛选——chips 语义只属于卡片。
              两个视图都保持挂载、用 hidden 切换：卸载会丢掉拖拽后的节点位置（T2 验收） */}
          <div className={view === "cards" ? "hidden" : ""}>
            <LanTopology devices={devices} onSelect={setSelected} />
          </div>
          <div className={view === "topo" ? "hidden" : ""}>
            {filter === "inbound" ? (
            connectedMe.length > 0 ? (
              <DeviceSection
                title={t("lan.section.inbound")}
                devices={connectedMe}
                at={snapshot.at}
                onSelect={setSelected}
              />
            ) : (
              <EmptyState
                title={t("lan.empty.noInboundTitle")}
                note={t("lan.empty.noInboundNote")}
              />
            )
          ) : (
            <>
              <DeviceSection
                title={t("lan.section.pinned")}
                devices={pinned}
                at={snapshot.at}
                onSelect={setSelected}
              />
              <DeviceSection
                title={t("lan.section.online")}
                devices={onlineOthers}
                at={snapshot.at}
                onSelect={setSelected}
              />
              {filter === "online" && onlineCount === 0 ? (
                <EmptyState
                  title={t("lan.empty.noOnlineTitle")}
                  note={t("lan.empty.noOnlineNote")}
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
                    {t("lan.offline.toggle", { n: offline.length })}
                    <span className="text-[9.5px] text-ink-faint/80">
                      {t("lan.offline.hint")}
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
                          at={snapshot.at}
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
          </div>

          <p className="mt-3.5 text-[10px] text-ink-faint">
            {t("lan.footer", {
              devices: devices.length,
              seconds: (snapshot.scanMs / 1000).toFixed(1),
              clock: formatClock(snapshot.at),
            })}
          </p>
        </>
      )}

      {selected ? (
        <LanDeviceDetailModal
          d={selected}
          at={snapshot.at}
          onClose={() => setSelected(null)}
        />
      ) : null}
    </div>
  );
}

/** 视图二态切换 chip（M20 spec §1.2）：与 FilterChip 同视觉语言，无计数 */
function ViewChip({
  active,
  label,
  onClick,
}: {
  active: boolean;
  label: string;
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
          : "text-ink-faint hover:bg-chip hover:text-ink-dim"
      }`}
    >
      {label}
    </button>
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
            : "text-ink-faint hover:bg-chip hover:text-ink-dim"
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
    <div className="rounded-[10px] border border-hairline-soft bg-chip px-3 py-2">
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
  at,
  onSelect,
}: {
  title: string;
  devices: LanDevice[];
  /** 快照时刻（Unix 秒）：透传给卡片与详情弹窗做相对时间锚（M19 spec §6.1） */
  at: number;
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
          <LanDeviceCard key={d.ip} d={d} at={at} onSelect={onSelect} />
        ))}
      </div>
    </div>
  );
}
