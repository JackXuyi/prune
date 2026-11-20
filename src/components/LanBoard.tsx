import { useEffect, useState } from "react";

import { EmptyState } from "./EmptyArt";
import { LanDeviceCard, LanDeviceDetailModal } from "./LanDeviceCard";
import { LanHero } from "./lan/LanHero";
import { LanRibbon } from "./lan/LanRibbon";
import { LanTopology } from "./LanTopology";
import { ChevronRightIcon } from "./icons";
import { formatClock } from "../lib/format";
import { useI18n } from "../i18n/store";
import { phaseText, useLanStore } from "../stores/lanStore";
import { kindLabel, type LanDevice } from "../types/lan";

/**
 * 局域网设备分段（M18-C5，spec §11）：挂在「端口」tab 内。
 * eul 重设计：ports-eul-mockup.html 局域网子页 —— 状态带 4 段 → 设备点阵 Hero
 * → 条件带 → 设备网格（工具行 / 固定组 / 在线组 / 离线折叠）。
 *
 * 两种状态 + 一条铁律：**进入分段不扫描**。macOS 15 起桌面 App 纳入本地网络
 * 授权，这是 Prune 第一次引入系统权限弹窗——只允许在用户点「扫描局域网」时
 * 触发，绝不在切 tab 时蹦出来。边界写进按钮下面的说明，不藏进设置。
 */

type LanFilter = "all" | "online" | "inbound";

export function LanBoard({
  query = "",
  /** 头部（状态带 + 点阵 Hero）：设备页由页面级 Ribbon/Hero 承担同源读数，传 false 只留设备区 */
  showHeader = true,
  /** 拓扑视图切换：设备页暂隐藏（2026-11-20 产品决策），端口页保持原样 */
  showTopoToggle = true,
}: {
  query?: string;
  showHeader?: boolean;
  showTopoToggle?: boolean;
}) {
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
  const q = query.trim().toLowerCase();
  // 搜索：IP / 主机名 / 厂商 / 类型词——用户手里只有其中一种线索
  const matchQuery = (d: LanDevice) =>
    !q ||
    d.ip.includes(q) ||
    (d.hostname ?? "").toLowerCase().includes(q) ||
    (d.vendor ?? "").toLowerCase().includes(q) ||
    kindLabel(d.kind, locale).toLowerCase().includes(q);

  const onlineCount = devices.filter((d) => d.online).length;
  const inboundCount = devices.filter((d) => d.inbound.length > 0).length;
  const randomMacCount = devices.filter((d) => d.randomizedMac).length;

  // 分组：本机与网关置顶固定 → 在线设备网格 → 离线折叠（plan §3 信息架构）
  const match = (d: LanDevice) =>
    matchQuery(d) &&
    (filter === "all"
      ? true
      : filter === "online"
        ? d.online
        : d.inbound.length > 0);
  const pinned = devices.filter((d) => (d.isSelf || d.isGateway) && match(d));
  const onlineOthers = devices.filter(
    (d) => d.online && !d.isSelf && !d.isGateway && match(d),
  );
  const offline = devices.filter((d) => !d.online && match(d));
  const connectedMe = devices.filter((d) => match(d) && d.inbound.length > 0);
  const hasMatch = pinned.length + onlineOthers.length + offline.length > 0;

  return (
    <div className="flex flex-col gap-[9px]">
      {error ? (
        <p className="rounded-[10px] bg-danger/[0.08] px-3 py-2 text-[11px] text-danger">{error}</p>
      ) : null}
      {snapshot.notes.map((n) => (
        <p
          key={n}
          className="rounded-[10px] bg-warn/[0.07] px-3 py-[7px] text-[10.5px] leading-relaxed text-warn"
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
          {/* ① 状态带 + ② 设备点阵 Hero：首屏读数区（扫描按钮在 Hero 的口径块里，
              进入分段绝不自动扫描——权限弹窗纪律）。
              设备页（showHeader=false）由页面级 DeviceRibbon/LanHero 承担，这里不重复画 */}
          {showHeader ? <LanRibbon snapshot={snapshot} /> : null}
          {showHeader ? (
            <LanHero snapshot={snapshot} scanning={scanning} onScan={() => void scan()} />
          ) : null}

          {randomMacCount > 0 ? (
            <p className="rounded-[10px] bg-chip px-3 py-[7px] text-[10.5px] leading-relaxed text-ink-dim">
              {t("lan.randomMacNote", { n: randomMacCount })}
            </p>
          ) : null}

          {/* 工具行：筛选 chips（只属于卡片网格——拓扑模式下隐藏，避免「点了没反应」）
              + 卡片/拓扑 视图切换 */}
          <div className="flex items-center justify-between gap-3">
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
            {showTopoToggle ? (
              <div className="flex shrink-0 items-center gap-1" role="tablist" aria-label={t("lan.topo.view")}>
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
          </div>

          {view === "cards" && !hasMatch ? (
            <EmptyState
              art="search"
              title={t("lan.empty.noMatchTitle", { query })}
              note={t("lan.empty.noMatchNote")}
            />
          ) : null}

          {/* 拓扑画布（M20）：全量设备直渲（离线置灰），不做筛选——chips 语义只属于卡片。
              两个视图都保持挂载、用 hidden 切换：卸载会丢掉拖拽后的节点位置（T2 验收） */}
          <div className={view === "cards" ? "hidden" : ""}>
            <LanTopology devices={devices} onSelect={setSelected} />
          </div>
          <div className={view === "topo" || !hasMatch ? "hidden" : ""}>
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
                <div>
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

          <p className="px-1 text-[9px] text-ink-faint">
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
