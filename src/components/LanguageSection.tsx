/**
 * 设置页语言段（docs/i18n-language-switch-spec.md §5.3）。
 *
 * 为什么从「三颗胶囊」换成下拉菜单：
 * 1. **胶囊是最小的控件，语言却是要一眼找到的项**：三颗就把一行撑满了，
 *    加第四种语言必炸；
 * 2. **「跟随系统」是个黑盒**：选中后完全看不出系统语言被认成了什么
 *    （系统是日语时静默回落英文，用户不知道为什么）。下拉项能带副标题，
 *    正好把这个解释放进去。
 *
 * 交互上有两处不能图省事：
 * - **点当前项不短路**：`locale_set` 值没变时不写盘，但会重读系统语言并广播 ——
 *   这正是「在 macOS 里换了语言，回来重选跟随系统」的刷新路径。
 * - **先关菜单再 await**：切完语言整棵树重渲染，菜单若还开着会以新语言闪一下。
 */
import { useEffect, useRef, useState } from "react";

import { ChevronRightIcon } from "./icons";
import { localeName, systemLangName, type I18n } from "../i18n";
import { isTraditionalTag } from "../i18n/detect";
import { useI18n, useLocaleStore } from "../i18n/store";
import type { LocalePref } from "../i18n/types";

/** 顺序固定：跟随系统在最前（默认项优先），手动项按母语 → 英文排 */
const OPTIONS: LocalePref[] = ["auto", "zh", "en"];

function prefLabel(pref: LocalePref, t: I18n["t"]): string {
  return pref === "auto"
    ? t("settings.language.auto")
    : pref === "zh"
      ? t("settings.language.zh")
      : t("settings.language.en");
}

export function LanguageSection() {
  const { t, locale } = useI18n();
  const source = useLocaleStore((s) => s.source);
  const system = useLocaleStore((s) => s.system);
  const autoResolved = useLocaleStore((s) => s.autoResolved);
  const setLocale = useLocaleStore((s) => s.setLocale);

  const [open, setOpen] = useState(false);
  /** 键盘高亮位置（与当前偏好解耦：光标可以停在别的项上） */
  const [cursor, setCursor] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);

  /**
   * 勾选态看**偏好**不看当前语言：跟随系统时 `locale` 是解析结果（可能是 zh），
   * 直接比对会误把「中文」项勾上。`source` 才是「用户选了什么」的唯一线索。
   */
  const activePref: LocalePref = source === "manual" ? locale : "auto";
  const systemName = systemLangName(system, t);
  /**
   * 繁体不算「被支持」：它解析落英文（spec §4.5），副标题必须说「暂用 English」，
   * 否则用户看到「系统语言：繁體中文」会以为界面该是中文。
   */
  const supported =
    system != null && /^(zh|en)/i.test(system) && !isTraditionalTag(system);

  /** auto 项的副标题：解释「跟随系统」此刻到底跟到了什么 */
  const autoHint = (): string => {
    const fallback = localeName(autoResolved);
    // systemName 与 system 同生共死：system 非空时必然有可显示名（未知语言退化为 tag）
    if (!system || !systemName) return t("settings.language.systemUnknown", { fallback });
    if (!supported) return t("settings.language.systemFallback", { lang: systemName, fallback });
    return t("settings.language.systemLang", { lang: systemName });
  };

  const triggerLabel =
    activePref !== "auto"
      ? localeName(locale)
      : systemName
        ? t("settings.language.autoWith", { lang: systemName })
        : t("settings.language.auto");

  const toggle = () => {
    if (busy) return;
    if (open) {
      setOpen(false);
      return;
    }
    setCursor(Math.max(0, OPTIONS.indexOf(activePref)));
    setOpen(true);
  };

  const pick = async (pref: LocalePref) => {
    setOpen(false);
    setBusy(true);
    setError(null);
    try {
      await setLocale(pref);
    } catch (e) {
      // store 里的 locale 没被动过，界面天然停在旧语言；这里只负责说出来。
      // 用切换前的 `t` 是对的：切失败时用户看到的仍是旧语言的界面
      setError(t("settings.language.switchFailed", { err: String(e) }));
    } finally {
      setBusy(false);
      triggerRef.current?.focus();
    }
  };

  // 菜单打开时才挂全局监听：平时一个多余的监听器都不留
  useEffect(() => {
    if (!open) return;

    const onPointerDown = (e: PointerEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        // 捕获阶段截住：否则主窗口的 Esc 会把整个设置弹层一起关掉
        e.stopPropagation();
        setOpen(false);
        triggerRef.current?.focus();
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault(); // 不拦的话整页会跟着滚
        const step = e.key === "ArrowDown" ? 1 : -1;
        setCursor((c) => (c + step + OPTIONS.length) % OPTIONS.length);
      } else if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        const pref = OPTIONS[cursor];
        if (pref && !busy) void pick(pref);
      }
    };

    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKey, true);
    };
    // pick 每次渲染都是新函数；cursor / busy 变了必须重挂（Enter 取的是最新 cursor）
  }, [open, cursor, busy]);

  return (
    <section className="rounded-xl border border-hairline bg-chip-soft px-4 py-3.5">
      <div className="flex items-center justify-between gap-4">
        <span className="text-[13px] font-medium">{t("settings.language.title")}</span>

        <div ref={rootRef} className="relative">
          <button
            ref={triggerRef}
            type="button"
            disabled={busy}
            aria-haspopup="menu"
            aria-expanded={open}
            onClick={toggle}
            className="flex max-w-[260px] items-center gap-1 rounded-md bg-chip px-2 py-1 text-[11.5px] font-medium text-ink-dim transition-colors hover:bg-chip-hover hover:text-ink disabled:opacity-60"
          >
            <span className="truncate">{triggerLabel}</span>
            <ChevronRightIcon size={11} className="shrink-0 rotate-90 opacity-60" />
          </button>

          {open ? (
            <div
              role="menu"
              aria-label={t("settings.language.title")}
              className="absolute right-0 top-[calc(100%+4px)] z-50 w-[260px] rounded-lg border border-hairline bg-surface-raised p-1 shadow-xl shadow-black/40"
            >
              {OPTIONS.map((opt, i) => {
                const active = activePref === opt;
                return (
                  <button
                    key={opt}
                    type="button"
                    role="menuitemradio"
                    aria-checked={active}
                    disabled={busy}
                    onMouseEnter={() => setCursor(i)}
                    onClick={() => void pick(opt)}
                    className={`flex w-full items-start gap-2 rounded-md px-2 py-1.5 text-left transition-colors disabled:opacity-60 ${
                      i === cursor ? "bg-chip" : ""
                    }`}
                  >
                    <span className="w-3 shrink-0 text-[10px] leading-5 text-accent">
                      {active ? "✓" : ""}
                    </span>
                    <span className="min-w-0">
                      <span
                        className={`block text-[11.5px] leading-5 ${
                          active ? "text-ink" : "text-ink-dim"
                        }`}
                      >
                        {prefLabel(opt, t)}
                      </span>
                      {/* 只有 auto 项带副标题：三项都带会把菜单撑成一张说明表 */}
                      {opt === "auto" ? (
                        <span className="mt-0.5 block text-[10px] leading-4 text-ink-faint">
                          {autoHint()}
                        </span>
                      ) : null}
                    </span>
                  </button>
                );
              })}
            </div>
          ) : null}
        </div>
      </div>

      <p className="mt-1.5 text-[10px] leading-4 text-ink-faint">
        {t("settings.language.current", { current: localeName(locale) })}
      </p>
      {error ? <p className="mt-1 text-[10px] text-danger">{error}</p> : null}
    </section>
  );
}
