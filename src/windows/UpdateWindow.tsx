/**
 * 独立「检查更新」弹窗窗口（docs/updater-plan.md）。
 *
 * 取代原生的纯文本 dialog：更新说明用 markdown 渲染（Markdown 组件），
 * 支持标题层级 / 加粗 / 行内代码 / 链接，窗口固定 800×600、超出时内容区滚动。四态：
 * - checking：转圈 + 正在检查更新（只允许关闭）
 * - available：立即更新（下载安装并重启）/ 稍后
 * - upToDate：好
 * - error：错误文案 + 好
 *
 * 数据流：mount 后主动 `updateDialogData()` pull 一次（规避事件重放竞态），
 * 同时 listen `update://dialog`——Rust 在写入 state 并 show 窗口后 emit，
 * 已 mount 的窗口收到即重新拉。下载进度 / 完成走既有事件。
 *
 * 与托盘、设置页共用同一 Rust 逻辑（check_and_prompt），只是反馈从原生
 * dialog 换成这个 WebView 窗口。浏览器 dev 下可用 ?window=update 预览。
 */
import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { Markdown } from "../components/Markdown";
import { CheckIcon, CloseIcon, DownloadIcon, RefreshIcon } from "../components/icons";
import { useI18n } from "../i18n/store";
import { DEV_UPDATE_CHECKING, DEV_UPDATE_DIALOG } from "../lib/devFixture";
import {
  isTauri,
  updateDialogData,
  updateInstall,
  type UpdateDialogPayload,
} from "../lib/ipc";

type Progress = { downloaded: number; total: number | null };

const EVENT_DIALOG = "update://dialog";
const EVENT_PROGRESS = "update://progress";
const EVENT_INSTALLED = "update://installed";

function fmt(bytes: number): string {
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** 浏览器 dev 预览数据：?checking=1 看「检查中」转圈，否则看结果态。 */
function devPayload(): UpdateDialogPayload {
  const checking = new URLSearchParams(window.location.search).get("checking") === "1";
  return (checking ? DEV_UPDATE_CHECKING : DEV_UPDATE_DIALOG) as UpdateDialogPayload;
}

export function UpdateWindow() {
  const { t } = useI18n();
  // 浏览器 dev（?window=update）用假数据预览；Tauri 下走真实 IPC。
  const [payload, setPayload] = useState<UpdateDialogPayload | null>(
    isTauri() ? null : devPayload(),
  );
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [done, setDone] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);

  const pull = useCallback(() => {
    if (!isTauri()) return;
    void updateDialogData()
      .then(setPayload)
      .catch(() => {});
  }, []);

  useEffect(() => {
    pull();
    if (!isTauri()) return;
    const unlisteners: Array<() => void> = [];
    void listen(EVENT_DIALOG, pull).then((u) => unlisteners.push(u));
    void listen<Progress>(EVENT_PROGRESS, (e) => setProgress(e.payload)).then((u) =>
      unlisteners.push(u),
    );
    void listen(EVENT_INSTALLED, () => {
      setInstalling(false);
      setDone(true);
    }).then((u) => unlisteners.push(u));
    return () => unlisteners.forEach((u) => u());
  }, [pull]);

  const close = useCallback(() => {
    setInstalling(false);
    setDone(false);
    setInstallError(null);
    setProgress(null);
    // 复位到 checking：窗口是 hide/show 复用、组件不卸载，下次显示时先看到
    // 转圈而不是上一次的旧结果（Rust 侧也同步写了 checking，pull 会再确认）。
    setPayload(isTauri() ? { status: "checking" } : devPayload());
    // 用 Tauri 内置窗口 API 关窗，而非自定义 command：后者依赖 Rust 侧注册，
    // dev 只热重载前端时旧二进制没有该 command，会抛 "Command not found"。
    if (isTauri()) void getCurrentWindow().hide().catch(() => {});
  }, []);

  // Escape 关闭（窗口无系统标题栏，键盘兜底）
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close]);

  const install = useCallback(async () => {
    setInstalling(true);
    setInstallError(null);
    try {
      await updateInstall();
    } catch (e) {
      setInstalling(false);
      setInstallError(String(e));
    }
  }, []);

  // 直接在 payload 上做类型收窄（discriminated union）；TS 无法把局部 status
  // 变量的收窄传回 payload，故这里一次性提取出各态所需字段。
  const available = payload?.status === "available" ? payload.data : null;
  const upToDate = payload?.status === "upToDate" ? payload.data : null;
  const errorData = payload?.status === "error" ? payload.data : null;
  // checking：Rust 明确回报「检查中」，或窗口刚 mount 还没 pull 到数据。
  const isChecking = payload === null || payload.status === "checking";
  const isAvailable = available !== null;
  const isError = errorData !== null;
  const current = available?.currentVersion ?? upToDate?.currentVersion ?? "";
  const notesMd = available?.notesMd ?? upToDate?.notesMd ?? null;
  const title = isChecking
    ? t("update.titleChecking")
    : isError
      ? t("update.titleError")
      : isAvailable
        ? t("update.titleAvailable")
        : t("update.titleUpToDate");

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-transparent p-2">
      <div className="flex h-full w-full flex-col overflow-hidden rounded-card border border-hairline bg-surface-raised shadow-2xl">
        {/* 顶部：标题 + 关闭 */}
        <div className="flex items-center justify-between border-b border-hairline-soft px-5 py-3">
          <div className="flex items-center gap-2 text-ink-dim">
            <RefreshIcon size={14} className={installing || isChecking ? "animate-spin" : ""} />
            <span className="text-[12px] font-medium">{t("update.dialogTitle")}</span>
          </div>
          <button
            type="button"
            onClick={close}
            aria-label="Close"
            className="rounded-ctl p-1 text-ink-faint transition-colors hover:bg-surface hover:text-ink"
          >
            <CloseIcon size={14} />
          </button>
        </div>

        {/* 主体：图标 + 标题 + 版本 + 说明；窗口固定 800×600，超出时此处滚动，顶栏/底栏不动。
            已是最新态整体紧凑（图标圈缩小、间距压缩）——该态日志最长，优先给说明留空间 */}
        <div className="min-h-0 flex-1 overflow-y-auto px-6 pb-2 pt-4">
          <div
            className={`mx-auto flex items-center justify-center rounded-full ${
              isChecking
                ? "h-12 w-12 bg-info/15 text-info"
                : isError
                  ? "h-12 w-12 bg-danger/15 text-danger"
                  : isAvailable
                    ? "h-12 w-12 bg-accent/15 text-accent"
                    : "h-9 w-9 bg-info/15 text-info"
            }`}
          >
            {isChecking ? (
              <RefreshIcon size={24} className="animate-spin" />
            ) : isError ? (
              <CloseIcon size={24} />
            ) : isAvailable ? (
              <DownloadIcon size={24} />
            ) : (
              <CheckIcon size={18} />
            )}
          </div>

          <h1
            className={`text-center text-[17px] font-semibold tracking-tight text-ink ${
              upToDate ? "mt-2.5" : "mt-4"
            }`}
          >
            {title}
          </h1>

          {/* 版本行 */}
          {isAvailable && available ? (
            <div className="mt-2 flex flex-wrap items-center justify-center gap-2">
              <span className="rounded-full bg-accent/15 px-2.5 py-0.5 font-mono text-[12px] font-medium text-accent">
                v{available.version}
              </span>
              <span className="text-[11px] text-ink-faint">
                {t("update.versionLine", { v: available.version, c: available.currentVersion })}
              </span>
            </div>
          ) : upToDate ? (
            <div className="mt-2 text-center font-mono text-[12px] text-ink-dim">v{current}</div>
          ) : null}

          {available?.date ? (
            <div className="mt-1.5 text-center text-[10px] text-ink-faint">
              {t("update.releasedOn", { d: available.date.slice(0, 10) })}
            </div>
          ) : null}

          {/* 分割线 + 说明标题 */}
          {!isError && notesMd ? (
            <>
              <div className="my-3 border-t border-hairline-soft" />
              <div className="mb-1.5 text-[11px] font-medium uppercase tracking-wide text-ink-faint">
                {t("update.releaseNotes")}
              </div>
              <Markdown source={notesMd} />
            </>
          ) : errorData ? (
            <p className="mt-4 break-words text-center text-[12px] leading-relaxed text-danger">
              {t("update.checkFailed", { err: errorData.message })}
            </p>
          ) : upToDate ? (
            <p className="mt-4 text-center text-[12px] text-ink-faint">{t("update.noNotes")}</p>
          ) : null}

          {/* 安装进度 / 完成 / 安装失败 */}
          {installing ? (
            <div className="mt-4">
              <div className="flex items-center justify-between text-[11px] text-ink-dim">
                <span>{t("update.downloading")}</span>
                {progress?.total ? (
                  <span className="font-mono tabular-nums">
                    {fmt(progress.downloaded)} / {fmt(progress.total)}
                  </span>
                ) : null}
              </div>
              <div className="mt-1.5 h-1.5 w-full overflow-hidden rounded-full bg-surface">
                <div
                  className="h-full rounded-full bg-accent transition-[width] duration-200"
                  style={{
                    width: progress?.total
                      ? `${Math.min(100, (progress.downloaded / progress.total) * 100)}%`
                      : "30%",
                  }}
                />
              </div>
            </div>
          ) : null}
          {done ? (
            <p className="mt-4 text-center text-[12px] text-accent">{t("update.done")}</p>
          ) : null}
          {installError ? (
            <p className="mt-4 break-words text-center text-[12px] text-danger">{installError}</p>
          ) : null}
        </div>

        {/* 底部按钮 */}
        <div className="flex items-center justify-end gap-2 border-t border-hairline-soft px-5 py-3">
          {done || installing ? null : isChecking ? (
            // 检查中：只允许关闭；不留「立即更新」避免误点无结果的按钮
            <button
              type="button"
              onClick={close}
              className="rounded-ctl border border-hairline-soft px-4 py-1.5 text-[12px] text-ink-dim transition-colors hover:border-hairline hover:text-ink"
            >
              {t("update.later")}
            </button>
          ) : isAvailable ? (
            <>
              <button
                type="button"
                onClick={close}
                className="rounded-ctl border border-hairline-soft px-4 py-1.5 text-[12px] text-ink-dim transition-colors hover:border-hairline hover:text-ink"
              >
                {t("update.later")}
              </button>
              <button
                type="button"
                onClick={() => void install()}
                className="flex items-center gap-1.5 rounded-ctl bg-accent px-4 py-1.5 text-[12px] font-medium text-accent-ink transition-opacity hover:opacity-90"
              >
                <DownloadIcon size={13} />
                {t("update.installNow")}
              </button>
            </>
          ) : (
            <button
              type="button"
              onClick={close}
              className="rounded-ctl bg-accent px-5 py-1.5 text-[12px] font-medium text-accent-ink transition-opacity hover:opacity-90"
            >
              {t("update.close")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
