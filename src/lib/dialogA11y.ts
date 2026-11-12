import { useEffect, useRef } from "react";

/**
 * 模态层焦点管理（spec §11.2「模态层焦点陷阱生效，关闭后焦点回到触发元素」）。
 *
 * 职责只有三件事：
 *  1. 打开时记录触发元素，关闭后把焦点还给它；
 *  2. 打开时把焦点移进弹层（第一个可聚焦元素）；
 *  3. Tab 在弹层内循环，焦点不漏到遮罩后面的页面。
 *
 * Escape 不在这里管——各模态的 Escape 语义不同（清理抽屉 busy 时禁关），
 * 由组件自己的 keydown 处理，避免双重关闭。
 *
 * 初始焦点带 rAF 重试：内容型弹层（如 SettingsBoard 等 cfg 返回后才渲染按钮）
 * 首帧是空的，effect 时 querySelector 会扑空 —— 最多兜 20 帧，超时放弃。
 */
export function useDialogA11y<T extends HTMLElement>(open: boolean) {
  const ref = useRef<T>(null);
  const restoreRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!open) return;
    restoreRef.current = document.activeElement as HTMLElement | null;

    const focusables = (): HTMLElement[] => {
      const node = ref.current;
      if (!node) return [];
      return [...node.querySelectorAll<HTMLElement>(
        'button,[href],input,select,textarea,[tabindex]:not([tabindex="-1"])',
      )].filter((el) => !el.hasAttribute("disabled") && el.offsetParent !== null);
    };

    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Tab") return;
      const list = focusables();
      if (list.length === 0) return;
      const first = list[0];
      const last = list[list.length - 1];
      const active = document.activeElement as HTMLElement | null;
      if (e.shiftKey && (active === first || !ref.current?.contains(active))) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && (active === last || !ref.current?.contains(active))) {
        e.preventDefault();
        first.focus();
      }
    };

    const node = ref.current;
    node?.addEventListener("keydown", onKey);

    // 初始焦点：内容延迟挂载时逐帧重试（上限 20 帧 ≈ 330 ms）
    let frames = 0;
    let raf = 0;
    const tryFocus = () => {
      const first = focusables()[0];
      if (first) {
        first.focus();
        return;
      }
      if (++frames < 20) raf = requestAnimationFrame(tryFocus);
    };
    tryFocus();

    return () => {
      cancelAnimationFrame(raf);
      node?.removeEventListener("keydown", onKey);
      restoreRef.current?.focus?.();
    };
  }, [open]);

  return ref;
}
