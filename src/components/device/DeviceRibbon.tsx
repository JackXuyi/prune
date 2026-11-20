import { useI18n } from "../../i18n/store";
import type { CpuTempSnapshot, PowerSource } from "../../types/hardware";
import type { LanSnapshot } from "../../types/lan";
import { BatteryIcon, CheckIcon, DeviceIcon, DiskIcon, DownloadIcon, GlobeIcon, UploadIcon, WarnTriangleIcon } from "../icons";
import { RibbonChip, RibbonSeg, RibbonShell, RibbonValue } from "../Ribbon";
import { MiniGauge } from "../stats/MiniGauge";
import { formatDiskRate, type DiskTelemetry } from "./useDiskTelemetry";

/** 电量段输入：最低设备的电量（battery / accessories 由调用方合并后取最低） */
export interface PowerLowest {
  name: string;
  percent: number;
  charging: boolean;
  isBattery: boolean;
}

/**
 * 设备页状态带（eul 重设计 · device-eul-mockup.html ①）：
 * 7 段整页摘要 = 本机硬件 5（CPU 温度 / 电量环 / 磁盘读 / 磁盘写 / SMART）
 * + 局域网 2（在线 / 对外暴露）——设备页一页两域，带子先给全貌。
 *
 * 「环=占比」纪律：只有电量段是百分比，套 MiniGauge；温度/速率/计数/状态灯
 * 全部用图标芯片。读不到显示 —，绝不画 0 弧（MiniGauge 空轨道兜底）。
 */
export function DeviceRibbon({
  temp,
  lowest,
  disk,
  snapshot,
}: {
  temp: CpuTempSnapshot | null;
  /** 电量最低设备；电池与配件都读不到为 null */
  lowest: PowerLowest | null;
  disk: DiskTelemetry;
  /** 局域网快照；未扫描为 null（LAN 两段显示 —，不画 0 台） */
  snapshot: LanSnapshot | null;
}) {
  const { t } = useI18n();

  // 磁盘速率：多块物理盘各自独立测量，这里的和是真实合计（不双计共享卷）
  const readSum = disk.io?.disks.reduce((acc, d) => acc + d.readKbPerSec, 0) ?? null;
  const writeSum = disk.io?.disks.reduce((acc, d) => acc + d.writeKbPerSec, 0) ?? null;

  // SMART 三态灯：failing > unsupported > verified（取最差，诚实优先）
  const smartWorst: DiskTelemetry["smart"][number]["status"] | null = disk.smart.some((s) => s.status === "failing")
    ? "failing"
    : disk.smart.some((s) => s.status === "unsupported")
      ? "unsupported"
      : disk.smart.length > 0
        ? "verified"
        : null;

  const online = snapshot?.devices.filter((d) => d.online).length ?? null;
  const exposed = snapshot?.exposedPorts.length ?? null;

  return (
    <RibbonShell ariaLabel={t("device.ribbon.aria")}>
      <RibbonSeg title={t("device.ribbon.tempTitle")}>
        <RibbonChip
          color="var(--color-info)"
          icon={
            <svg width={13} height={13} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" aria-hidden>
              <path d="M10 4a2 2 0 1 1 4 0v9a4 4 0 1 1-4 0V4Z" />
              <path d="M12 13v3" />
            </svg>
          }
        />
        {temp ? (
          <RibbonValue value={temp.celsius.toFixed(0)} unit="℃" />
        ) : (
          <RibbonValue value="—" />
        )}
      </RibbonSeg>

      <RibbonSeg title={t("device.ribbon.powerTitle")}>
        {lowest ? (
          <>
            <MiniGauge
              ratio={lowest.percent / 100}
              color="var(--color-accent)"
              size={26}
              icon={<BatteryIcon size={12} />}
              ariaLabel={`${lowest.name} ${lowest.percent.toFixed(0)}%`}
            />
            <RibbonValue value={lowest.percent.toFixed(0)} unit="%" />
          </>
        ) : (
          <>
            <RibbonChip color="var(--color-ink-faint)" icon={<BatteryIcon size={13} />} />
            <RibbonValue value="—" />
          </>
        )}
      </RibbonSeg>

      <RibbonSeg title={t("device.ribbon.diskReadTitle")}>
        <RibbonChip color="var(--color-info)" icon={<DownloadIcon size={13} />} />
        {readSum != null ? (
          <RibbonValue value={formatDiskRate(readSum)} />
        ) : (
          <RibbonValue value="—" />
        )}
      </RibbonSeg>

      <RibbonSeg title={t("device.ribbon.diskWriteTitle")}>
        <RibbonChip color="var(--color-accent)" icon={<UploadIcon size={13} />} />
        {writeSum != null ? (
          <RibbonValue value={formatDiskRate(writeSum)} />
        ) : (
          <RibbonValue value="—" />
        )}
      </RibbonSeg>

      <RibbonSeg title={t("device.ribbon.smartTitle")}>
        {smartWorst === "verified" ? (
          <>
            <RibbonChip color="var(--color-accent)" icon={<CheckIcon size={13} />} />
            <RibbonValue value={t("disk.hw.smart.verified")} />
          </>
        ) : smartWorst === "failing" ? (
          <>
            <RibbonChip color="var(--color-danger)" icon={<WarnTriangleIcon size={13} />} />
            <RibbonValue value={t("disk.hw.smart.failing")} tone="danger" />
          </>
        ) : smartWorst === "unsupported" ? (
          <>
            <RibbonChip color="var(--color-ink-faint)" icon={<DiskIcon size={13} />} />
            <RibbonValue value={t("disk.hw.smart.unsupported")} />
          </>
        ) : (
          <>
            <RibbonChip color="var(--color-ink-faint)" icon={<DiskIcon size={13} />} />
            <RibbonValue value="—" />
          </>
        )}
      </RibbonSeg>

      <RibbonSeg title={t("device.ribbon.lanTitle")}>
        <RibbonChip color="var(--color-domain-net)" icon={<DeviceIcon size={13} />} />
        {snapshot && online != null ? (
          <RibbonValue value={String(online)} unit={t("lan.ribbon.onlineUnit", { n: snapshot.devices.length })} />
        ) : (
          <RibbonValue value="—" unit={t("device.ribbon.notScanned")} />
        )}
      </RibbonSeg>

      <RibbonSeg title={t("device.ribbon.exposedTitle")}>
        <RibbonChip color="var(--color-warn)" icon={<GlobeIcon size={13} />} />
        {snapshot && exposed != null ? (
          <RibbonValue
            value={String(exposed)}
            unit={t("lan.ribbon.exposedUnit")}
            tone={exposed > 0 ? "warn" : undefined}
          />
        ) : (
          <RibbonValue value="—" unit={t("device.ribbon.notScanned")} />
        )}
      </RibbonSeg>
    </RibbonShell>
  );
}

/** 从电池 + 配件里取电量最低的设备（ribbon 与磁贴共用同一口径） */
export function lowestPowerSource(
  battery: PowerSource | null,
  accessories: PowerSource[],
): PowerLowest | null {
  const all: PowerLowest[] = [
    ...(battery ? [{ name: battery.name, percent: battery.percent ?? 0, charging: battery.charging, isBattery: true }] : []),
    ...accessories.map((a) => ({
      name: a.name,
      percent: a.percent ?? 0,
      charging: a.charging,
      isBattery: false,
    })),
  ];
  if (all.length === 0) return null;
  return all.reduce((lo, cur) => (cur.percent < lo.percent ? cur : lo));
}
