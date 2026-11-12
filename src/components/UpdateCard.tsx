/**
 * 面板自动更新卡片（docs/updater-plan.md）。
 *
 * 三个来源汇聚成同一个「待更新」状态：
 * 1. 挂载时 `update_pending()` 兜底查询（面板重开后状态不丢）
 * 2. `update://available` 事件（启动静默检查 / 托盘检查发现新版）
 * 3. 手动「检查更新」按钮（直接调 `update_check` 命令拿返回值）
 *
 * 下载安装走 `update_install`，进度读 `update://progress` 事件；
 * 安装完成 Rust 侧会重启应用（Windows 由 NSIS 安装器接管）。
 * 浏览器 dev 环境（isTauri() = false）整块隐藏。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { useI18n } from "../i18n/store";
import { isTauri, updateCheck, updateInstall, updatePending, type UpdateStatus } from "../lib/ipc";
import { DownloadIcon, RefreshIcon } from "./icons";

type PendingUpdate = Extract<UpdateStatus, { status: "available" }>;
type DownloadProgress = { downloaded: number; total: number | null };

const EVENTS = {
  available: "update://available",
  checkResult: "update://check-result",
  progress: "update://progress",
  installed: "update://installed",
} as const;

function formatMB(bytes: number): string {
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function UpdateCard() {
  const { t } = useI18n();
  const [pending, setPending] = useState<PendingUpdate | null>(null);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [done, setDone] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null); // 「已是最新」等轻提示
  const noteTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const flashNote = useCallback((text: string) => {
    setNote(text);
    if (noteTimer.current) clearTimeout(noteTimer.current);
    noteTimer.current = setTimeout(() => setNote(null), 3000);
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    const unlisteners: Array<() => void> = [];

    // 兜底：本次会话里启动检查 / 托盘检查已发现的更新，面板重开也能看到
    void updatePending()
      .then((s) => {
        if (s.status === "available") setPending(s);
      })
      .catch(() => {});

    void listen<UpdateStatus>(EVENTS.available, (e) => {
      if (e.payload.status === "available") setPending(e.payload);
    }).then((u) => unlisteners.push(u));

    void listen<UpdateStatus>(EVENTS.checkResult, (e) => {
      if (e.payload.status === "up-to-date") flashNote(t("update.upToDate"));
      if (e.payload.status === "error") setError(e.payload.message);
    }).then((u) => unlisteners.push(u));

    void listen<DownloadProgress>(EVENTS.progress, (e) => setProgress(e.payload)).then((u) =>
      unlisteners.push(u),
    );

    void listen(EVENTS.installed, () => {
      setInstalling(false);
      setDone(true);
    }).then((u) => unlisteners.push(u));

    return () => unlisteners.forEach((u) => u());
    // t 跟着 locale 变：切语言后事件回调要造新语言的提示
  }, [flashNote, t]);

  const check = useCallback(async () => {
    setChecking(true);
    setError(null);
    try {
      const result = await updateCheck();
      if (result.status === "available") setPending(result);
      if (result.status === "up-to-date") flashNote(t("update.upToDate"));
      if (result.status === "error") setError(result.message);
    } catch (e) {
      setError(String(e));
    } finally {
      setChecking(false);
    }
  }, [flashNote, t]);

  const install = useCallback(async () => {
    setInstalling(true);
    setError(null);
    try {
      await updateInstall(); // 成功 = 应用已被 Rust 侧重启，正常走不到这里
    } catch (e) {
      setError(String(e));
      setInstalling(false);
    }
  }, []);

  // 浏览器 dev + 无待更新且无过程态：整块隐藏，不占面板版面
  if (!isTauri() && !pending && !note) return null;
  if (!pending && !installing && !done && !note && !error) return null;

  const percent =
    progress && progress.total ? Math.min(Math.round((progress.downloaded / progress.total) * 100), 100) : null;

  return (
    <div className="shrink-0 border-t border-hairline-soft px-3.5 py-2">
      {pending && !installing && !done ? (
        <div className="flex items-center gap-2">
          <DownloadIcon size={13} className="shrink-0 text-accent" />
          <span className="min-w-0 flex-1 truncate text-[11px] text-ink-dim">
            {t("update.availablePrefix")}{" "}
            <span className="font-medium text-ink">v{pending.version}</span>
            <span className="text-ink-faint">
              {t("update.currentVersion", { v: pending.currentVersion })}
            </span>
          </span>
          <button
            type="button"
            onClick={() => void install()}
            className="shrink-0 rounded-md bg-accent px-2 py-1 text-[10px] font-medium text-accent-ink transition-opacity hover:opacity-85"
          >
            {t("update.installNow")}
          </button>
          <button
            type="button"
            onClick={() => setPending(null)}
            className="shrink-0 rounded-md px-1 py-1 text-[10px] text-ink-faint transition-colors hover:text-ink-dim"
            title={t("update.ignore")}
          >
            {t("update.ignore")}
          </button>
        </div>
      ) : null}

      {installing ? (
        <div className="space-y-1.5">
          <div className="flex items-center justify-between text-[11px] text-ink-dim">
            <span>{t("update.downloading")}</span>
            {progress ? (
              <span className="font-mono tabular-nums text-ink-faint">
                {percent != null ? `${percent}%` : formatMB(progress.downloaded)}
              </span>
            ) : null}
          </div>
          <div className="h-1 w-full overflow-hidden rounded-full bg-chip">
            <div
              className="h-full rounded-full bg-accent transition-[width] duration-300"
              style={{ width: percent != null ? `${percent}%` : "30%" }}
            />
          </div>
        </div>
      ) : null}

      {done ? (
        <p className="text-[11px] text-ink-dim">{t("update.done")}</p>
      ) : null}

      {note ? <p className="text-[10px] text-ink-faint">{note}</p> : null}

      {error ? <p className="text-[10px] text-danger">{t("update.checkFailed", { err: error })}</p> : null}

      {/* 没有待安装、没有过程态时也保留手动检查入口（低调的行内按钮） */}
      {!pending && !installing && !done ? (
        <button
          type="button"
          onClick={() => void check()}
          className="mt-0.5 flex items-center gap-1.5 text-[10px] text-ink-faint transition-colors hover:text-ink-dim"
        >
          <RefreshIcon size={11} className={checking ? "animate-spin" : ""} />
          {checking ? t("update.checking") : t("update.check")}
        </button>
      ) : null}
    </div>
  );
}
