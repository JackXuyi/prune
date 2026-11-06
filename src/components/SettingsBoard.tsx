/**
 * 灵动岛设置（M15.4）。见 docs/m15-dynamic-island-plan.md §5.6。
 *
 * 两个设计决定：
 * 1. **滑杆即时保存 + 400 ms 防抖**：拖一次滑杆会连发几十个 change 事件，
 *    每个都写盘既慢又会和 `set_many` 抢文件；
 * 2. **范围前后端一致**：滑杆的 min/max 就是后端的钳制范围，所以不需要
 *    「保存后回写钳制值」这一步 —— 用户根本拖不出越界值。
 */
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { Badge } from "../components/primitives";
import { isTauri } from "../lib/ipc";

interface IslandSettings {
  enabled: boolean;
  cpuPct: number;
  cpuSustainSec: number;
  cpuTolerancePct: number;
  swapPct: number;
  maxPerHour: number;
}

interface IslandStatus {
  available: boolean;
  notchW: number;
  notchH: number;
}

const SAVE_DEBOUNCE_MS = 400;

const DEFAULTS: IslandSettings = {
  enabled: true,
  cpuPct: 90,
  cpuSustainSec: 60,
  cpuTolerancePct: 90,
  swapPct: 80,
  maxPerHour: 6,
};

export function SettingsBoard() {
  const [cfg, setCfg] = useState<IslandSettings | null>(null);
  const [status, setStatus] = useState<IslandStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number>(0);

  const dirty = useRef(false);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => {
    if (!isTauri()) {
      setCfg(DEFAULTS);
      setStatus({ available: false, notchW: 0, notchH: 0 });
      return;
    }
    void (async () => {
      try {
        const [s, st] = await Promise.all([
          invoke<IslandSettings>("island_settings_get"),
          invoke<IslandStatus>("island_status"),
        ]);
        setCfg(s);
        setStatus(st);
      } catch (e) {
        setError(String(e));
        setCfg(DEFAULTS);
      }
    })();
  }, []);

  useEffect(() => {
    // 首次加载不触发保存：否则一进设置页就把默认值写盘
    if (!cfg || !dirty.current || !isTauri()) return;
    saveTimer.current = setTimeout(() => {
      void (async () => {
        try {
          await invoke<IslandSettings>("island_settings_set", { settings: cfg });
          setSavedAt(Date.now());
          setError(null);
        } catch (e) {
          setError(String(e));
        }
      })();
    }, SAVE_DEBOUNCE_MS);
    return () => {
      if (saveTimer.current) clearTimeout(saveTimer.current);
    };
  }, [cfg]);

  const update = (patch: Partial<IslandSettings>) => {
    dirty.current = true;
    setCfg((prev) => (prev ? { ...prev, ...patch } : prev));
  };

  if (!cfg) {
    return <div className="px-5 py-6 text-[12px] text-ink-faint">读取设置…</div>;
  }

  const disabled = !cfg.enabled;

  return (
    <div className="flex-1 overflow-y-auto px-5 py-5">
      <div className="mx-auto flex max-w-[560px] flex-col gap-4">
        <section className="rounded-xl border border-hairline bg-white/[0.02] px-4 py-3.5">
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="flex items-center gap-2">
                <span className="text-[13px] font-medium">灵动岛</span>
                {status && !status.available ? (
                  <Badge tone="warn" title="本机屏幕没有物理刘海（或只接了外接屏）">
                    本机无刘海
                  </Badge>
                ) : (
                  <Badge tone="accent">已就位</Badge>
                )}
              </div>
              <p className="mt-1.5 text-[11px] leading-5 text-ink-dim">
                CPU 持续偏高时从刘海里长出一条提报，6 秒后自行收回。悬停展开看是谁在烧。
              </p>
              {status?.available ? (
                <p className="mt-1 text-[10px] text-ink-faint">
                  刘海 {Math.round(status.notchW)} × {Math.round(status.notchH)} pt
                </p>
              ) : (
                <p className="mt-1 text-[10px] text-ink-faint">
                  接上带刘海的屏幕后会自动启用；外接屏不会造假刘海
                </p>
              )}
            </div>
            <Switch
              checked={cfg.enabled}
              onChange={(v) => update({ enabled: v })}
              label="启用灵动岛"
            />
          </div>
        </section>

        <section
          className={`rounded-xl border border-hairline bg-white/[0.02] px-4 py-3.5 transition-opacity ${
            disabled ? "opacity-40" : ""
          }`}
        >
          <div className="mb-3 flex items-center justify-between">
            <span className="text-[12px] font-medium">CPU 暴走</span>
            <span className="text-[10px] text-ink-faint">判定逻辑在 prune-core，可单测</span>
          </div>

          <Slider
            label="阈值"
            hint="采样值 ≥ 这个数才算「高」"
            value={cfg.cpuPct}
            min={60}
            max={100}
            step={1}
            suffix="%"
            disabled={disabled}
            onChange={(v) => update({ cpuPct: v })}
          />
          <Slider
            label="判定窗口"
            hint="看最近多久的采样，太短会被尖峰骗，太长反应慢"
            value={cfg.cpuSustainSec}
            min={10}
            max={600}
            step={10}
            suffix=" 秒"
            disabled={disabled}
            onChange={(v) => update({ cpuSustainSec: v })}
          />
          <Slider
            label="达标率"
            hint="窗口里多少比例的采样要超阈值；90% 表示容忍短暂回落"
            value={cfg.cpuTolerancePct}
            min={50}
            max={100}
            step={5}
            suffix="%"
            disabled={disabled}
            onChange={(v) => update({ cpuTolerancePct: v })}
          />
          <Slider
            label="每小时上限"
            hint="所有灵动岛事件的总量闸门"
            value={cfg.maxPerHour}
            min={1}
            max={30}
            step={1}
            suffix=" 次"
            disabled={disabled}
            onChange={(v) => update({ maxPerHour: v })}
          />

          <p className="mt-3 rounded-lg bg-white/[0.03] px-3 py-2 text-[10.5px] leading-4.5 text-ink-faint">
            当前规则：最近 {cfg.cpuSustainSec} 秒里 {cfg.cpuTolerancePct}% 的采样 ≥{" "}
            {cfg.cpuPct}% 才提报；触发后要掉到 {Math.max(0, cfg.cpuPct - 10)}% 以下才会再次触发。
          </p>
        </section>

        <section className="rounded-xl border border-hairline bg-white/[0.02] px-4 py-3.5">
          <div className="mb-1 flex items-center justify-between">
            <span className="text-[12px] font-medium">Swap 破阈值</span>
            <Badge tone="neutral">下一版启用</Badge>
          </div>
          <Slider
            label="阈值"
            hint="swap 占上限的比例"
            value={cfg.swapPct}
            min={50}
            max={100}
            step={5}
            suffix="%"
            disabled
            onChange={(v) => update({ swapPct: v })}
          />
        </section>

        <div className="flex h-4 items-center gap-2 text-[10.5px]">
          {error ? (
            <span className="text-danger">保存失败：{error}</span>
          ) : savedAt ? (
            <span className="text-accent">已保存</span>
          ) : (
            <span className="text-ink-faint">改动即时生效并保存</span>
          )}
        </div>
      </div>
    </div>
  );
}

function Switch({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className={`relative h-[22px] w-[38px] shrink-0 rounded-full transition-colors ${
        checked ? "bg-accent/80" : "bg-white/[0.12]"
      }`}
    >
      <span
        className="absolute top-[3px] h-4 w-4 rounded-full bg-white transition-all"
        style={{ left: checked ? 20 : 3 }}
      />
    </button>
  );
}

function Slider({
  label,
  hint,
  value,
  min,
  max,
  step,
  suffix,
  disabled,
  onChange,
}: {
  label: string;
  hint: string;
  value: number;
  min: number;
  max: number;
  step: number;
  suffix: string;
  disabled?: boolean;
  onChange: (v: number) => void;
}) {
  return (
    <div className="mt-2.5">
      <div className="flex items-baseline justify-between">
        <span className="text-[11.5px] text-ink-dim">{label}</span>
        <span className="text-[11.5px] font-medium tabular-nums">
          {value}
          {suffix}
        </span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(Number(e.target.value))}
        aria-label={label}
        className="mt-1.5 h-1 w-full cursor-pointer appearance-none rounded-full bg-white/[0.1] accent-accent disabled:cursor-not-allowed [&::-webkit-slider-thumb]:h-3 [&::-webkit-slider-thumb]:w-3 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-accent"
      />
      <p className="mt-1 text-[10px] leading-4 text-ink-faint">{hint}</p>
    </div>
  );
}
