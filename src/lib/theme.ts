/**
 * 主题切换（UI v5 spec §5.3）—— 纯前端，不新增任何 Rust command。
 *
 * 为什么不放 store：主题是**同步、无异步副作用**的，用 zustand 反而要在
 * 首帧前多等一个订阅。这里直接在模块初始化时读 localStorage 并落到
 * `<html data-theme>`，index.css 的同名变量覆盖即刻生效。
 *
 * 三态语义：
 * - system：跟随系统，监听 matchMedia 变化实时切换
 * - light / dark：用户显式指定，不再跟随系统
 *
 * P0 阶段默认值是 dark 而不是 system —— 浅色主题的**组件级对比度**要到 P3
 * 才逐项验收（spec §2.3），这期间若默认跟随系统，浅色系统用户会直接看到
 * 半成品。P3 完成对比度回归后把 DEFAULT_MODE 改成 "system" 即可。
 */

export type ThemeMode = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

const STORAGE_KEY = "prune.theme";
const DEFAULT_MODE: ThemeMode = "dark";

const VALID: readonly ThemeMode[] = ["system", "light", "dark"];

function isMode(v: unknown): v is ThemeMode {
  return typeof v === "string" && (VALID as readonly string[]).includes(v);
}

/** localStorage 在隐私模式 / 磁盘配额满时会抛异常，主题不该因此崩掉界面 */
function readStored(): ThemeMode {
  try {
    const v = window.localStorage.getItem(STORAGE_KEY);
    return isMode(v) ? v : DEFAULT_MODE;
  } catch {
    return DEFAULT_MODE;
  }
}

function writeStored(mode: ThemeMode): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, mode);
  } catch {
    /* 存不下就只影响「下次启动的记忆」，当前会话仍然生效 */
  }
}

function systemTheme(): ResolvedTheme {
  if (typeof window === "undefined" || !window.matchMedia) return "dark";
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

/** 把 mode 解析成实际生效的主题；system 时才问系统 */
export function resolveTheme(mode: ThemeMode): ResolvedTheme {
  return mode === "system" ? systemTheme() : mode;
}

export function getTheme(): ThemeMode {
  return readStored();
}

/** 落到 DOM。data-theme 同时给 index.css 与调试用 */
function apply(resolved: ResolvedTheme): void {
  const root = document.documentElement;
  if (root.dataset.theme === resolved) return;
  root.dataset.theme = resolved;
  // 让原生控件（滚动条、表单、原生右键菜单里的选中色）跟着走
  root.style.colorScheme = resolved;
}

export function setTheme(mode: ThemeMode): void {
  writeStored(mode);
  apply(resolveTheme(mode));
  listeners.forEach((fn) => fn(mode));
}

type Listener = (mode: ThemeMode) => void;
const listeners = new Set<Listener>();

/** 订阅「模式」变化（不是解析值），设置弹层的分段控件用 */
export function subscribeTheme(fn: Listener): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

let mediaBound = false;

/**
 * 启动时调一次。重复调用是安全的（React Fast Refresh 会重复执行模块体）。
 * 返回当前模式，方便调用方初始化控件选中态。
 */
export function initTheme(): ThemeMode {
  const mode = readStored();
  apply(resolveTheme(mode));

  // system 模式下监听系统变化；只绑一次，避免 HMR 叠加监听器
  if (!mediaBound && typeof window !== "undefined" && window.matchMedia) {
    mediaBound = true;
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    const onChange = () => {
      if (readStored() === "system") apply(systemTheme());
    };
    // Safari < 14 只有 addListener，主窗口是 WKWebView，保留双通道
    if (typeof mq.addEventListener === "function") mq.addEventListener("change", onChange);
    else mq.addListener(onChange);
  }

  return mode;
}
