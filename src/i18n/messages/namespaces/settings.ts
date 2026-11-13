/**
 * `settings` namespace：设置页（灵动岛 / 端口冲突保镖 / 局域网新面孔 / 语言）。
 *
 * 语言名不翻译是通行做法（`中文` / `English` 两行恒为原文），
 * 否则用户看不出「点哪个能换回自己看得懂的语言」。
 */
import type { MessageTree } from "../../types";

const zh = {
  title: "设置",
  language: {
    title: "语言",
    auto: "跟随系统",
    /** auto 且系统语言读得到：把解析结果写进触发器，省得用户猜 */
    autoWith: "跟随系统（{lang}）",
    zh: "中文",
    en: "English",
    /** 语言名：给「系统语言是哪个」用，跟 UI 语言走（不是可点击的选项） */
    languageName: {
      zh: "简体中文",
      zhHant: "繁體中文",
      en: "English",
    },
    systemLang: "系统语言：{lang}",
    systemFallback: "系统语言：{lang} · 暂用 {fallback}",
    systemUnknown: "系统语言无法读取 · 暂用 {fallback}",
    current: "当前生效：{current}",
    switchFailed: "语言切换失败：{err}",
  },
  island: {
    title: "灵动岛",
    noNotch: "本机无刘海",
    noNotchHint: "本机屏幕没有物理刘海（或只接了外接屏）",
    ready: "已就位",
    desc: "CPU 持续偏高、内存紧张或端口被占用时从刘海里长出一条提报，6 秒后自行收回。悬停展开看是谁在烧。",
    notchSize: "刘海 {w} × {h} pt",
    notchHint: "接上带刘海的屏幕后会自动启用；外接屏不会造假刘海",
    enable: "启用灵动岛",
  },
  portGuard: {
    title: "端口冲突保镖",
    tag: "关注端口新占用 / 多持有者",
    desc: "谁占了 3000、5173 这类开发端口，从刘海里直接告诉你，不用切到端口页。同一端口 10 分钟内不重复提醒。",
    enable: "启用端口冲突保镖",
  },
  lanWatch: {
    title: "局域网新面孔",
    tag: "只看邻居表，零主动探测",
    desc: "有新设备连上 Wi-Fi 时从刘海里说一声。同一设备只提醒一次；换了网络会重新记住，不刷屏。",
    enable: "启用局域网新面孔",
  },
  cpu: {
    title: "CPU 暴走",
    tag: "判定逻辑在 prune-core，可单测",
    threshold: "阈值",
    thresholdHint: "采样值 ≥ 这个数才算「高」",
    window: "判定窗口",
    windowHint: "看最近多久的采样，太短会被尖峰骗，太长反应慢",
    tolerance: "达标率",
    toleranceHint: "窗口里多少比例的采样要超阈值；90% 表示容忍短暂回落",
    maxPerHour: "每小时上限",
    maxPerHourHint: "所有灵动岛事件的总量闸门",
    rule: "当前规则：最近 {window} 秒里 {tolerance}% 的采样 ≥ {threshold}% 才提报；触发后要掉到 {reset}% 以下才会再次触发。",
  },
  mem: {
    title: "内存异常",
    tag: "内核压力 + swap 占用，双通道",
    swapThreshold: "Swap 阈值",
    swapHint: "swap 占用 ≥ 此值且持续才算紧张；拉到 95% 视为关闭该通道",
    window: "持续窗口",
    windowHint: "内核压力警告 / swap 超阈值要连续保持多久才提报",
    rule: "当前规则：内核压力严重立即提报；警告或 swap ≥ {swap}% 持续 {window} 秒才提报；swap 掉到 {reset}% 以下才会再次触发。",
  },
  saved: "已保存",
  autoSave: "改动即时生效并保存",
  loading: "读取设置…",
  about: {
    title: "关于",
  },
  theme: {
    title: "外观",
    desc: "主题立即生效；「跟随系统」会随系统深浅色自动切换。",
    system: "跟随系统",
    light: "浅色",
    dark: "深色",
  },
  saveFailed: "保存失败：{err}",
} as const;

const en: MessageTree<typeof zh> = {
  title: "Settings",
  language: {
    title: "Language",
    auto: "Follow system",
    autoWith: "Follow system ({lang})",
    zh: "中文",
    en: "English",
    languageName: {
      zh: "Chinese (Simplified)",
      zhHant: "Chinese (Traditional)",
      en: "English",
    },
    systemLang: "System language: {lang}",
    systemFallback: "System language: {lang} · using {fallback}",
    systemUnknown: "System language unavailable · using {fallback}",
    current: "Active: {current}",
    switchFailed: "Language switch failed: {err}",
  },
  island: {
    title: "Dynamic Island",
    noNotch: "No notch on this display",
    noNotchHint: "This display has no physical notch (or only an external display is attached)",
    ready: "Ready",
    desc: "When CPU stays high, memory gets tight, or a port is taken, a report grows out of the notch and retracts after 6 s. Hover to see who's burning.",
    notchSize: "Notch {w} × {h} pt",
    notchHint:
      "Turns on automatically when a notched display is attached; external displays never get a fake notch",
    enable: "Enable Dynamic Island",
  },
  portGuard: {
    title: "Port conflict guard",
    tag: "Watched ports newly taken / multi-holder",
    desc: "Tells you straight from the notch who took 3000 or 5173 — no tab switching. Same port is not re-reported within 10 min.",
    enable: "Enable port conflict guard",
  },
  lanWatch: {
    title: "New device on LAN",
    tag: "Neighbor table only, zero active probing",
    desc: "A word from the notch when a new device joins Wi-Fi. Reported once per device; a new network re-baselines.",
    enable: "Enable new-device alerts",
  },
  cpu: {
    title: "CPU runaway",
    tag: "Logic lives in prune-core and is unit-tested",
    threshold: "Threshold",
    thresholdHint: "A sample counts as high at or above this",
    window: "Window",
    windowHint:
      "How far back samples count — too short is spike-fooled, too long reacts late",
    tolerance: "Required share",
    toleranceHint:
      "Share of samples that must exceed the threshold; 90% tolerates brief dips",
    maxPerHour: "Per-hour cap",
    maxPerHourHint: "Overall gate for all Dynamic Island events",
    rule: "Current rule: {tolerance}% of samples in the last {window} s must be ≥ {threshold}% to report; it re-arms only after dropping below {reset}%.",
  },
  mem: {
    title: "Memory anomaly",
    tag: "Kernel pressure + swap usage, two channels",
    swapThreshold: "Swap threshold",
    swapHint:
      "Tight only when swap stays at or above this; 95% effectively disables the channel",
    window: "Sustain window",
    windowHint: "How long a kernel warning or swap breach must hold before reporting",
    rule: "Current rule: critical pressure reports immediately; warning or swap ≥ {swap}% must hold {window} s; re-arms below {reset}%.",
  },
  saved: "Saved",
  autoSave: "Changes apply and save immediately",
  loading: "Loading settings…",
  about: {
    title: "About",
  },
  theme: {
    title: "Appearance",
    desc: "Applies immediately. \"Follow system\" tracks the OS appearance.",
    system: "System",
    light: "Light",
    dark: "Dark",
  },
  saveFailed: "Save failed: {err}",
};

export const settings = { zh, en } as const;
