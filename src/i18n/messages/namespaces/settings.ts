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
  idleServer: {
    title: "闲置 dev server 提醒",
    tag: "实验功能 · 默认关",
    desc: "有项目归属的开发端口持续 30 分钟没有任何访问连接时，从刘海里提报一次。不会自动停掉服务；操作仍在端口页。默认关闭，确认对你有用再开。",
    enable: "启用闲置 dev server 提醒",
  },
  portsProbe: {
    title: "端口服务自动探测",
    tag: "关注层 TCP 端口 · 本机握手",
    desc: "开启后端口页每轮扫描会对关注层端口发一次本机握手（Web / MCP / 数据库协议），把「推断」升级为「确认」。只连 127.0.0.1，结果按进程缓存——对 MCP server 不会重复建会话。",
    enable: "启用自动探测",
  },
  hw: {
    title: "硬件模块",
    tag: "默认全开 · 关闭即停采",
    saveFailed: "开关保存失败，已回拨",
    diskIo: {
      title: "磁盘 I/O",
      desc: "存储页按物理盘显示读 / 写速率。本 tab 可见时每 2 秒差分一次，切走即停。",
      enable: "启用磁盘 I/O",
    },
    smart: {
      title: "SMART 状态灯",
      desc: "内置盘健康状态（已验证 / 警告 / 不支持）。每小时最多查一次 diskutil，外置盘通常不支持。",
      enable: "启用 SMART 状态灯",
    },
    power: {
      title: "电池与配件电量",
      desc: "概览页显示内置电池（电量 / 充电 / 剩余时间）与蓝牙键盘、鼠标、耳机的电量。30 秒低频采样。",
      enable: "启用电池与配件电量",
    },
    cpuTemp: {
      title: "CPU 温度",
      desc: "通过 SMC 读取温度候选键取中位数。读不到的机器上对应卡片自动隐藏，不影响其他功能。",
      enable: "启用 CPU 温度",
    },
    pubIp: {
      title: "公网 IP",
      desc: "网络页显示出口 IP。只发一个无参数请求到公共服务，不携带任何数据，结果缓存 10 分钟。",
      enable: "显示公网 IP",
    },
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
    checkUpdate: "检查更新",
    checking: "检查中…",
  },
  selfFootprint: {
    title: "自身开销",
    at: "{clock} 实测",
    sampleNote: "打开设置页时实测；数字随版本与机器而异",
    mem: "内存",
    cpu: "CPU",
    threads: "线程",
    db: "数据库",
    memHint: "phys_footprint 口径，与内存页同源",
    unavailable: "暂无",
    noPromise: "数字为当前实测，随版本与机器而异；不设上限承诺。CPU 为近 10 轮均值。",
  },
  report: {
    title: "体检报告",
    desc:
      "在本机跑五条只读命令（doctor / scan / mem / port / projects，约 1 分钟），出一份九段 Markdown 报告：每个数字带来源脚注，读不到的写明原因，不自动删除任何东西。",
    detecting: "检测 prune CLI…",
    notFound: "未检测到 prune CLI（需要 v0.13+）",
    notFoundHint: "brew install prune 后重开设置页；或用仓库脚本 pnpm report:sample",
    cliFound: "CLI {version}",
    generate: "生成报告",
    generating: "生成中…（约 1 分钟，磁盘面较慢）",
    done: "已生成（{seconds}s），保存在应用数据目录 reports/ 下",
    openFolder: "打开所在文件夹",
    degradedWarn: "部分项读不到（原因已写进报告）：{items}",
    failed: "生成失败：{err}",
  },
  theme: {
    title: "外观",
    desc: "主题立即生效；「跟随系统」会随系统深浅色自动切换。",
    system: "跟随系统",
    light: "浅色",
    dark: "深色",
  },

  /** 开机自启动（docs/autostart-plan.md）：状态直接问系统，不落 settings.json */
  autostart: {
    title: "开机自启动",
    desc: "登录 macOS 后自动在菜单栏待命，不弹主窗口。也可在「系统设置 → 登录项」里管理。",
    enable: "登录时启动 Prune",
  },

  /** W2/D-09：prune.toml——只复制模板到剪贴板，Prune 永不写项目文件 */
  pruneToml: {
    title: "项目配置（prune.toml）",
    desc:
      "在项目根放一份 prune.toml：跳过项目、排除清理路径、调体检阈值、加关注端口。只能收窄范围，不能扩大；Prune 不写任何项目文件。",
    copyTemplate: "复制模板",
    copied: "模板已复制。自己保存为项目根下的 prune.toml（Prune 不代写）",
    copyFailed: "复制失败：{err}",
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
  idleServer: {
    title: "Idle dev server alert",
    tag: "Experimental · off by default",
    desc: "Reports once from the notch when a project-owned dev port has had zero connections for 30 minutes. Never stops the service for you — actions stay on the Ports page. Off by default; turn it on if it proves useful.",
    enable: "Enable idle dev server alerts",
  },
  portsProbe: {
    title: "Auto-probe port services",
    tag: "Focus-layer TCP ports · local handshake",
    desc: "When on, each port scan handshakes once with focus-layer ports (Web / MCP / database protocols), upgrading \"inferred\" to \"confirmed\". Only 127.0.0.1 is contacted, and results are cached per process — MCP servers never get duplicate sessions.",
    enable: "Enable auto-probe",
  },
  hw: {
    title: "Hardware modules",
    tag: "all on by default · off means no sampling",
    saveFailed: "Failed to save the switch; reverted",
    diskIo: {
      title: "Disk I/O",
      desc: "Per-physical-disk read / write rates on the storage page. Diffs every 2 s while the tab is visible and stops when you leave.",
      enable: "Enable disk I/O",
    },
    smart: {
      title: "SMART status light",
      desc: "Internal-disk health (verified / failing / unsupported). Queries diskutil at most once per hour; external drives usually have none.",
      enable: "Enable SMART status light",
    },
    power: {
      title: "Battery & accessories",
      desc: "Internal battery (level / charging / time left) and Bluetooth keyboard, mouse, headphone levels on the overview. 30 s low-frequency sampling.",
      enable: "Enable battery & accessories",
    },
    cpuTemp: {
      title: "CPU temperature",
      desc: "Reads candidate SMC keys and takes the median. On machines where it can't be read, the card simply hides — nothing else is affected.",
      enable: "Enable CPU temperature",
    },
    pubIp: {
      title: "Public IP",
      desc: "Shows your egress IP on the network page. Sends one parameter-free request to a public service, carries no data, cached for 10 minutes.",
      enable: "Show public IP",
    },
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
    checkUpdate: "Check for updates",
    checking: "Checking…",
  },
  selfFootprint: {
    title: "Its own footprint",
    at: "measured {clock}",
    sampleNote: "Measured when this page opened; numbers vary by version and machine",
    mem: "Memory",
    cpu: "CPU",
    threads: "Threads",
    db: "Database",
    memHint: "phys_footprint, same source as the Memory page",
    unavailable: "n/a",
    noPromise:
      "Current measurement only — varies by version and machine; no ceiling promised. CPU is a 10-round average.",
  },
  report: {
    title: "Health report",
    desc:
      "Runs five read-only commands locally (doctor / scan / mem / port / projects, ~1 min) and produces a nine-section Markdown report: every number footnoted to its source, unreadable items say why, nothing is deleted automatically.",
    detecting: "Detecting prune CLI…",
    notFound: "prune CLI not found (v0.13+ required)",
    notFoundHint: "Run brew install prune and reopen Settings; or use the repo script pnpm report:sample",
    cliFound: "CLI {version}",
    generate: "Generate report",
    generating: "Generating… (~1 min, disk is the slow part)",
    done: "Generated in {seconds}s, saved under app data dir reports/",
    openFolder: "Open folder",
    degradedWarn: "Some domains were unreadable (reasons are in the report): {items}",
    failed: "Generation failed: {err}",
  },
  theme: {
    title: "Appearance",
    desc: "Applies immediately. \"Follow system\" tracks the OS appearance.",
    system: "System",
    light: "Light",
    dark: "Dark",
  },

  /** Autostart (docs/autostart-plan.md): state comes from the system, not settings.json */
  autostart: {
    title: "Launch at login",
    desc: "Starts Prune in the menu bar when you log in — no main window pops up. Also manageable under System Settings → Login Items.",
    enable: "Launch Prune at login",
  },

  /** W2/D-09: prune.toml — copy the template to the clipboard only; Prune never writes project files */
  pruneToml: {
    title: "Project config (prune.toml)",
    desc:
      "Put a prune.toml at a project root to skip projects, exclude cleanup paths, tune thresholds, or add watch ports. It can only narrow the scope, never widen it; Prune never writes project files.",
    copyTemplate: "Copy template",
    copied: "Template copied. Save it as prune.toml at the project root yourself (Prune does not write it)",
    copyFailed: "Copy failed: {err}",
  },
  saveFailed: "Save failed: {err}",
};

export const settings = { zh, en } as const;
