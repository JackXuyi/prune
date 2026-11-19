import type { ReactNode } from "react";

import { useI18n } from "../../i18n/store";
import type { NetDirectionReadout, Readout } from "../../lib/statsReadout";
import { formatPercent } from "../../lib/format";
import type { PowerSource } from "../../types/hardware";
import { BatteryIcon, ChipIcon, DatabaseIcon, DownloadIcon, MemoryCloudIcon, PortJackIcon, UploadIcon } from "../icons";
import type { DrillTab } from "../../types/drill";
import { MiniGauge } from "./MiniGauge";

/**
 * 状态带（eul 签名形态 · 首页重设计）：一段圆角长条内 7 段 hairline 分隔，
 * 每段 = 图标位 + mono 读数，零文字标题——扫一眼即得全部活值。
 *
 * 图标位二态（「环=占比」纪律）：百分比段（CPU / 内存 / 磁盘 / 电池）用
 * MiniGauge；速率 / 端口计数用图标芯片。口径全部进 tooltip；每段可点，
 * 直达对应 tab（与磁贴同一套下钻红线：只导航，不做任何清理动作）。
 */
export function StatusRibbon({
  cpu,
  mem,
  disk,
  port,
  netDown,
  netUp,
  battery,
  exposedCount,
  onDrill,
}: {
  cpu: Readout;
  mem: Readout;
  disk: Readout;
  port: Readout;
  netDown: NetDirectionReadout;
  netUp: NetDirectionReadout;
  /** 主电池；台式机为 null → 该段整段隐藏（失败即隐藏，不留空栏） */
  battery: PowerSource | null;
  /** 对外暴露端口数（>0 时数值转警示色）；null = 端口快照未就绪 */
  exposedCount: number | null;
  onDrill: (tab: DrillTab) => void;
}) {
  const { t } = useI18n();

  const seg =
    "group/seg flex min-w-0 flex-1 cursor-pointer items-center justify-center gap-2.5 rounded-none px-2 transition-colors hover:bg-chip first:rounded-l-[14px] last:rounded-r-[14px]";

  return (
    <div
      className="flex h-[62px] shrink-0 items-stretch overflow-hidden rounded-[14px] border border-hairline-soft bg-gradient-to-b from-chip-soft to-transparent bg-surface-raised divide-x divide-hairline-soft"
      role="group"
      aria-label={t("nav.stats")}
    >
      {/* CPU → 应用页（按 CPU 排序） */}
      <button type="button" className={seg} title={`${t("stats.caption.cpu")}${cpu.sub[0] ? ` · ${cpu.sub[0]}` : ""}`} onClick={() => onDrill("resources")}>
        <MiniGauge ratio={cpu.ratio} color="var(--color-domain-cpu)" icon={<ChipIcon size={12} />} ariaLabel={t("stats.caption.cpu")} />
        <Value v={cpu.main} unit={cpu.unit} />
      </button>

      {/* 内存（主读数是字节，占比才是百分比——读数取 ratio） */}
      <button type="button" className={seg} title={`${t("stats.caption.memory")}${mem.sub[0] ? ` · ${mem.sub[0]}` : ""}`} onClick={() => onDrill("memory")}>
        <MiniGauge ratio={mem.ratio} color="var(--color-info)" icon={<MemoryCloudIcon size={12} />} ariaLabel={t("stats.caption.memory")} />
        <Value v={mem.ratio != null ? formatPercent(mem.ratio) : "—"} />
      </button>

      {/* 磁盘（同上：读数取占比；可回收口径在 tooltip） */}
      <button type="button" className={seg} title={`${t("stats.caption.disk")}${disk.sub[0] ? ` · ${disk.sub[0]}` : ""}`} onClick={() => onDrill("disk")}>
        <MiniGauge ratio={disk.ratio} color="var(--color-accent)" icon={<DatabaseIcon size={12} />} ariaLabel={t("stats.caption.disk")} />
        <Value v={disk.ratio != null ? formatPercent(disk.ratio) : "—"} />
      </button>

      {/* 下载 / 上传：各自读数，非百分比 → 图标芯片 */}
      <button type="button" className={seg} title={`${t("stats.caption.down")} · ${t("stats.net.m5", { value: netDown.avgText })}`} onClick={() => onDrill("net")}>
        <Chip color="var(--color-domain-net)" icon={<DownloadIcon size={13} />} />
        <Value v={netDown.main} unit={netDown.unit} />
      </button>
      <button type="button" className={seg} title={`${t("stats.caption.up")} · ${t("stats.net.m5", { value: netUp.avgText })}`} onClick={() => onDrill("net")}>
        <Chip color="var(--color-domain-net)" icon={<UploadIcon size={13} />} />
        <Value v={netUp.main} unit={netUp.unit} />
      </button>

      {/* 端口：非百分比 → 图标芯片；有对外暴露时数值转警示色（颜色不是唯一载体，tooltip 里有明文） */}
      <button type="button" className={seg} title={`${t("stats.caption.ports")}${port.sub[0] ? ` · ${port.sub[0]}` : ""}`} onClick={() => onDrill("ports")}>
        <Chip color="var(--color-domain-port)" icon={<PortJackIcon size={13} />} />
        <Value
          v={port.status === "ok" ? port.main : "—"}
          tone={exposedCount != null && exposedCount > 0 ? "warn" : undefined}
        />
      </button>

      {/* 电池：台式机整段隐藏 */}
      {battery?.percent != null ? (
        <button
          type="button"
          className={seg}
          title={t("hw.strip.batteryAria", { pct: battery.percent.toFixed(0) })}
          onClick={() => onDrill("device")}
        >
          <MiniGauge
            ratio={battery.percent / 100}
            color={battery.percent <= 20 ? "var(--color-danger)" : battery.percent <= 45 ? "var(--color-warn)" : "var(--color-accent)"}
            icon={<BatteryIcon size={12} />}
            ariaLabel={t("hw.strip.batteryAria", { pct: battery.percent.toFixed(0) })}
          />
          <Value v={battery.percent.toFixed(0)} unit="%" />
        </button>
      ) : null}
    </div>
  );
}

/** 段内图标芯片（非百分比位） */
function Chip({ color, icon }: { color: string; icon: ReactNode }) {
  return (
    <span
      className="grid size-[26px] shrink-0 place-items-center rounded-[8px]"
      style={{
        background: `color-mix(in srgb, ${color} 14%, transparent)`,
        color,
      }}
      aria-hidden
    >
      {icon}
    </span>
  );
}

/** 段内读数：mono 17px + 弱化单位 */
function Value({ v, unit, tone }: { v: string; unit?: string; tone?: "warn" | "danger" }) {
  return (
    <span className="min-w-0 whitespace-nowrap">
      <span
        className={`font-mono text-[17px] leading-none font-semibold tabular-nums ${
          tone === "warn" ? "text-warn" : tone === "danger" ? "text-danger" : "text-ink"
        }`}
      >
        {v}
      </span>
      {unit ? <span className="ml-0.5 text-[10.5px] font-medium text-ink-faint">{unit}</span> : null}
    </span>
  );
}
