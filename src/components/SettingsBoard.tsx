/**
 * 设置弹层（settings-eul-mockup.html W2 落码，取代 M15.4 单列布局）。
 *
 * 结构：弹层头（标题 + 唯一保存状态出口）+ 双栏分组。
 * 左栏 = 常规（外观与语言 / 系统 / 硬件模块 / 关于），
 * 右栏 = 灵动岛域（主开关 + 提报开关 + 阈值滑杆 + 端口探测）。
 *
 * 保留的三条原有设计决定（行为不变，只换皮）：
 * 1. **滑杆即时保存 + 400 ms 防抖**：拖一次滑杆会连发几十个 change 事件，
 *    每个都写盘既慢又会和 `set_many` 抢文件；
 * 2. **范围前后端一致**：滑杆的 min/max 就是后端的钳制范围，所以不需要
 *    「保存后回写钳制值」这一步 —— 用户根本拖不出越界值；
 * 3. **语言不防抖**：点一下就切，切失败要立刻回滚并报错（spec §1.5），
 *    防抖只会让用户以为没点上。
 *
 * eul 纪律落位：每组一个图形锚点；说明文字全降级 tooltip（文字三层红线）；
 * 保存状态只在弹层头一条；实验项带 tag 且默认关；读不到的状态点不渲染。
 */
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { AboutSection } from "./AboutSection";
import { LanguageSection } from "./LanguageSection";
import { ReportSection } from "./ReportSection";
import {
  SettingsGroup,
  Slider,
  StatusDot,
  Switch,
  ToggleRow,
  RuleLine,
} from "./settings/kit";
import { toast } from "../components/primitives";
import { useI18n } from "../i18n/store";
import { isTauri, pruneTomlTemplate, autostartGet, autostartSet, hwFlags, hwFlagSet } from "../lib/ipc";
import { writeClipboard } from "../lib/clipboard";
import { formatClock } from "../lib/format";
import { getTheme, setTheme, subscribeTheme, type ThemeMode } from "../lib/theme";
import { usePortStore } from "../stores/portStore";
import {
  ActivityIcon,
  ChipIcon,
  CopyIcon,
  ExpandIcon,
  GearIcon,
  GlobeIcon,
  PortJackIcon,
} from "./icons";

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

/** f26 硬件模块：UI 呈现顺序与文案键（键名即 settings.json 键） */
const HW_SECTIONS = [
  { key: "hw.diskIo.enabled", titleKey: "settings.hw.diskIo.title", descKey: "settings.hw.diskIo.desc", enableKey: "settings.hw.diskIo.enable" },
  { key: "hw.smart.enabled", titleKey: "settings.hw.smart.title", descKey: "settings.hw.smart.desc", enableKey: "settings.hw.smart.enable" },
  { key: "hw.power.enabled", titleKey: "settings.hw.power.title", descKey: "settings.hw.power.desc", enableKey: "settings.hw.power.enable" },
  { key: "hw.cpuTemp.enabled", titleKey: "settings.hw.cpuTemp.title", descKey: "settings.hw.cpuTemp.desc", enableKey: "settings.hw.cpuTemp.enable" },
  { key: "net.publicIp.enabled", titleKey: "settings.hw.pubIp.title", descKey: "settings.hw.pubIp.desc", enableKey: "settings.hw.pubIp.enable" },
] as const;

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
  // f26 硬件模块开关：settings.json 布尔键（hwFlags/hwFlagSet），乐观更新
  const [hwFlagsMap, setHwFlagsMap] = useState<Map<string, boolean> | null>(null);
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

  // f26 硬件模块开关：mount 读全量；写失败回滚 + toast
  useEffect(() => {
    if (!isTauri()) return;
    hwFlags()
      .then((fs) => setHwFlagsMap(new Map(fs.map((f) => [f.key, f.enabled]))))
      .catch(() => setHwFlagsMap(new Map()));
  }, []);

  const setHwFlag = (key: string, v: boolean) => {
    setHwFlagsMap((prev) => {
      const m = new Map(prev ?? []);
      m.set(key, v);
      return m;
    });
    hwFlagSet(key, v).catch(() => {
      setHwFlagsMap((prev) => {
        const m = new Map(prev ?? []);
        m.set(key, !v);
        return m;
      });
      toast(t("settings.hw.saveFailed"), "error");
    });
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
    return (
      <div className="px-5 py-6 text-[12px] text-ink-faint">{t("settings.loading")}</div>
    );
  }

  const disabled = !cfg.enabled;

  return (
    <div className="flex flex-col">
      {/* 弹层头：标题 + 唯一保存状态出口（自动保存 / 已保存 HH:MM / 失败红字） */}
      <div className="flex h-11 shrink-0 items-center gap-2.5 border-b border-hairline-soft bg-surface-raised px-4">
        <span className="text-[13px] font-semibold">{t("settings.title")}</span>
        <span className="ml-auto text-[10px]">
          {error ? (
            <span className="text-danger">{t("settings.saveFailed", { err: error })}</span>
          ) : savedAt ? (
            <span className="text-accent">{t("settings.savedAt", { clock: formatClock(savedAt) })}</span>
          ) : (
            <span className="text-ink-faint">{t("settings.autoSave")}</span>
          )}
        </span>
      </div>

      {/* 双栏分组：左栏常规，右栏灵动岛域。列内各自滚动兜底（小窗口不裁内容） */}
      <div className="grid min-h-0 flex-1 grid-cols-2 gap-2.5 overflow-y-auto px-4 py-3">
        {/* ═══ 左栏：常规 ═══ */}
        <div className="flex min-w-0 flex-col gap-2.5">
          <SettingsGroup
            icon={<GlobeIcon size={13} />}
            title={t("settings.group.appearance")}
            note={t("settings.group.appearanceNote")}
          >
            <LanguageSection />
            <div
              className="flex items-center justify-between gap-2.5 border-t border-hairline-soft py-1"
              title={t("settings.theme.desc")}
            >
              <span className="text-[11.5px] text-ink">{t("settings.theme.row")}</span>
              <ThemeSegmented />
            </div>
          </SettingsGroup>

          <SettingsGroup
            icon={<GearIcon size={13} />}
            title={t("settings.group.system")}
            note={t("settings.group.systemNote")}
          >
            {/* 自启动：状态直接问系统（LaunchAgent plist），系统设置里手动增删也能反映 */}
            <div
              className="flex items-center justify-between gap-2.5 py-1"
              title={t("settings.autostart.desc")}
            >
              <span className="text-[11.5px] text-ink">{t("settings.autostart.title")}</span>
              <span className="flex shrink-0 items-center gap-2">
                {autostartOn ? (
                  <StatusDot title={t("settings.autostart.desc")}>
                    {t("settings.autostart.onState")}
                  </StatusDot>
                ) : null}
                <Switch
                  checked={autostartOn ?? false}
                  onChange={(v) => void toggleAutostart(v)}
                  label={t("settings.autostart.enable")}
                />
              </span>
            </div>
            <PruneTomlRow />
            {/* 体检报告（复用已装 CLI，docs/w1-report-sample-plan.md §10）：并入系统组一行 */}
            <div className="border-t border-hairline-soft pt-0.5">
              <ReportSection />
            </div>
          </SettingsGroup>

          {/* f26 硬件模块开关（D7）：settings.json 布尔键，默认全开。
              乐观更新 + 失败回滚：开关拨下去 UI 立即翻转，写盘失败回拨并提示。
              两列网格一屏收齐；关闭的模块对应读数整段隐藏（隐藏是失败语义） */}
          <SettingsGroup
            icon={<ChipIcon size={13} />}
            title={t("settings.hw.title")}
            note={t("settings.hw.tag")}
          >
            <div className="grid grid-cols-2 gap-x-3.5">
              {hwFlagsMap == null ? (
                <p className="col-span-2 py-1 text-[11px] text-ink-faint">{t("settings.loading")}</p>
              ) : (
                HW_SECTIONS.map((s, i) => (
                  <div
                    key={s.key}
                    className={i >= 2 ? "border-t border-hairline-soft" : undefined}
                  >
                    <ToggleRow
                      label={t(s.titleKey)}
                      desc={t(s.descKey)}
                      checked={hwFlagsMap.get(s.key) ?? true}
                      onChange={(v) => setHwFlag(s.key, v)}
                      ariaLabel={t(s.enableKey)}
                    />
                  </div>
                ))
              )}
            </div>
          </SettingsGroup>

          {/* 关于（docs/version-info-plan.md）：应用名 + 版本号 + 自身开销，沉底惯例 */}
          <AboutSection />
        </div>

        {/* ═══ 右栏：灵动岛域 ═══ */}
        <div className="flex min-w-0 flex-col gap-2.5">
          <SettingsGroup
            icon={<ExpandIcon size={13} />}
            title={t("settings.island.title")}
            note={t("settings.island.note")}
            titleTooltip={t("settings.island.desc")}
          >
            {/* 主开关行：可用性点徽来自 island_status（刘海实测尺寸）。
                无刘海机型徽标变 warn，其余不动——诚实口径 */}
            <div className="flex items-center justify-between gap-2.5 py-1">
              <span className="text-[11.5px] text-ink">{t("settings.group.mainRow")}</span>
              <span className="flex shrink-0 items-center gap-2">
                {status && !status.available ? (
                  <StatusDot
                    tone="warn"
                    title={t("settings.island.noNotchHint")}
                  >
                    {t("settings.island.noNotch")}
                  </StatusDot>
                ) : status?.available ? (
                  <StatusDot title={t("settings.island.notchHint")}>
                    {t("settings.island.ready")} · {t("settings.island.notchSize", {
                      w: Math.round(status.notchW),
                      h: Math.round(status.notchH),
                    })}
                  </StatusDot>
                ) : null}
                <Switch
                  checked={cfg.enabled}
                  onChange={(v) => update({ enabled: v })}
                  label={t("settings.island.enable")}
                />
              </span>
            </div>
            <div className={disabled ? "transition-opacity opacity-40" : "transition-opacity"}>
              {/* 三条提报通道：说明全降级 tooltip */}
              <ToggleRow
                label={t("settings.portGuard.title")}
                desc={t("settings.portGuard.desc")}
                checked={cfg.portGuard}
                onChange={(v) => update({ portGuard: v })}
                ariaLabel={t("settings.portGuard.enable")}
                dim={disabled}
              />
              <ToggleRow
                label={t("settings.lanWatch.title")}
                desc={t("settings.lanWatch.desc")}
                checked={cfg.lanWatch}
                onChange={(v) => update({ lanWatch: v })}
                ariaLabel={t("settings.lanWatch.enable")}
                dim={disabled}
              />
              {/* F18 闲置 dev server 提报（实验功能，默认关）：路线图「冻结第五类事件」
                  的 opt-in 通道——默认值翻转到 M-B 访谈判定通过后（f18 方案 §1 门禁） */}
              <ToggleRow
                label={t("settings.idleServer.title")}
                tag={t("settings.idleServer.tag")}
                desc={t("settings.idleServer.desc")}
                checked={cfg.idleServer}
                onChange={(v) => update({ idleServer: v })}
                ariaLabel={t("settings.idleServer.enable")}
                dim={disabled}
              />
            </div>
          </SettingsGroup>

          {/* 提报阈值：六条滑杆单行化，hint 进 tooltip；两条规则口径句在场 */}
          <SettingsGroup
            icon={<ActivityIcon size={13} />}
            title={t("settings.group.thresholds")}
            note={t("settings.group.thresholdsNote")}
          >
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
            <RuleLine>
              {t("settings.cpu.rule", {
                window: cfg.cpuSustainSec,
                tolerance: cfg.cpuTolerancePct,
                threshold: cfg.cpuPct,
                reset: Math.max(0, cfg.cpuPct - 10),
              })}
            </RuleLine>
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
            <RuleLine>
              {t("settings.mem.rule", {
                swap: cfg.swapPct,
                window: cfg.memSustainSec,
                reset: Math.max(50, cfg.swapPct - 10),
              })}
            </RuleLine>
          </SettingsGroup>

          {/* 端口服务自动探测（P2）：它发包，默认关——用户显式授权才跑（调研 §5 的纪律）。
              不随灵动岛开关变灰：这是端口页功能，跟灵动岛无关。
              组头是全名，行用短 label 避免同屏重复 */}
          <SettingsGroup
            icon={<PortJackIcon size={13} />}
            title={t("settings.portsProbe.title")}
            note={t("settings.portsProbe.tag")}
          >
            <ToggleRow
              label={t("settings.portsProbe.rowLabel")}
              desc={t("settings.portsProbe.desc")}
              checked={autoProbe}
              onChange={setAutoProbe}
              ariaLabel={t("settings.portsProbe.enable")}
            />
          </SettingsGroup>
        </div>
      </div>
    </div>
  );
}

/** W2/D-09 prune.toml 行：说明 + 「复制模板」。纪律（方案 §6.4）：
    Prune 永不写项目文件——模板进剪贴板，由用户自己落盘。 */
function PruneTomlRow() {
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
    <div className="border-t border-hairline-soft">
      <div
        className="flex items-center justify-between gap-2.5 py-1"
        title={t("settings.pruneToml.desc")}
      >
        <span className="min-w-0 truncate text-[11.5px] text-ink">
          {t("settings.pruneToml.title")}
        </span>
        <button
          type="button"
          onClick={() => void copy()}
          className="flex shrink-0 items-center gap-1 rounded-md bg-chip px-2.5 py-1 text-[10.5px] text-ink-dim transition-colors hover:bg-chip-hover hover:text-ink"
        >
          <CopyIcon size={11} />
          {t("settings.pruneToml.copyTemplate")}
        </button>
      </div>
      {copied ? (
        <p className="pb-1 text-[9.5px] leading-4 text-accent">{t("settings.pruneToml.copied")}</p>
      ) : null}
      {err ? <p className="pb-1 text-[9.5px] leading-4 text-danger">{err}</p> : null}
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
