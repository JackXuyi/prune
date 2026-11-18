/**
 * 设置页（M15.4 灵动岛 + 语言）。见 docs/m15-dynamic-island-plan.md §5.6。
 *
 * 三个设计决定：
 * 1. **滑杆即时保存 + 400 ms 防抖**：拖一次滑杆会连发几十个 change 事件，
 *    每个都写盘既慢又会和 `set_many` 抢文件；
 * 2. **范围前后端一致**：滑杆的 min/max 就是后端的钳制范围，所以不需要
 *    「保存后回写钳制值」这一步 —— 用户根本拖不出越界值；
 * 3. **语言不防抖**：点一下就切，切失败要立刻回滚并报错（spec §1.5），
 *    防抖只会让用户以为没点上。这段逻辑连同整张语言卡片一起搬到了
 *    `LanguageSection`（下拉菜单 + 系统语言解释），本文件只负责摆位置。
 */
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { AboutSection } from "./AboutSection";
import { LanguageSection } from "./LanguageSection";
import { ReportSection } from "./ReportSection";
import { Badge } from "../components/primitives";
import { useI18n } from "../i18n/store";
import { isTauri, pruneTomlTemplate, autostartGet, autostartSet } from "../lib/ipc";
import { writeClipboard } from "../lib/clipboard";
import { getTheme, setTheme, subscribeTheme, type ThemeMode } from "../lib/theme";
import { usePortStore } from "../stores/portStore";

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
  /** 闲置 dev server（F18，实验功能）：项目端口持续无访问 → 灵动岛提报。默认关 */
  idleServer: boolean;
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
  idleServer: false,
  maxPerHour: 6,
};

export function SettingsBoard() {
  const { t } = useI18n();
  // 端口自动探测（P2）与灵动岛设置不同源：localStorage（同主题约定），
  // 状态在 portStore——开关拨下去立即对当前快照跑一轮，不用等下次扫描
  const autoProbe = usePortStore((s) => s.autoProbe);
  const setAutoProbe = usePortStore((s) => s.setAutoProbe);

  const [cfg, setCfg] = useState<IslandSettings | null>(null);
  const [status, setStatus] = useState<IslandStatus | null>(null);
  // 开机自启动（docs/autostart-plan.md）：与灵动岛设置不同源——状态直接问系统
  // （LaunchAgent plist），不落 settings.json；null = 还没读到
  const [autostartOn, setAutostartOn] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number>(0);

  const dirty = useRef(false);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => {
    if (!isTauri()) {
      setCfg(DEFAULTS);
      setStatus({ available: false, notchW: 0, notchH: 0 });
      setAutostartOn(false);
      return;
    }
    void (async () => {
      try {
        const [s, st, auto] = await Promise.all([
          invoke<IslandSettings>("island_settings_get"),
          invoke<IslandStatus>("island_status"),
          autostartGet(),
        ]);
        setCfg(s);
        setStatus(st);
        setAutostartOn(auto);
      } catch (e) {
        setError(String(e));
        setCfg(DEFAULTS);
        setAutostartOn(false);
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

  /**
   * 开机自启动开关（docs/autostart-plan.md）：不进 400 ms 防抖通道
   * （那是滑杆连发的场景），单击直接生效。乐观更新 + 失败回滚。
   */
  const toggleAutostart = async (v: boolean) => {
    const prev = autostartOn;
    setAutostartOn(v);
    try {
      await autostartSet(v);
      setError(null);
      setSavedAt(Date.now());
    } catch (e) {
      setAutostartOn(prev);
      setError(String(e));
    }
  };

  if (!cfg) {
    return <div className="px-5 py-6 text-[12px] text-ink-faint">{t("settings.loading")}</div>;
  }

  const disabled = !cfg.enabled;

  return (
    <div className="flex-1 overflow-y-auto px-5 py-5">
      <div className="mx-auto flex max-w-[560px] flex-col gap-4">
        <LanguageSection />

        {/* 外观（spec §5.3 v5 新增）：三态主题分段控件。落盘 localStorage["prune.theme"]，
            system 时由 theme.ts 监听 matchMedia 实时切换 —— 不新增 Rust command。 */}
        <section className="rounded-card border border-hairline bg-surface-raised px-4 py-3.5">
          <div className="flex items-center justify-between gap-4">
            <div>
              <span className="text-[13px] font-medium">{t("settings.theme.title")}</span>
              <p className="mt-1 text-[10.5px] leading-4.5 text-ink-dim">{t("settings.theme.desc")}</p>
            </div>
            <ThemeSegmented />
          </div>
        </section>

        {/* 开机自启动（docs/autostart-plan.md）：状态直接问系统（LaunchAgent plist），
            不落 settings.json —— 用户在系统设置登录项里手动增删也能反映出来 */}
        <section className="rounded-card border border-hairline bg-surface-raised px-4 py-3.5">
          <div className="flex items-start justify-between gap-4">
            <div>
              <span className="text-[13px] font-medium">{t("settings.autostart.title")}</span>
              <p className="mt-1 text-[10.5px] leading-4.5 text-ink-dim">
                {t("settings.autostart.desc")}
              </p>
            </div>
            <Switch
              checked={autostartOn ?? false}
              onChange={(v) => void toggleAutostart(v)}
              label={t("settings.autostart.enable")}
            />
          </div>
        </section>

        {/* W2/D-09：prune.toml——只复制模板到剪贴板。Prune 永不写项目文件
            （「工具悄悄改我仓库」是信任红线），用户自己落盘 */}
        <PruneTomlSection />

        <section className="rounded-card border border-hairline bg-surface-raised px-4 py-3.5">
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
          className={`rounded-card border border-hairline bg-surface-raised px-4 py-3.5 transition-opacity ${
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
          className={`rounded-card border border-hairline bg-surface-raised px-4 py-3.5 transition-opacity ${
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

        {/* F18 闲置 dev server 提报（实验功能，默认关）：路线图「冻结第五类事件」
            的 opt-in 通道——默认值翻转到 M-B 访谈判定通过后（f18 方案 §1 门禁）。
            随灵动岛开关变灰 */}
        <section
          className={`rounded-card border border-hairline bg-surface-raised px-4 py-3.5 transition-opacity ${
            disabled ? "opacity-40" : ""
          }`}
        >
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="flex items-center gap-2">
                <span className="text-[12px] font-medium">{t("settings.idleServer.title")}</span>
                <span className="text-[10px] text-ink-faint">{t("settings.idleServer.tag")}</span>
              </div>
              <p className="mt-1.5 text-[10.5px] leading-4.5 text-ink-dim">
                {t("settings.idleServer.desc")}
              </p>
            </div>
            <Switch
              checked={cfg.idleServer}
              onChange={(v) => update({ idleServer: v })}
              label={t("settings.idleServer.enable")}
            />
          </div>
        </section>

        {/* 端口服务自动探测（P2）：它发包，默认关——用户显式授权才跑（调研 §5 的纪律）。
            不随灵动岛开关变灰：这是端口页功能，跟灵动岛无关 */}
        <section className="rounded-card border border-hairline bg-surface-raised px-4 py-3.5">
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="flex items-center gap-2">
                <span className="text-[12px] font-medium">{t("settings.portsProbe.title")}</span>
                <span className="text-[10px] text-ink-faint">{t("settings.portsProbe.tag")}</span>
              </div>
              <p className="mt-1.5 text-[10.5px] leading-4.5 text-ink-dim">
                {t("settings.portsProbe.desc")}
              </p>
            </div>
            <Switch
              checked={autoProbe}
              onChange={setAutoProbe}
              label={t("settings.portsProbe.enable")}
            />
          </div>
        </section>

        <section
          className={`rounded-card border border-hairline bg-surface-raised px-4 py-3.5 transition-opacity ${
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

          <p className="mt-3 rounded-lg bg-chip px-3 py-2 text-[10.5px] leading-4.5 text-ink-faint">
            {t("settings.cpu.rule", {
              window: cfg.cpuSustainSec,
              tolerance: cfg.cpuTolerancePct,
              threshold: cfg.cpuPct,
              reset: Math.max(0, cfg.cpuPct - 10),
            })}
          </p>
        </section>

        <section
          className={`rounded-card border border-hairline bg-surface-raised px-4 py-3.5 transition-opacity ${
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

          <p className="mt-3 rounded-lg bg-chip px-3 py-2 text-[10.5px] leading-4.5 text-ink-faint">
            {t("settings.mem.rule", {
              swap: cfg.swapPct,
              window: cfg.memSustainSec,
              reset: Math.max(50, cfg.swapPct - 10),
            })}
          </p>
        </section>

        {/* 体检报告（复用已装 CLI，docs/w1-report-sample-plan.md §10） */}
        <ReportSection />

        {/* 关于（docs/version-info-plan.md）：应用名 + 版本号，沉底惯例 */}
        <AboutSection />

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
        checked ? "bg-accent/80" : "bg-chip-hover"
      }`}
    >
      <span
        className="absolute top-[3px] h-4 w-4 rounded-full bg-white shadow-[0_1px_3px_rgba(0,0,0,0.3)] transition-all"
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
        className="mt-1.5 h-1 w-full cursor-pointer appearance-none rounded-full bg-chip accent-accent disabled:cursor-not-allowed [&::-webkit-slider-thumb]:h-3 [&::-webkit-slider-thumb]:w-3 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-accent"
      />
      <p className="mt-1 text-[10px] leading-4 text-ink-faint">{hint}</p>
    </div>
  );
}

/** 三态主题分段控件（跟随系统 / 浅色 / 深色）。订阅模式变化，HMR 安全。 */
function ThemeSegmented() {
  const { t } = useI18n();
  const [mode, setMode] = useState<ThemeMode>(() => getTheme());

  useEffect(() => subscribeTheme(setMode), []);

  const items: { key: ThemeMode; label: string }[] = [
    { key: "system", label: t("settings.theme.system") },
    { key: "light", label: t("settings.theme.light") },
    { key: "dark", label: t("settings.theme.dark") },
  ];
  return (
    <div
      role="radiogroup"
      aria-label={t("settings.theme.title")}
      className="flex shrink-0 rounded-full bg-chip p-0.5"
    >
      {items.map((it) => {
        const active = mode === it.key;
        return (
          <button
            key={it.key}
            type="button"
            role="radio"
            aria-checked={active}
            onClick={() => {
              setMode(it.key);
              setTheme(it.key);
            }}
            className={`rounded-full px-2.5 py-1 text-[10.5px] transition-colors ${
              active ? "bg-chip-hover font-medium text-ink" : "text-ink-faint hover:text-ink-dim"
            }`}
          >
            {it.label}
          </button>
        );
      })}
    </div>
  );
}

/**
 * W2/D-09 prune.toml 设置段：说明 + 「复制模板」。
 *
 * 纪律（方案 §6.4）：Prune 永不写项目文件——模板进剪贴板，由用户自己落盘。
 * 这样既不会弄脏 `git status`，也不存在「工具悄悄改我仓库」的信任问题。
 * 模板文本由核心命令下发（`pruneTomlTemplate()`），前端不存副本，
 * 避免两处漂移；注释是单语中文，与 CLI 文案同一约定。
 */

function PruneTomlSection() {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const copy = async () => {
    let template: string;
    try {
      template = await pruneTomlTemplate();
    } catch {
      setErr(t("settings.pruneToml.copyFailed", { err: "template" }));
      return;
    }
    const ok = await writeClipboard(template);
    if (ok) {
      setCopied(true);
      setErr(null);
      window.setTimeout(() => setCopied(false), 4000);
    } else {
      setErr(t("settings.pruneToml.copyFailed", { err: "clipboard" }));
    }
  };

  return (
    <section className="rounded-card border border-hairline bg-surface-raised px-4 py-3.5">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <span className="text-[13px] font-medium">{t("settings.pruneToml.title")}</span>
          <p className="mt-1 text-[10.5px] leading-4.5 text-ink-dim">{t("settings.pruneToml.desc")}</p>
          {copied ? (
            <p className="mt-1 text-[10px] text-accent">{t("settings.pruneToml.copied")}</p>
          ) : null}
          {err ? <p className="mt-1 text-[10px] text-danger">{err}</p> : null}
        </div>
        <button
          type="button"
          onClick={() => void copy()}
          className="shrink-0 rounded-ctl bg-chip px-2.5 py-1.5 text-[11px] text-ink-dim transition-colors hover:bg-chip-hover hover:text-ink"
        >
          {t("settings.pruneToml.copyTemplate")}
        </button>
      </div>
    </section>
  );
}
