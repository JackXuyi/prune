/**
 * 设置页（M15.4 灵动岛 + 语言）。见 docs/m15-dynamic-island-plan.md §5.6。
 *
 * 三个设计决定：
 * 1. **滑杆即时保存 + 400 ms 防抖**：拖一次滑杆会连发几十个 change 事件，
 *    每个都写盘既慢又会和 `set_many` 抢文件；
 * 2. **范围前后端一致**：滑杆的 min/max 就是后端的钳制范围，所以不需要
 *    「保存后回写钳制值」这一步 —— 用户根本拖不出越界值；
 * 3. **语言不防抖**：点一下就切，切失败要立刻回滚并报错（spec §1.5），
 *    防抖只会让用户以为没点上。
 */
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { Badge } from "../components/primitives";
import { localeName } from "../i18n";
import { useI18n, useLocaleStore } from "../i18n/store";
import type { LocalePref } from "../i18n/types";
import { isTauri } from "../lib/ipc";

interface IslandSettings {
  enabled: boolean;
  cpuPct: number;
  cpuSustainSec: number;
  cpuTolerancePct: number;
  swapPct: number;
  memSustainSec: number;
  /** 端口冲突保镖（M19）：关注端口新占用 / 多持有者 → 灵动岛提报 */
  portGuard: boolean;
  /** 局域网新面孔（M23）：邻居表出现新设备 → 灵动岛提报 */
  lanWatch: boolean;
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
  memSustainSec: 30,
  portGuard: true,
  lanWatch: true,
  maxPerHour: 6,
};

/** 语言三胶囊：顺序固定（跟随系统在最左，符合「默认项优先」的阅读顺序） */
const LOCALE_OPTIONS: LocalePref[] = ["auto", "zh", "en"];

export function SettingsBoard() {
  const { t } = useI18n();
  const locale = useLocaleStore((s) => s.locale);
  const source = useLocaleStore((s) => s.source);
  const setLocale = useLocaleStore((s) => s.setLocale);
  const [localeError, setLocaleError] = useState<string | null>(null);
  const [switching, setSwitching] = useState(false);

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

  const pickLocale = async (next: LocalePref) => {
    setSwitching(true);
    setLocaleError(null);
    try {
      await setLocale(next);
    } catch (e) {
      // store 里的 locale 没被动过，界面天然停在旧语言；这里只负责说出来
      setLocaleError(t("settings.language.switchFailed", { err: String(e) }));
    } finally {
      setSwitching(false);
    }
  };

  if (!cfg) {
    return <div className="px-5 py-6 text-[12px] text-ink-faint">{t("settings.loading")}</div>;
  }

  const disabled = !cfg.enabled;
  // 「跟随系统」被选中时 settings.json 里存的是 auto；界面该高亮哪颗胶囊只能看
  // 解析结果来自哪里（source），不能看 locale 值本身
  const activePref: LocalePref = source === "manual" ? locale : "auto";
  const sourceLabel =
    source === "manual" ? t("settings.language.sourceManual") : t("settings.language.sourceAuto");

  return (
    <div className="flex-1 overflow-y-auto px-5 py-5">
      <div className="mx-auto flex max-w-[560px] flex-col gap-4">
        <section className="rounded-xl border border-hairline bg-white/[0.02] px-4 py-3.5">
          <div className="flex items-center justify-between gap-4">
            <span className="text-[13px] font-medium">{t("settings.language.title")}</span>
            <div className="inline-flex items-center gap-0.5 rounded-lg bg-white/[0.05] p-0.5">
              {LOCALE_OPTIONS.map((opt) => {
                const active = activePref === opt;
                const label =
                  opt === "auto"
                    ? t("settings.language.auto")
                    : opt === "zh"
                      ? t("settings.language.zh")
                      : t("settings.language.en");
                return (
                  <button
                    key={opt}
                    type="button"
                    disabled={switching}
                    aria-pressed={active}
                    onClick={() => void pickLocale(opt)}
                    className={`rounded-md px-2.5 py-1 text-[11px] font-medium transition-colors disabled:opacity-50 ${
                      active ? "bg-white/[0.1] text-ink" : "text-ink-faint hover:text-ink-dim"
                    }`}
                  >
                    {label}
                  </button>
                );
              })}
            </div>
          </div>
          <p className="mt-1.5 text-[10px] leading-4 text-ink-faint">
            {t("settings.language.hint", { current: localeName(locale), source: sourceLabel })}
          </p>
          {localeError ? <p className="mt-1 text-[10px] text-danger">{localeError}</p> : null}
        </section>

        <section className="rounded-xl border border-hairline bg-white/[0.02] px-4 py-3.5">
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="flex items-center gap-2">
                <span className="text-[13px] font-medium">{t("settings.island.title")}</span>
                {status && !status.available ? (
                  <Badge tone="warn" title={t("settings.island.noNotchHint")}>
                    {t("settings.island.noNotch")}
                  </Badge>
                ) : (
                  <Badge tone="accent">{t("settings.island.ready")}</Badge>
                )}
              </div>
              <p className="mt-1.5 text-[11px] leading-5 text-ink-dim">{t("settings.island.desc")}</p>
              {status?.available ? (
                <p className="mt-1 text-[10px] text-ink-faint">
                  {t("settings.island.notchSize", {
                    w: Math.round(status.notchW),
                    h: Math.round(status.notchH),
                  })}
                </p>
              ) : (
                <p className="mt-1 text-[10px] text-ink-faint">{t("settings.island.notchHint")}</p>
              )}
            </div>
            <Switch
              checked={cfg.enabled}
              onChange={(v) => update({ enabled: v })}
              label={t("settings.island.enable")}
            />
          </div>
        </section>

        <section
          className={`rounded-xl border border-hairline bg-white/[0.02] px-4 py-3.5 transition-opacity ${
            disabled ? "opacity-40" : ""
          }`}
        >
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="flex items-center gap-2">
                <span className="text-[12px] font-medium">{t("settings.portGuard.title")}</span>
                <span className="text-[10px] text-ink-faint">{t("settings.portGuard.tag")}</span>
              </div>
              <p className="mt-1.5 text-[10.5px] leading-4.5 text-ink-dim">
                {t("settings.portGuard.desc")}
              </p>
            </div>
            <Switch
              checked={cfg.portGuard}
              onChange={(v) => update({ portGuard: v })}
              label={t("settings.portGuard.enable")}
            />
          </div>
        </section>

        <section
          className={`rounded-xl border border-hairline bg-white/[0.02] px-4 py-3.5 transition-opacity ${
            disabled ? "opacity-40" : ""
          }`}
        >
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="flex items-center gap-2">
                <span className="text-[12px] font-medium">{t("settings.lanWatch.title")}</span>
                <span className="text-[10px] text-ink-faint">{t("settings.lanWatch.tag")}</span>
              </div>
              <p className="mt-1.5 text-[10.5px] leading-4.5 text-ink-dim">
                {t("settings.lanWatch.desc")}
              </p>
            </div>
            <Switch
              checked={cfg.lanWatch}
              onChange={(v) => update({ lanWatch: v })}
              label={t("settings.lanWatch.enable")}
            />
          </div>
        </section>

        <section
          className={`rounded-xl border border-hairline bg-white/[0.02] px-4 py-3.5 transition-opacity ${
            disabled ? "opacity-40" : ""
          }`}
        >
          <div className="mb-3 flex items-center justify-between">
            <span className="text-[12px] font-medium">{t("settings.cpu.title")}</span>
            <span className="text-[10px] text-ink-faint">{t("settings.cpu.tag")}</span>
          </div>

          <Slider
            label={t("settings.cpu.threshold")}
            hint={t("settings.cpu.thresholdHint")}
            value={cfg.cpuPct}
            min={60}
            max={100}
            step={1}
            suffix="%"
            disabled={disabled}
            onChange={(v) => update({ cpuPct: v })}
          />
          <Slider
            label={t("settings.cpu.window")}
            hint={t("settings.cpu.windowHint")}
            value={cfg.cpuSustainSec}
            min={10}
            max={600}
            step={10}
            suffix={t("common.unit.seconds")}
            disabled={disabled}
            onChange={(v) => update({ cpuSustainSec: v })}
          />
          <Slider
            label={t("settings.cpu.tolerance")}
            hint={t("settings.cpu.toleranceHint")}
            value={cfg.cpuTolerancePct}
            min={50}
            max={100}
            step={5}
            suffix="%"
            disabled={disabled}
            onChange={(v) => update({ cpuTolerancePct: v })}
          />
          <Slider
            label={t("settings.cpu.maxPerHour")}
            hint={t("settings.cpu.maxPerHourHint")}
            value={cfg.maxPerHour}
            min={1}
            max={30}
            step={1}
            suffix={t("common.unit.times")}
            disabled={disabled}
            onChange={(v) => update({ maxPerHour: v })}
          />

          <p className="mt-3 rounded-lg bg-white/[0.03] px-3 py-2 text-[10.5px] leading-4.5 text-ink-faint">
            {t("settings.cpu.rule", {
              window: cfg.cpuSustainSec,
              tolerance: cfg.cpuTolerancePct,
              threshold: cfg.cpuPct,
              reset: Math.max(0, cfg.cpuPct - 10),
            })}
          </p>
        </section>

        <section
          className={`rounded-xl border border-hairline bg-white/[0.02] px-4 py-3.5 transition-opacity ${
            disabled ? "opacity-40" : ""
          }`}
        >
          <div className="mb-1 flex items-center justify-between">
            <span className="text-[12px] font-medium">{t("settings.mem.title")}</span>
            <span className="text-[10px] text-ink-faint">{t("settings.mem.tag")}</span>
          </div>
          <Slider
            label={t("settings.mem.swapThreshold")}
            hint={t("settings.mem.swapHint")}
            value={cfg.swapPct}
            min={50}
            max={95}
            step={5}
            suffix="%"
            disabled={disabled}
            onChange={(v) => update({ swapPct: v })}
          />
          <Slider
            label={t("settings.mem.window")}
            hint={t("settings.mem.windowHint")}
            value={cfg.memSustainSec}
            min={10}
            max={300}
            step={5}
            suffix={t("common.unit.seconds")}
            disabled={disabled}
            onChange={(v) => update({ memSustainSec: v })}
          />

          <p className="mt-3 rounded-lg bg-white/[0.03] px-3 py-2 text-[10.5px] leading-4.5 text-ink-faint">
            {t("settings.mem.rule", {
              swap: cfg.swapPct,
              window: cfg.memSustainSec,
              reset: Math.max(50, cfg.swapPct - 10),
            })}
          </p>
        </section>

        <div className="flex h-4 items-center gap-2 text-[10.5px]">
          {error ? (
            <span className="text-danger">{t("settings.saveFailed", { err: error })}</span>
          ) : savedAt ? (
            <span className="text-accent">{t("settings.saved")}</span>
          ) : (
            <span className="text-ink-faint">{t("settings.autoSave")}</span>
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
