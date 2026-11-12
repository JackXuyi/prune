import { useEffect, useState } from "react";

import { CopyIcon, DeviceKindIcon } from "./icons";
import { writeClipboard } from "../lib/clipboard";
import { formatAgo, formatDayClock } from "../lib/format";
import { useI18n } from "../i18n/store";
import { kindLabel, type LanDevice } from "../types/lan";

/**
 * 局域网设备卡（m18-lan-cards-redesign-plan.md §4 / m19-lan-device-times-spec.md §6）：
 * 图标 tile（域色 14% 底）+ 状态点 + IP mono 徽标 + 「连了我」warn 徽标
 * + services 标签 + 「活跃 x 前」时间行；点击打开设备详情弹窗。
 *
 * 展示原则：hostname 命中 → 设备名为主标题；查不到 → 回退类型名，不编造。
 * 「连了我」命中时卡片加 warn 描边，安全信号前置。
 * 时间线：卡片只显示「活跃 x 前」，加入时间只进详情弹窗（M19 已确认决策）。
 */
/**
 * 设备身份的展示三件套（主标题 / 副标题 / 状态）。
 *
 * 卡片与详情弹窗共用同一套推导：两处各写一遍必然漂移，而这里恰恰是
 * 「查不到就回退类型名、不编造」这条规则的唯一落点。
 */
function useIdentity(d: LanDevice) {
  const { t, locale } = useI18n();
  const hostname = d.hostname?.replace(/\.local\.?$/, "") ?? null;
  const title = d.isSelf
    ? t("lan.card.self")
    : d.isGateway
      ? t("lan.card.gateway")
      : (hostname ?? kindLabel(d.kind, locale));
  const subtitle = d.vendor ?? (d.randomizedMac ? t("lan.card.randomMac") : null);
  const state = !d.online
    ? t("lan.card.stateOffline")
    : d.state === "stale"
      ? t("lan.card.stateStale")
      : t("lan.card.stateOnline");
  const dot = !d.online ? "bg-ink-faint" : d.state === "stale" ? "bg-warn" : "bg-accent";
  return { t, locale, hostname, title, subtitle, state, dot };
}

export function LanDeviceCard({
  d,
  at,
  dimmed = false,
  onSelect,
}: {
  d: LanDevice;
  /** 快照时刻（Unix 秒）：相对时间锚定它而非墙钟（M19 spec 决策 3） */
  at: number;
  dimmed?: boolean;
  onSelect: (d: LanDevice) => void;
}) {
  const { t, locale, title, subtitle, state, dot } = useIdentity(d);
  const { tile, icon } = tileTone(d);
  const hasInbound = d.inbound.length > 0;
  const hasTimeline = d.lastSeen > 0 && at > 0;

  return (
    <div
      onClick={() => onSelect(d)}
      title={t("lan.card.openTitle")}
      className={`cursor-pointer rounded-[10px] border bg-chip-soft px-3 py-2.5 transition-colors ${
        hasInbound
          ? "border-warn/[0.4] hover:bg-warn/[0.04]"
          : "border-hairline-soft hover:border-hairline hover:bg-chip-hover"
      } ${dimmed ? "opacity-55" : ""}`}
    >
      <div className="flex items-center gap-2.5">
        <span
          className={`flex size-[34px] shrink-0 items-center justify-center rounded-[9px] ${tile} ${icon}`}
        >
          <DeviceKindIcon kind={d.kind} size={17} />
        </span>
        <span className="min-w-0 flex-1">
          <span className="flex items-center gap-1.5">
            <span className="truncate text-[12px] leading-tight text-ink">
              {title}
            </span>
            {d.isSelf ? (
              <span className="shrink-0 rounded bg-accent/[0.12] px-1.5 py-px text-[9.5px] text-accent">
                {t("lan.card.badgeSelf")}
              </span>
            ) : null}
            {d.isGateway ? (
              <span className="shrink-0 rounded bg-accent/[0.12] px-1.5 py-px text-[9.5px] text-accent">
                {t("lan.card.badgeUplink")}
              </span>
            ) : null}
            {d.isNew ? (
              <span className="shrink-0 rounded bg-accent/[0.12] px-1.5 py-px text-[9.5px] font-medium text-accent">
                {t("lan.card.badgeNew")}
              </span>
            ) : null}
          </span>
          {subtitle ? (
            <span className="mt-0.5 block truncate text-[10px] leading-tight text-ink-faint">
              {subtitle}
            </span>
          ) : null}
        </span>
        <span
          aria-label={state}
          title={state}
          className={`size-[7px] shrink-0 rounded-full ${dot}`}
        />
      </div>

      <div className="mt-2 flex flex-wrap items-center gap-1.5">
        <span className="rounded-md bg-surface-overlay px-1.5 py-0.5 font-mono text-[10.5px] tabular-nums text-ink">
          {d.ip}
        </span>
        {hasInbound ? (
          <span className="rounded-md bg-warn/[0.12] px-1.5 py-0.5 text-[10px] font-medium text-warn">
            {t("lan.card.inbound", {
              ports:
                d.inbound.slice(0, 2).join(" / ") + (d.inbound.length > 2 ? " …" : ""),
            })}
          </span>
        ) : null}
        {d.services.slice(0, 2).map((s) => (
          <span
            key={s}
            className="truncate rounded-md bg-chip px-1.5 py-0.5 text-[10px] text-ink-dim"
          >
            {s}
          </span>
        ))}
        {d.services.length > 2 ? (
          <span className="text-[10px] text-ink-faint">
            +{d.services.length - 2}
          </span>
        ) : null}
      </div>

      {/* 时间行：只显示「活跃 x 前」，锚定快照时刻；旧缓存（lastSeen = 0）不渲染 */}
      {hasTimeline ? (
        <div className="mt-1.5 text-[10px] leading-tight text-ink-faint">
          {t("lan.card.lastActive", { ago: formatAgo(d.lastSeen, at, locale) })}
        </div>
      ) : null}
    </div>
  );
}

/** 设备类型 → 域色 tile（plan §4 映射表：低饱和域色 + 14% 透明度底） */
function tileTone(d: LanDevice): { tile: string; icon: string } {
  if (d.isSelf || d.isGateway || d.kind === "router") {
    return { tile: "bg-accent/[0.14]", icon: "text-accent" };
  }
  switch (d.kind) {
    case "computer":
    case "phone":
    case "tablet":
      return { tile: "bg-info/[0.14]", icon: "text-info" };
    case "nas":
      return { tile: "bg-[#22d3ee]/[0.14]", icon: "text-[#22d3ee]" };
    case "tv":
    case "speaker":
    case "camera":
    case "iot":
      return { tile: "bg-[#a78bfa]/[0.14]", icon: "text-[#a78bfa]" };
    default:
      return { tile: "bg-chip", icon: "text-ink-dim" };
  }
}

/** 设备详情弹窗：点击卡片打开，Esc / 点遮罩 / 关闭按钮退出 */
export function LanDeviceDetailModal({
  d,
  at,
  onClose,
}: {
  d: LanDevice;
  /** 快照时刻（Unix 秒）：相对时间锚定它而非墙钟（M19 spec 决策 3） */
  at: number;
  onClose: () => void;
}) {
  const { t, locale, hostname, title, subtitle, state, dot } = useIdentity(d);
  const [copied, setCopied] = useState<"ip" | "mac" | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const stateTone = !d.online
    ? "text-ink-faint"
    : d.state === "stale"
      ? "text-warn"
      : "text-accent";
  const { tile, icon } = tileTone(d);
  const hasInbound = d.inbound.length > 0;

  async function copy(text: string, key: "ip" | "mac") {
    const ok = await writeClipboard(text);
    if (ok) {
      setCopied(key);
      window.setTimeout(() => setCopied(null), 1200);
    }
  }

  return (
    <div
      className="fixed inset-0 z-40 bg-black/40"
      onClick={onClose}
      onContextMenu={(e) => {
        e.preventDefault();
        onClose();
      }}
    >
      <div className="fixed inset-0 z-50 flex items-center justify-center p-6">
        <div
          role="dialog"
          aria-label={t("lan.card.detailAria", { title })}
          onClick={(e) => e.stopPropagation()}
          onContextMenu={(e) => e.stopPropagation()}
          className="w-[320px] rounded-xl border border-hairline bg-surface-raised p-4 shadow-2xl shadow-black/50"
        >
          {/* 头部：图标 + 名称 + 状态 */}
          <div className="flex items-center gap-3">
            <span
              className={`flex size-[40px] shrink-0 items-center justify-center rounded-[10px] ${tile} ${icon}`}
            >
              <DeviceKindIcon kind={d.kind} size={20} />
            </span>
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-1.5">
                <span className="truncate text-[13.5px] font-medium text-ink">
                  {title}
                </span>
                {d.isSelf ? (
                  <span className="shrink-0 rounded bg-accent/[0.12] px-1.5 py-px text-[9.5px] text-accent">
                    {t("lan.card.badgeSelf")}
                  </span>
                ) : null}
                {d.isGateway ? (
                  <span className="shrink-0 rounded bg-accent/[0.12] px-1.5 py-px text-[9.5px] text-accent">
                    {t("lan.card.badgeUplink")}
                  </span>
                ) : null}
              </div>
              {subtitle ? (
                <div className="mt-0.5 truncate text-[10.5px] text-ink-faint">
                  {subtitle}
                </div>
              ) : null}
            </div>
            <span className="flex shrink-0 items-center gap-1.5">
              <span className={`size-[7px] rounded-full ${dot}`} />
              <span className={`text-[11px] ${stateTone}`}>{state}</span>
            </span>
          </div>

          {/* 字段明细 */}
          <dl className="mt-3.5 space-y-2 border-t border-hairline-soft pt-3 text-[11px]">
            <DetailRow label={t("lan.field.ip")}>
              <span className="flex items-center gap-1.5">
                <span className="font-mono tabular-nums text-ink">{d.ip}</span>
                <CopyBtn
                  label={copied === "ip" ? t("lan.modal.copied") : t("lan.modal.copy")}
                  onClick={() => void copy(d.ip, "ip")}
                />
              </span>
            </DetailRow>
            <DetailRow label={t("lan.field.mac")}>
              {d.mac ? (
                <span className="flex items-center gap-1.5">
                  <span className="font-mono tabular-nums text-ink">
                    {d.mac}
                    {d.randomizedMac ? (
                      <span className="ml-1 font-sans text-[9.5px] text-ink-faint">
                        {t("lan.field.macRandom")}
                      </span>
                    ) : null}
                  </span>
                  <CopyBtn
                    label={copied === "mac" ? t("lan.modal.copied") : t("lan.modal.copy")}
                    onClick={() => {
                      const mac = d.mac;
                      if (mac) void copy(mac, "mac");
                    }}
                  />
                </span>
              ) : (
                <span className="text-ink-faint">{t("lan.field.macMissing")}</span>
              )}
            </DetailRow>
            <DetailRow label={t("lan.field.vendor")}>
              <span className={d.vendor ? "text-ink" : "text-ink-faint"}>
                {d.vendor ?? t("lan.field.vendorUnknown")}
              </span>
            </DetailRow>
            <DetailRow label={t("lan.field.kind")}>
              <span className="text-ink">{kindLabel(d.kind, locale)}</span>
            </DetailRow>
            {/* 时间线（M19 spec §6.2）：相对 + 绝对双行；旧缓存（0）显示 timeMissing */}
            <DetailRow label={t("lan.field.firstSeen")}>
              {d.firstSeen > 0 && at > 0 ? (
                <span className="text-ink">
                  {formatAgo(d.firstSeen, at, locale)}
                  <span className="mt-0.5 block text-[9.5px] text-ink-faint">
                    {formatDayClock(d.firstSeen, locale)}
                  </span>
                </span>
              ) : (
                <span className="text-ink-faint">{t("lan.field.timeMissing")}</span>
              )}
            </DetailRow>
            <DetailRow label={t("lan.field.lastSeen")}>
              {d.lastSeen > 0 && at > 0 ? (
                <span className="text-ink">
                  {formatAgo(d.lastSeen, at, locale)}
                  <span className="mt-0.5 block text-[9.5px] text-ink-faint">
                    {formatDayClock(d.lastSeen, locale)}
                  </span>
                </span>
              ) : (
                <span className="text-ink-faint">{t("lan.field.timeMissing")}</span>
              )}
            </DetailRow>
            <DetailRow label={t("lan.field.hostname")}>
              <span
                className={hostname ? "font-mono text-ink" : "text-ink-faint"}
              >
                {hostname ?? t("lan.field.hostnameMissing")}
              </span>
            </DetailRow>
            <DetailRow label={t("lan.field.services")}>
              {d.services.length > 0 ? (
                <span className="flex flex-wrap justify-end gap-1">
                  {d.services.map((s) => (
                    <span
                      key={s}
                      className="rounded-md bg-chip px-1.5 py-0.5 text-[10px] text-ink-dim"
                    >
                      {s}
                    </span>
                  ))}
                </span>
              ) : (
                <span className="text-ink-faint">{t("lan.field.servicesNone")}</span>
              )}
            </DetailRow>
            <DetailRow label={t("lan.field.inbound")}>
              {hasInbound ? (
                <span className="font-mono tabular-nums text-warn">
                  {d.inbound.join(" / ")}
                </span>
              ) : (
                <span className="text-ink-faint">{t("lan.field.inboundNone")}</span>
              )}
            </DetailRow>
          </dl>

          {d.randomizedMac ? (
            <p className="mt-3 rounded-lg bg-chip-soft px-2.5 py-1.5 text-[10px] leading-relaxed text-ink-faint">
              {t("lan.modal.randomMacNote")}
            </p>
          ) : null}

          <div className="mt-3 flex items-center justify-between">
            <p className="text-[9.5px] text-ink-faint">{t("lan.modal.footer")}</p>
            <button
              type="button"
              onClick={onClose}
              className="rounded-full bg-chip px-3 py-1 text-[10.5px] text-ink-dim transition-colors hover:bg-chip-hover hover:text-ink"
            >
              {t("lan.modal.close")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function DetailRow({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-start justify-between gap-3">
      <dt className="shrink-0 pt-0.5 text-ink-faint">{label}</dt>
      <dd className="min-w-0 text-right">{children}</dd>
    </div>
  );
}

function CopyBtn({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex shrink-0 items-center gap-1 rounded bg-chip px-1.5 py-0.5 text-[10px] text-ink-dim transition-colors hover:bg-chip-hover hover:text-ink"
    >
      <CopyIcon size={10} />
      {label}
    </button>
  );
}
