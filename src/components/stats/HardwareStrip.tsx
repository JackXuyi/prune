import { useEffect, useState } from "react";

import { useI18n } from "../../i18n/store";
import {
  cpuTempSnapshot,
  cpuTempSetActive,
  hwFlags,
  isTauri,
  powerSnapshot,
  powerSetActive,
} from "../../lib/ipc";
import type { CpuTempSnapshot, PowerSnapshot } from "../../types/hardware";
import { MeterBar } from "../charts/MeterBar";

/**
 * 概览页硬件读数条（f26 G-05 电池 + G-06 蓝牙配件 + G-07 CPU 温度）。
 *
 * 纪律（方案 §四 D4/D6）：
 * - **失败即隐藏**：任一模块读不到（无电池的台式机 / SMC 不可用）就不渲染
 *   对应行，绝不显示 0 或「不可用」占位；
 * - **谁可见谁负责**：挂载时 set_active(true)，卸载时 false——后端门控循环
 *   随之停止采样，常驻开销 0；
 * - 快照轮询 15 s：后端 30 s 一轮，前端半频轮询只是取最新值（纯内存读）。
 */
export function HardwareStrip() {
  const { t } = useI18n();
  const [flags, setFlags] = useState<Map<string, boolean> | null>(null);
  const [power, setPower] = useState<PowerSnapshot | null>(null);
  const [temp, setTemp] = useState<CpuTempSnapshot | null>(null);

  const powerEnabled = flags?.get("hw.power.enabled") ?? true;
  const tempEnabled = flags?.get("hw.cpuTemp.enabled") ?? true;

  useEffect(() => {
    if (!isTauri()) return;
    hwFlags()
      .then((fs) => setFlags(new Map(fs.map((f) => [f.key, f.enabled]))))
      .catch(() => setFlags(new Map()));
  }, []);

  // 电源 + CPU 温度：同一套可见性门控与轮询节奏
  useEffect(() => {
    if (!isTauri()) return;
    const wanted = powerEnabled || tempEnabled;
    if (!wanted) return;
    void powerSetActive(powerEnabled);
    void cpuTempSetActive(tempEnabled);

    const tick = () => {
      if (powerEnabled) void powerSnapshot().then(setPower).catch(() => setPower(null));
      if (tempEnabled) void cpuTempSnapshot().then(setTemp).catch(() => setTemp(null));
    };
    tick();
    const timer = setInterval(tick, 15_000);
    return () => {
      clearInterval(timer);
      void powerSetActive(false);
      void cpuTempSetActive(false);
    };
  }, [powerEnabled, tempEnabled]);

  const battery = power?.sources.find((s) => s.kind === "battery" && s.percent != null);
  const accessories = power?.sources.filter((s) => s.kind === "accessory" && s.percent != null) ?? [];
  const hasAny = battery != null || accessories.length > 0 || temp != null;

  if (!hasAny) return null;

  return (
    <div className="mt-2.5 rounded-card border border-hairline-soft bg-surface-raised px-4 py-2.5">
      <div className="flex items-baseline justify-between gap-3">
        <span className="text-[11px] font-medium tracking-wide text-ink-dim">
          {t("hw.strip.title")}
        </span>
        <span className="text-[9px] text-ink-faint">{t("hw.strip.hint")}</span>
      </div>

      <div className="mt-1.5 grid grid-cols-2 gap-x-5 gap-y-1">
        {battery ? (
          <BatteryRow
            name={battery.name}
            percent={battery.percent ?? 0}
            charging={battery.charging}
            timeToEmptyMin={battery.timeToEmptyMin}
          />
        ) : null}
        {temp ? (
          <div className="flex items-center gap-2">
            <span className="w-[64px] shrink-0 truncate text-[10.5px] text-ink-dim" title={t("hw.strip.cpuTempTitle")}>
              {t("hw.strip.cpuTemp")}
            </span>
            <span className="font-mono text-[12px] tabular-nums text-ink">
              {temp.celsius.toFixed(0)} ℃
            </span>
          </div>
        ) : null}
        {accessories.map((a) => (
          <div key={a.name} className="flex items-center gap-2">
            <span className="w-[64px] shrink-0 truncate text-[10.5px] text-ink-dim" title={a.name}>
              {a.name}
            </span>
            <span className="min-w-0 flex-1">
              <MeterBar value={a.percent ?? 0} max={100} tone="accent" height={3} label={`${a.name} ${(a.percent ?? 0).toFixed(0)}%`} />
            </span>
            <span className="w-[40px] shrink-0 text-right font-mono text-[11px] tabular-nums text-ink">
              {(a.percent ?? 0).toFixed(0)}%
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

function BatteryRow({
  name,
  percent,
  charging,
  timeToEmptyMin,
}: {
  name: string;
  percent: number;
  charging: boolean;
  timeToEmptyMin: number | null;
}) {
  const { t } = useI18n();
  const timeText =
    timeToEmptyMin != null
      ? t("hw.strip.timeLeft", { h: Math.floor(timeToEmptyMin / 60), m: timeToEmptyMin % 60 })
      : charging
        ? t("hw.strip.onAc")
        : "";
  return (
    <div className="flex items-center gap-2" title={name}>
      <span className="w-[64px] shrink-0 truncate text-[10.5px] text-ink-dim">{name}</span>
      <span className="min-w-0 flex-1">
        <MeterBar
          value={percent}
          max={100}
          tone={percent <= 20 ? (charging ? "warn" : "danger") : "accent"}
          height={3}
          label={t("hw.strip.batteryAria", { pct: percent.toFixed(0) })}
        />
      </span>
      <span className="shrink-0 font-mono text-[11px] tabular-nums text-ink">
        {percent.toFixed(0)}%
      </span>
      {timeText ? (
        <span className="w-[72px] shrink-0 text-right text-[9.5px] text-ink-faint">{timeText}</span>
      ) : null}
      {charging ? (
        <span className="shrink-0 rounded-full bg-accent/12 px-1.5 py-px text-[9px] font-medium text-accent">
          {t("hw.strip.charging")}
        </span>
      ) : null}
    </div>
  );
}
