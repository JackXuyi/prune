import { useI18n } from "../../i18n/store";
import type { PowerSource } from "../../types/hardware";
import { BatteryIcon, DiskIcon } from "../icons";
import { MeterBar } from "../charts/MeterBar";
import { InfoTile } from "../stats/InfoTile";
import { formatDiskRate, type DiskTelemetry } from "./useDiskTelemetry";
import type { PowerLowest } from "./DeviceRibbon";
import { lowestPowerSource } from "./DeviceRibbon";

/**
 * 设备页磁贴 ×2（eul 重设计 · device-eul-mockup.html ③）：本机硬件细节。
 *
 * - 电源与配件：百分比域 → MiniGauge 环 + 逐设备电量条；台式机无内置电池
 *   只报配件，两者都读不到整块不渲染（隐藏是失败语义，不是 0 占位）；
 * - 磁盘 I/O · 体检：读速率主读数 + 读:写构成条（只表达此时刻占比，
 *   不代表容量、不是历史走势——当前无 I/O 时序，无数据不画线）。
 *
 * 四层结构（标题 / 读数 / 一行口径 / 微图表）由 InfoTile 契约保证，
 * 完整口径进 tooltip。
 */
export function DeviceTiles({
  battery,
  accessories,
  disk,
}: {
  battery: PowerSource | null;
  accessories: PowerSource[];
  disk: DiskTelemetry;
}) {
  const lowest = lowestPowerSource(battery, accessories);
  const hasBattery = battery != null;
  const hasPower = lowest != null;

  const readSum = disk.io?.disks.reduce((acc, d) => acc + d.readKbPerSec, 0) ?? null;
  const writeSum = disk.io?.disks.reduce((acc, d) => acc + d.writeKbPerSec, 0) ?? null;
  const hasDisk = disk.io != null || disk.smart.length > 0;

  if (!hasPower && !hasDisk) return null;

  // SMART 最差态（failing > unsupported > verified）
  const smartWorst: "verified" | "failing" | "unsupported" | null = disk.smart.some((s) => s.status === "failing")
    ? "failing"
    : disk.smart.some((s) => s.status === "unsupported")
      ? "unsupported"
      : disk.smart.length > 0
        ? "verified"
        : null;

  return (
    <div className="grid shrink-0 grid-cols-2 gap-2.5">
      {hasPower ? (
        <PowerTile lowest={lowest!} sources={{ battery, accessories }} hasBattery={hasBattery} />
      ) : null}
      {hasDisk ? <DiskTile readSum={readSum} writeSum={writeSum} disk={disk} smartWorst={smartWorst} /> : null}
    </div>
  );
}

function PowerTile({
  lowest,
  sources,
  hasBattery,
}: {
  lowest: PowerLowest;
  sources: { battery: PowerSource | null; accessories: PowerSource[] };
  hasBattery: boolean;
}) {
  const { t } = useI18n();

  const suffix = lowest.charging
    ? t("hw.strip.charging")
    : lowest.isBattery && sources.battery?.timeToEmptyMin != null
      ? t("hw.strip.timeLeft", {
          h: Math.floor(sources.battery.timeToEmptyMin / 60),
          m: sources.battery.timeToEmptyMin % 60,
        })
      : lowest.isBattery
        ? t("hw.strip.onAc")
        : "";

  const rows = [
    ...(sources.battery
      ? [
          {
            name: sources.battery.name,
            percent: sources.battery.percent ?? 0,
            charging: sources.battery.charging,
            isBattery: true,
          },
        ]
      : []),
    ...sources.accessories.map((a) => ({
      name: a.name,
      percent: a.percent ?? 0,
      charging: a.charging,
      isBattery: false,
    })),
  ];

  return (
    <InfoTile
      title={t("device.tiles.powerTitle")}
      caption={hasBattery ? t("device.tiles.powerTipBattery") : t("device.tiles.powerTipDesktop")}
      color="var(--color-accent)"
      gaugeRatio={lowest.percent / 100}
      icon={<BatteryIcon size={12} />}
      main={{ value: lowest.percent.toFixed(0), unit: "%" }}
      sub={`${t("device.tiles.lowest", { name: lowest.name })}${suffix ? `（${suffix}）` : ""}`}
      viz={
        <div className="flex flex-col gap-[5px]">
          {rows.map((r) => (
            <div key={r.name} className="flex items-center gap-2" title={r.name}>
              <span className="w-[92px] min-w-0 shrink-0 truncate text-[9.5px] text-ink-dim">{r.name}</span>
              <span className="min-w-0 flex-1">
                <MeterBar
                  value={r.percent}
                  max={100}
                  height={3}
                  tone={r.percent <= 20 ? (r.charging ? "warn" : "danger") : "accent"}
                  label={t("hw.strip.batteryAria", { pct: r.percent.toFixed(0) })}
                />
              </span>
              <span className="w-[32px] shrink-0 text-right font-mono text-[10px] leading-none tabular-nums text-ink-dim">
                {r.percent.toFixed(0)}%
              </span>
            </div>
          ))}
        </div>
      }
    />
  );
}

function DiskTile({
  readSum,
  writeSum,
  disk,
  smartWorst,
}: {
  readSum: number | null;
  writeSum: number | null;
  disk: DiskTelemetry;
  smartWorst: "verified" | "failing" | "unsupported" | null;
}) {
  const { t } = useI18n();

  const diskNames = disk.io?.disks.map((d) => d.name).join(" · ") ?? "";
  const smartText = smartWorst ? t(`disk.hw.smart.${smartWorst}` as const) : null;

  // 构成条：读:写占比（此时刻）。两边都是 0 = 空闲（真实测得的 0 速率，不是缺数据）
  const total = (readSum ?? 0) + (writeSum ?? 0);
  const rShare = total > 0 && readSum != null ? Math.round((readSum / total) * 100) : 0;
  const wShare = total > 0 && writeSum != null ? 100 - rShare : 0;

  // sub = 「↑ 写速率 · disk0 · SMART 已验证」
  const writeRate = writeSum != null ? formatDiskRate(writeSum) : null;
  const sub = [writeRate ? `↑ ${writeRate}` : null, diskNames || null, smartText ? `SMART ${smartText}` : null]
    .filter(Boolean)
    .join(" · ");

  const caption = `${t("device.tiles.diskTip")}${smartText ? ` · SMART ${smartText}` : ""}`;

  const readParts = readSum != null ? splitRate(formatDiskRate(readSum)) : null;

  return (
    <InfoTile
      title={t("device.tiles.diskTitle")}
      caption={caption}
      color="var(--color-info)"
      icon={<DiskIcon size={13} />}
      main={
        readParts
          ? { value: `↓ ${readParts.num}`, unit: readParts.unit }
          : undefined
      }
      note={readSum == null ? t("disk.hw.ioUnavailable") : undefined}
      sub={sub || undefined}
      viz={
        <div>
          <div className="flex h-2 gap-0.5 overflow-hidden rounded-full bg-chip">
            {rShare > 0 ? (
              <div
                className="h-full rounded-l-full"
                style={{
                  width: `${rShare}%`,
                  background:
                    "linear-gradient(90deg, color-mix(in srgb, var(--color-info) 88%, transparent), color-mix(in srgb, var(--color-info) 55%, transparent))",
                }}
                title={`↓ ${readSum != null ? formatDiskRate(readSum) : "—"}`}
              />
            ) : null}
            {wShare > 0 ? (
              <div
                className="h-full rounded-r-full"
                style={{
                  width: `${wShare}%`,
                  background:
                    "linear-gradient(90deg, color-mix(in srgb, var(--color-accent) 55%, transparent), color-mix(in srgb, var(--color-accent) 88%, transparent))",
                }}
                title={writeSum != null ? `↑ ${formatDiskRate(writeSum)}` : undefined}
              />
            ) : null}
          </div>
          <div className="mt-1 flex items-center justify-between text-[9px] text-ink-faint">
            <span>{total > 0 ? t("device.tiles.rwShare", { r: rShare, w: wShare }) : t("device.tiles.idle")}</span>
            <span>{t("device.tiles.snapshotNote")}</span>
          </div>
        </div>
      }
    />
  );
}

/** "126.4 MB/s" → { num: "126.4", unit: "MB/s" }；无空格原样返回 */
function splitRate(rate: string): { num: string; unit?: string } {
  const i = rate.lastIndexOf(" ");
  if (i <= 0) return { num: rate };
  return { num: rate.slice(0, i), unit: rate.slice(i + 1) };
}
