import { useEffect, useState } from "react";

import { formatClock } from "../../lib/format";
import { useI18n } from "../../i18n/store";
import type { AudioDevice, BTDevice, EpSnapshot, USBDevice } from "../../types/peripheral";
import { Badge } from "../primitives";
import { BatteryIcon, ChevronRightIcon, DeviceIcon, HeadphonesIcon, UsbIcon } from "../icons";
import { RibbonChip, RibbonSeg, RibbonShell, RibbonValue } from "../Ribbon";
import { MeterBar } from "../charts/MeterBar";
import { MiniGauge } from "../stats/MiniGauge";
import { useDialogA11y } from "../../lib/dialogA11y";
import { usePeripherals } from "./usePeripherals";

/** 外设域色（与 rail「外设」导航项同源） */
const EP_COLOR = "var(--color-domain-device)";

/**
 * 卡面统一入口：一张卡 = 一台设备，三个来源抹平成一种卡面。
 * kind 详情弹窗用；collectedAt = 该源最近一次成功采集（弹窗脚注）。
 */
interface DeviceEntry {
  key: string;
  kind: "bt" | "usb" | "audio";
  bt?: BTDevice;
  usb?: USBDevice;
  audio?: AudioDevice;
  collectedAt: number | null;
}

/**
 * 外设页（m21 · eul 重设计，稿 peripherals-eul-mockup.html）：
 * ① Ribbon 四段摘要（蓝牙 N 台 / USB N 台 / 音频 N 流 / 最低配件电量环）
 * ② 设备卡片网格（每台设备一张卡，整卡可点）
 * ③ 详情弹窗（类型 / 厂商 / 电量分件环 / 采集时刻）
 *
 * 纪律：
 * - 环=占比：只有「最低配件电量」与分件电量套 MiniGauge，计数/mA/方向用图标芯片或明文；
 * - 读不到电量 → 主读数画 —（不是 0）；无设备的源按时间戳区分「未采集(—)」与「0 台」；
 * - 谁可见谁负责：usePeripherals 挂载开 / 卸载停，页面无全局刷新按钮（同设备页）。
 */
export function PeripheralsBoard() {
  const { t } = useI18n();
  const { snapshot, enabled } = usePeripherals();
  const [detail, setDetail] = useState<DeviceEntry | null>(null);

  if (!snapshot) {
    // 首轮采样未到 / 开关关闭：一行状态说明，不画假设备
    return (
      <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto px-5 pt-3 pb-3">
        <p className="mt-10 text-center text-[11px] text-ink-faint">
          {enabled ? t("peripherals.waiting") : t("peripherals.switchOff")}
        </p>
      </div>
    );
  }

  const entries: DeviceEntry[] = [
    ...snapshot.bt.map((d, i) => ({
      key: `bt:${d.name}:${i}`,
      kind: "bt" as const,
      bt: d,
      collectedAt: snapshot.btAt,
    })),
    ...snapshot.usb.map((d, i) => ({
      key: `usb:${d.name}:${i}`,
      kind: "usb" as const,
      usb: d,
      collectedAt: snapshot.usbAt,
    })),
    ...snapshot.audio.map((d, i) => ({
      key: `audio:${d.name}:${d.direction}:${i}`,
      kind: "audio" as const,
      audio: d,
      collectedAt: snapshot.audioAt,
    })),
  ];

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2.5 overflow-x-hidden overflow-y-auto px-5 pt-3 pb-3">
      <EpRibbon snapshot={snapshot} />
      {entries.length > 0 ? (
        <div className="grid grid-cols-3 gap-2.5">
          {entries.map((e) => (
            <DeviceCard key={e.key} entry={e} onOpen={() => setDetail(e)} />
          ))}
        </div>
      ) : (
        <p className="mt-8 text-center text-[11px] text-ink-faint">{t("peripherals.noneFound")}</p>
      )}
      {detail ? <DeviceDetailModal entry={detail} onClose={() => setDetail(null)} /> : null}
    </div>
  );
}

// ---------------------------------------------------------------- ① Ribbon

/**
 * 四段摘要：计数三段（芯片图标，不套环）+ 最低配件电量一段（全页唯一百分比位）。
 * 源从未成功采集 → —；采集成功但 0 台 → 如实显示 0（0 是真话，不是占位）。
 */
function EpRibbon({ snapshot }: { snapshot: EpSnapshot }) {
  const { t } = useI18n();

  const btOk = snapshot.btAt != null;
  const usbOk = snapshot.usbAt != null;
  const audioOk = snapshot.audioAt != null;

  // 最低配件电量：分件电量取最低（IOPS 语义，与设备页「最低设备」同口径）
  const lowest = snapshot.bt.reduce<number | null>((lo, d) => {
    const parts = [d.batteryLeft, d.batteryRight, d.batteryCase].filter((v): v is number => v != null);
    const min = parts.length > 0 ? Math.min(...parts) : null;
    return min != null && (lo == null || min < lo) ? min : lo;
  }, null);

  return (
    <RibbonShell ariaLabel={t("peripherals.ribbonAria")}>
      <RibbonSeg title={t("peripherals.ribbonBtTitle")}>
        <RibbonChip color={EP_COLOR} icon={<HeadphonesIcon size={13} />} />
        {btOk ? (
          <RibbonValue value={String(snapshot.bt.length)} unit={t("peripherals.btUnit")} />
        ) : (
          <RibbonValue value="—" />
        )}
      </RibbonSeg>
      <RibbonSeg title={t("peripherals.ribbonUsbTitle")}>
        <RibbonChip color={EP_COLOR} icon={<UsbIcon size={13} />} />
        {usbOk ? (
          <RibbonValue value={String(snapshot.usb.length)} unit={t("peripherals.usbUnit")} />
        ) : (
          <RibbonValue value="—" />
        )}
      </RibbonSeg>
      <RibbonSeg title={t("peripherals.ribbonAudioTitle")}>
        <RibbonChip color={EP_COLOR} icon={<DeviceIcon size={13} />} />
        {audioOk ? (
          <RibbonValue value={String(snapshot.audio.length)} unit={t("peripherals.audioUnit")} />
        ) : (
          <RibbonValue value="—" />
        )}
      </RibbonSeg>
      <RibbonSeg title={t("peripherals.ribbonLowTitle")}>
        {lowest != null ? (
          <>
            <MiniGauge
              ratio={lowest / 100}
              color={lowest <= 20 ? "var(--color-warn)" : EP_COLOR}
              size={26}
              icon={<HeadphonesIcon size={12} />}
              ariaLabel={t("peripherals.ribbonLowTitle")}
            />
            <RibbonValue value={String(lowest)} unit="%" tone={lowest <= 20 ? "warn" : undefined} />
          </>
        ) : (
          <>
            <RibbonChip color="var(--color-ink-faint)" icon={<HeadphonesIcon size={13} />} />
            <RibbonValue value="—" unit={t("peripherals.ribbonLowEmpty")} />
          </>
        )}
      </RibbonSeg>
    </RibbonShell>
  );
}

// ---------------------------------------------------------------- ② 设备卡

/** 卡面读数与口径行的三源抹平形态（卡与弹窗共用，避免两处各推一遍） */
interface EntryView {
  icon: React.ReactNode;
  badge: { tone: "accent" | "neutral"; label: string } | null;
  /** 分件最低电量（AirPods 类环位）；无分件为 null */
  lowest: number | null;
  /** 主电量百分比（键盘/鼠标条位）；无主电量为 null */
  mainPct: number | null;
  /** USB 总线供电（mA）；非 USB 为 undefined */
  powerMa?: number | null;
  /** 音频方向；非音频为 undefined */
  direction?: "output" | "input";
  caption: string;
  parts: { label: string; pct: number }[];
  name: string;
}

/** 卡面读数与口径行的三源抹平计算 */
function entryView(e: DeviceEntry, t: ReturnType<typeof useI18n>["t"]): EntryView {
  if (e.kind === "bt" && e.bt) {
    const d = e.bt;
    const parts = [
      { label: t("peripherals.partLeft"), pct: d.batteryLeft },
      { label: t("peripherals.partRight"), pct: d.batteryRight },
      { label: t("peripherals.partCase"), pct: d.batteryCase },
    ].filter((p): p is { label: string; pct: number } => p.pct != null);
    const vendor = d.isApple ? "Apple" : (d.vendorId ?? null);
    const typeLabel = d.deviceType || null;
    const caption = parts.length > 0
      ? [typeLabel, vendor, t("peripherals.capLowestBasis")]
      : d.batteryMain != null
        ? [typeLabel, vendor]
        : [typeLabel, vendor, t("peripherals.capNoBattery")];
    return {
      icon: parts.length > 0 || d.batteryMain != null || d.deviceType.toLowerCase().includes("audio") ? (
        <HeadphonesIcon size={13} />
      ) : (
        <DeviceIcon size={13} />
      ),
      badge: d.connected
        ? { tone: "accent" as const, label: t("peripherals.connectedLabel") }
        : { tone: "neutral" as const, label: t("peripherals.notConnected") },
      /** AirPods 类 = 分件最低电量环；有主电量 = MeterBar；都没有 = — */
      lowest: parts.length > 0 ? Math.min(...parts.map((p) => p.pct)) : null,
      mainPct: d.batteryMain,
      caption: caption.filter(Boolean).join(" · "),
      parts,
      name: d.name,
    };
  }
  if (e.kind === "usb" && e.usb) {
    const d = e.usb;
    return {
      icon: <UsbIcon size={13} />,
      badge: null,
      lowest: null,
      mainPct: null,
      powerMa: d.powerMa,
      caption: [d.vendor, d.speed].filter(Boolean).join(" · "),
      parts: [],
      name: d.name,
    };
  }
  const d = e.audio!;
  return {
    icon: <HeadphonesIcon size={13} />,
    badge: d.isDefault ? { tone: "accent" as const, label: t("peripherals.defaultDevice") } : null,
    lowest: null,
    mainPct: null,
    caption: d.direction === "output" ? t("peripherals.capAudioOut") : t("peripherals.capAudioIn"),
    parts: [],
    name: d.name,
    direction: d.direction,
  };
}

/**
 * 单设备卡（对齐 InfoTile 形态）：图标芯片 + 名称 + 状态 Badge，
 * 主读数 + 一行口径 + viz；hover 出 chev，整卡可点开详情弹窗。
 */
function DeviceCard({ entry, onOpen }: { entry: DeviceEntry; onOpen: () => void }) {
  const { t } = useI18n();
  const v = entryView(entry, t);

  return (
    <button
      type="button"
      onClick={onOpen}
      title={t("peripherals.cardHint")}
      className="group relative flex h-full min-h-[104px] w-full cursor-pointer flex-col rounded-card border border-hairline-soft bg-surface-raised px-3.5 py-2.5 text-left transition-colors hover:bg-chip/60"
    >
      {/* 头：图标芯片 + 名称 + 状态 Badge + hover chev */}
      <div className="flex w-full items-center gap-2">
        <span
          className="grid size-[24px] shrink-0 place-items-center rounded-[7px]"
          style={{
            background: `color-mix(in srgb, ${EP_COLOR} 15%, transparent)`,
            color: EP_COLOR,
            boxShadow: `0 0 10px color-mix(in srgb, ${EP_COLOR} 20%, transparent)`,
          }}
          aria-hidden
        >
          {v.icon}
        </span>
        <span className="min-w-0 flex-1 truncate text-[11px] font-semibold text-ink-dim" title={v.name}>
          {v.name}
        </span>
        {v.badge ? (
          <Badge tone={v.badge.tone}>{v.badge.label}</Badge>
        ) : null}
        <ChevronRightIcon
          size={12}
          className="shrink-0 text-ink-faint opacity-0 transition-opacity group-hover:opacity-100"
          aria-hidden
        />
      </div>

      {/* 主读数：AirPods=分件最低环 / 有主电量=MeterBar / USB=mA / 音频=方向 / 其余=— */}
      <div className="mt-2 flex min-w-0 flex-1 items-center">
        {v.lowest != null ? (
          <>
            <MiniGauge
              ratio={v.lowest / 100}
              color={v.lowest <= 20 ? "var(--color-warn)" : EP_COLOR}
              size={34}
              stroke={2.6}
              icon={<BatteryIcon size={13} />}
              ariaLabel={`${v.name} ${v.lowest}%`}
            />
            <span className="ml-2.5 font-mono text-[23px] leading-none font-semibold tabular-nums text-ink">
              {v.lowest}
              <span className="ml-0.5 text-[11px] font-medium text-ink-faint">%</span>
            </span>
          </>
        ) : v.mainPct != null ? (
          <span className="font-mono text-[23px] leading-none font-semibold tabular-nums text-ink">
            {v.mainPct}
            <span className="ml-0.5 text-[11px] font-medium text-ink-faint">%</span>
          </span>
        ) : entry.kind === "usb" ? (
          <span className="font-mono text-[23px] leading-none font-semibold tabular-nums text-ink">
            {v.powerMa != null ? (
              <>
                {v.powerMa}
                <span className="ml-0.5 text-[11px] font-medium text-ink-faint">mA</span>
              </>
            ) : (
              "—"
            )}
          </span>
        ) : entry.kind === "audio" ? (
          <span className="text-[17px] leading-none font-semibold text-ink">
            {v.direction === "output" ? t("peripherals.output") : t("peripherals.input")}
          </span>
        ) : (
          <span className="font-mono text-[23px] leading-none font-semibold text-ink-faint">—</span>
        )}
      </div>

      {/* viz：分件小读数 / 主电量条（无 viz 的源不占位） */}
      {v.parts.length > 0 ? (
        <div className="mt-1.5 flex flex-wrap gap-x-3 gap-y-0.5">
          {v.parts.map((p) => (
            <span key={p.label} className="flex items-baseline gap-1 text-[10px] leading-none">
              <span className="text-ink-faint">{p.label}</span>
              <span className="font-mono tabular-nums text-ink-dim">{p.pct}%</span>
            </span>
          ))}
        </div>
      ) : v.mainPct != null ? (
        <div className="mt-1.5">
          <MeterBar
            value={v.mainPct}
            max={100}
            height={3}
            tone={v.mainPct <= 20 ? "warn" : "accent"}
            label={`${v.name} ${v.mainPct}%`}
          />
        </div>
      ) : null}

      {/* 口径行（第三层文字，超出部分在弹窗里） */}
      <div className="mt-1.5 w-full border-t border-hairline-soft pt-1.5">
        <span className="block truncate text-[10px] leading-none text-ink-faint" title={v.caption}>
          {v.caption}
        </span>
      </div>
    </button>
  );
}

// ---------------------------------------------------------------- ③ 详情弹窗

/**
 * 详情弹窗（稿 ③，430px 居中）：明细行 = 连接 / 类型 / 厂商 / 电量（分件环）
 * / USB 速率与总线供电 / 音频方向与默认态 / 采集时刻。a11y 模式同设置弹层。
 */
function DeviceDetailModal({ entry, onClose }: { entry: DeviceEntry; onClose: () => void }) {
  const { t } = useI18n();
  const dialogRef = useDialogA11y<HTMLDivElement>(true);
  const v = entryView(entry, t);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const rows: { label: string; node: React.ReactNode }[] = [];
  if (entry.kind === "bt" && entry.bt) {
    const d = entry.bt;
    const vendor = d.isApple
      ? d.vendorId
        ? `Apple · ${d.vendorId}`
        : "Apple"
      : (d.vendorId ?? null);
    rows.push(
      { label: t("peripherals.dConnection"), node: d.connected ? t("peripherals.connectedLabel") : t("peripherals.notConnected") },
      { label: t("peripherals.dType"), node: d.deviceType || "—" },
      { label: t("peripherals.dVendor"), node: vendor ?? "—" },
      {
        label: t("peripherals.dBattery"),
        node: (
          <span className="flex flex-wrap items-center gap-2.5">
            {v.parts.length > 0
              ? v.parts.map((p) => (
                  <span key={p.label} className="flex items-center gap-1.5">
                    <MiniGauge
                      ratio={p.pct / 100}
                      color={p.pct <= 20 ? "var(--color-warn)" : EP_COLOR}
                      size={24}
                      stroke={2.2}
                      icon={<span className="text-[8px] leading-none font-medium">{p.label}</span>}
                      ariaLabel={`${p.label} ${p.pct}%`}
                    />
                    <span className="font-mono text-[10.5px] leading-none tabular-nums text-ink-dim">{p.pct}%</span>
                  </span>
                ))
              : d.batteryMain != null
                ? (
                    <span className="min-w-0 flex-1">
                      <MeterBar
                        value={d.batteryMain}
                        max={100}
                        height={4}
                        tone={d.batteryMain <= 20 ? "warn" : "accent"}
                        label={`${d.name} ${d.batteryMain}%`}
                      />
                    </span>
                  )
                : "—"}
          </span>
        ),
      },
    );
  } else if (entry.kind === "usb" && entry.usb) {
    const d = entry.usb;
    rows.push(
      { label: t("peripherals.dVendor"), node: d.vendor ?? "—" },
      { label: t("peripherals.dSpeed"), node: d.speed ?? "—" },
      { label: t("peripherals.dPower"), node: d.powerMa != null ? `${d.powerMa} mA` : "—" },
    );
  } else if (entry.kind === "audio" && entry.audio) {
    const d = entry.audio;
    rows.push(
      { label: t("peripherals.dDirection"), node: d.direction === "output" ? t("peripherals.output") : t("peripherals.input") },
      { label: t("peripherals.dDefaultState"), node: d.isDefault ? t("peripherals.yes") : t("peripherals.no") },
    );
  }

  return (
    <div className="fixed inset-0 z-[70] flex items-center justify-center p-6">
      <div className="prune-fade-in absolute inset-0 bg-black/45 backdrop-blur-[2px]" onClick={onClose} />
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label={t("peripherals.detailTitle", { name: v.name })}
        className="relative flex max-h-[86vh] w-[430px] max-w-full flex-col overflow-hidden rounded-[14px] border border-hairline bg-surface-raised shadow-2xl shadow-black/50"
      >
        {/* 头：图标芯片 + 名称 + 状态 Badge + 关闭 */}
        <div className="flex items-center gap-2 px-4 pt-3.5 pb-2.5">
          <span
            className="grid size-[26px] shrink-0 place-items-center rounded-[8px]"
            style={{
              background: `color-mix(in srgb, ${EP_COLOR} 15%, transparent)`,
              color: EP_COLOR,
            }}
            aria-hidden
          >
            {v.icon}
          </span>
          <span className="min-w-0 flex-1 truncate text-[13.5px] font-medium text-ink">{v.name}</span>
          {v.badge ? <Badge tone={v.badge.tone}>{v.badge.label}</Badge> : null}
          <button
            type="button"
            onClick={onClose}
            title={t("common.action.close")}
            aria-label={t("common.action.close")}
            className="grid size-6 shrink-0 place-items-center rounded-ctl text-ink-faint transition-colors hover:bg-chip hover:text-ink"
          >
            ✕
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto border-t border-hairline-soft px-4 py-3">
          <div className="grid grid-cols-[64px_1fr] gap-x-3 gap-y-2 text-[11px]">
            {rows.map((r) => (
              <div key={r.label} className="col-span-2 grid grid-cols-[64px_1fr] gap-x-3">
                <span className="text-ink-faint">{r.label}</span>
                <span className="min-w-0 text-ink-dim">{r.node}</span>
              </div>
            ))}
          </div>
        </div>

        {/* 脚：该源最近一次成功采集时刻 */}
        <div className="flex items-center justify-between gap-3 border-t border-hairline-soft px-4 py-2.5">
          <span className="min-w-0 truncate text-[10px] text-ink-faint">
            {t("peripherals.dCollected")} {formatClock(entry.collectedAt)}
          </span>
          <button
            type="button"
            onClick={onClose}
            className="shrink-0 rounded-md border border-hairline-soft px-2.5 py-1 text-[10.5px] text-ink-dim transition-colors hover:bg-chip"
          >
            {t("common.action.close")}
          </button>
        </div>
      </div>
    </div>
  );
}
