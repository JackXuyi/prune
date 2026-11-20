/**
 * 独立「检查更新」弹窗窗口（docs/updater-plan.md）。
 *
 * 版式（output/design/update-eul-mockup.html，W1 已验收）：每态同一骨架，
 * 状态色随态切换（checking=info / available=accent / upToDate=info / error=danger）：
 * - 顶栏 46px：状态色图标芯片 + 窗口名 + 关闭
 * - Hero 92px：App 图标（或 checking 的扫描环）+ 版本跃迁 v旧 → v新 + 状态徽标 + 口径行
 * - 说明区：唯一 flex:1 滚动区，Markdown 渲染；无说明不占位
 * - 底栏 56px：左下载进度（百分比是 downloaded÷total 的推导值，total 未知不显示）/ 右按钮组
 *
 * 四态 + 三个局部态（installing / done / installError 覆盖在 available 上）：
 * - checking：扫描环 + 检查提示；只允许关闭（稍后）
 * - available：立即更新（下载安装并重启）/ 稍后
 * - upToDate：好
 * - error：错误详情卡 + 重试（复调 check_update_dialog，替代旧版只能关窗重开）/ 好
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
import {
  CheckIcon,
  CloseIcon,
  DownloadIcon,
  RefreshIcon,
  ScissorsIcon,
  WarnTriangleIcon,
} from "../components/icons";
import { useI18n } from "../i18n/store";
import {
  DEV_UPDATE_AVAILABLE,
  DEV_UPDATE_CHECKING,
  DEV_UPDATE_DIALOG,
  DEV_UPDATE_ERROR,
} from "../lib/devFixture";
import {
  isTauri,
  updateCheckDialog,
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

/** 次按钮：描边 + dim 文字；主按钮：accent 实底（文字色走 accent-ink 令牌）。 */
const BTN = "inline-flex h-8 shrink-0 items-center gap-1.5 rounded-ctl px-4 text-[12px] font-medium";
const BTN_SECONDARY = `${BTN} border border-hairline-soft text-ink-dim transition-colors hover:border-hairline hover:text-ink`;
const BTN_PRIMARY = `${BTN} bg-accent font-semibold text-accent-ink transition-opacity hover:opacity-90`;

/**
 * 浏览器 dev 预览数据：?checking=1 看「检查中」；?state=available|error 看对应
 * 结果态（installing 是组件局部态，用 ?state=installing 装配）；缺省看 upToDate。
 */
function devPayload(): UpdateDialogPayload {
  const params = new URLSearchParams(window.location.search);
  if (params.get("checking") === "1") return DEV_UPDATE_CHECKING as UpdateDialogPayload;
  switch (params.get("state")) {
    case "available":
      return DEV_UPDATE_AVAILABLE as UpdateDialogPayload;
    case "error":
      return DEV_UPDATE_ERROR as UpdateDialogPayload;
    case "installing":
      // installing 覆盖在 available 上：dev 分支直接返回 available，
      // 局部态由 devInstallFixture 开关在组件内装配。
      return DEV_UPDATE_AVAILABLE as UpdateDialogPayload;
    default:
      return DEV_UPDATE_DIALOG as UpdateDialogPayload;
  }
}

/** 浏览器 dev 是否要预览「下载中」局部态（?state=installing）。 */
function devInstallFixture(): boolean {
  return (
    !isTauri() && new URLSearchParams(window.location.search).get("state") === "installing"
  );
}

/** 版本跃迁：v旧 → v新（Hero 主视觉；currentVersion / version 各只出现一次）。 */
function VersionFlow({ from, to }: { from: string; to: string }) {
  return (
    <div className="flex items-center gap-3.5">
      <span className="font-mono text-[14px] font-medium text-ink-faint">v{from}</span>
      <svg
        width="18"
        height="10"
        viewBox="0 0 24 12"
        fill="none"
        stroke="currentColor"
        strokeWidth={1.8}
        strokeLinecap="round"
        strokeLinejoin="round"
        className="text-ink-faint"
        aria-hidden="true"
      >
        <path d="M2 6h18m0 0-5-5m5 5-5 5" />
      </svg>
      <span className="whitespace-nowrap font-mono text-[26px] font-semibold tabular-nums text-current">
        <span className="mr-1 font-mono text-[12px] font-medium text-ink-faint">v</span>
        {to}
      </span>
    </div>
  );
}

export function UpdateWindow() {
  const { t } = useI18n();
  // 浏览器 dev（?window=update）用假数据预览；Tauri 下走真实 IPC。
  const [payload, setPayload] = useState<UpdateDialogPayload | null>(
    isTauri() ? null : devPayload(),
  );
  const [installing, setInstalling] = useState(devInstallFixture());
  const [progress, setProgress] = useState<Progress | null>(
    // dev 下载中预览：43%（16.4/38.2 MB），与设计稿示意值一致
    devInstallFixture() ? { downloaded: 16_400_000, total: 38_200_000 } : null,
  );
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

  const resetInstallState = useCallback(() => {
    setInstalling(false);
    setDone(false);
    setInstallError(null);
    setProgress(null);
  }, []);

  const close = useCallback(() => {
    resetInstallState();
    // 复位到 checking：窗口是 hide/show 复用、组件不卸载，下次显示时先看到
    // 转圈而不是上一次的旧结果（Rust 侧也同步写了 checking，pull 会再确认）。
    setPayload(isTauri() ? { status: "checking" } : devPayload());
    // 用 Tauri 内置窗口 API 关窗，而非自定义 command：后者依赖 Rust 侧注册，
    // dev 只热重载前端时旧二进制没有该 command，会抛 "Command not found"。
    if (isTauri()) void getCurrentWindow().hide().catch(() => {});
  }, [resetInstallState]);

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

  // 重试 = 复调 check_and_prompt（与托盘/设置页同一入口）：Rust 同步写 checking 态
  // + emit 事件，本窗口经既有 listen 重新拉取，不必关窗重开。
  const retry = useCallback(() => {
    resetInstallState();
    if (isTauri()) {
      void updateCheckDialog().catch(() => {});
    } else {
      setPayload(devPayload());
    }
  }, [resetInstallState]);

  // 直接在 payload 上做类型收窄（discriminated union）；TS 无法把局部 status
  // 变量的收窄传回 payload，故这里一次性提取出各态所需字段。
  const available = payload?.status === "available" ? payload.data : null;
  const upToDate = payload?.status === "upToDate" ? payload.data : null;
  const errorData = payload?.status === "error" ? payload.data : null;
  // checking：Rust 明确回报「检查中」，或窗口刚 mount 还没 pull 到数据。
  const isChecking = payload === null || payload.status === "checking";
  const isAvailable = available !== null;
  const isError = errorData !== null;

  // ---- 状态色：checking=info / available(含下载中/完成)=accent / upToDate=info / error=danger ----
  const tone: "info" | "accent" | "danger" =
    isError || installError ? "danger" : isChecking ? "info" : isAvailable || done ? "accent" : "info";
  const toneText = { info: "text-info", accent: "text-accent", danger: "text-danger" }[tone];
  const toneChip = {
    info: "bg-info/15 text-info",
    accent: "bg-accent/15 text-accent",
    danger: "bg-danger/15 text-danger",
  }[tone];

  // ---- Hero 内容：状态标题 + 版本跃迁 + 一行口径（每个数字都有真实字段对应） ----
  const stateline = done
    ? t("update.done")
    : installing
      ? t("update.downloading")
      : isChecking
        ? t("update.titleChecking")
        : isError
          ? t("update.titleError")
          : isAvailable
            ? t("update.titleAvailable")
            : t("update.titleUpToDate");

  // 徽标只在有独立信息时出现：可更新 / 安装包大小（total 实测，未知不显示不猜）。
  const badge = installing
    ? progress?.total
      ? t("update.pkgSize", { size: fmt(progress.total) })
      : null
    : isAvailable
      ? t("update.stateUpdatable")
      : null;

  const cal = isChecking
    ? t("update.checkHint")
    : isError || installError
      ? t("update.noImpact")
      : installing
        ? t("update.autoRestart")
        : isAvailable && available?.date
          ? t("update.releasedOn", { d: available.date.slice(0, 10) })
          : null;

  const notesMd = available?.notesMd ?? upToDate?.notesMd ?? null;
  // 说明区内容优先级：错误详情卡 > Markdown 说明 > 无内容不占位（诚实口径）。
  const errMessage = installError ?? errorData?.message ?? null;

  const topIco =
    isChecking ? (
      <RefreshIcon size={13} className="animate-spin" />
    ) : isError || installError ? (
      <WarnTriangleIcon size={13} />
    ) : isAvailable || done ? (
      <DownloadIcon size={13} />
    ) : (
      <CheckIcon size={13} />
    );

  // ---- 底栏按钮组（主按钮唯一、语义真实；done 后自动重启，不留按钮） ----
  const footer =
    done || installing ? null : isChecking ? (
      // 检查中：只允许关闭；不留「立即更新」避免误点无结果的按钮
      <button type="button" onClick={close} className={BTN_SECONDARY}>
        {t("update.later")}
      </button>
    ) : isAvailable ? (
      <>
        <button type="button" onClick={close} className={BTN_SECONDARY}>
          {t("update.later")}
        </button>
        <button type="button" onClick={() => void install()} className={BTN_PRIMARY}>
          <DownloadIcon size={13} />
          {t("update.installNow")}
        </button>
      </>
    ) : isError ? (
      <>
        <button type="button" onClick={retry} className={BTN_SECONDARY}>
          <RefreshIcon size={13} />
          {t("update.retry")}
        </button>
        <button type="button" onClick={close} className={BTN_PRIMARY}>
          {t("update.close")}
        </button>
      </>
    ) : (
      <button type="button" onClick={close} className={BTN_PRIMARY}>
        {t("update.close")}
      </button>
    );

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-transparent p-2">
      <div className="flex h-full w-full flex-col overflow-hidden rounded-card border border-hairline bg-surface-raised shadow-2xl">
        {/* ① 顶栏：状态色图标芯片 + 窗口名 + 关闭 */}
        <div className="flex h-[46px] shrink-0 items-center gap-2.5 border-b border-hairline-soft px-3.5">
          <span className={`grid h-6 w-6 shrink-0 place-items-center rounded-[7px] ${toneChip}`}>
            {topIco}
          </span>
          <span className="text-[12.5px] font-medium text-ink-dim">{t("update.dialogTitle")}</span>
          <span className="flex-1" />
          <button
            type="button"
            onClick={close}
            aria-label="Close"
            className="grid h-6 w-6 place-items-center rounded-[7px] text-ink-faint transition-colors hover:bg-surface hover:text-ink"
          >
            <CloseIcon size={12} />
          </button>
        </div>

        {/* ② Hero：App 图标 / 扫描环 + 状态标题 + 版本跃迁 + 一行口径 */}
        <div
          className={`mx-3.5 mt-3 flex h-[92px] shrink-0 items-center gap-4 rounded-[16px] border px-[22px] ${toneText}`}
          style={{
            borderColor: "color-mix(in srgb, currentColor 26%, transparent)",
            background:
              "linear-gradient(120deg, color-mix(in srgb, currentColor 8%, transparent), transparent 72%)",
          }}
        >
          {isChecking ? (
            // 检查中：扫描环（旋转弧）。checking 态 payload 没有当前版本字段，
            // 环心不硬造数字，动效即锚点。
            <svg width="56" height="56" viewBox="0 0 56 56" aria-hidden="true" className="shrink-0">
              <circle cx="28" cy="28" r="23" fill="none" stroke="var(--color-chip)" strokeWidth="4" />
              <circle
                cx="28"
                cy="28"
                r="23"
                fill="none"
                stroke="currentColor"
                strokeWidth="4"
                strokeLinecap="round"
                strokeDasharray="54 90.5"
                transform="rotate(-90 28 28)"
                className="origin-center animate-spin"
                style={{ animationDuration: "1.1s" }}
              />
            </svg>
          ) : (
            // 其余态：App 图标芯片（品牌锚点；用矢量图标而非位图，免引二进制资源）
            <span
              className={`grid h-14 w-14 shrink-0 place-items-center rounded-[14px] border border-hairline-soft bg-surface ${toneText}`}
            >
              <ScissorsIcon size={28} />
            </span>
          )}

          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <b className="whitespace-nowrap text-[15px] font-semibold text-ink">{stateline}</b>
              {badge ? (
                <span
                  className={`shrink-0 rounded-full px-2 py-0.5 text-[9.5px] font-semibold ${toneChip}`}
                >
                  {badge}
                </span>
              ) : null}
            </div>
            <div className="mt-2">
              {available ? (
                <VersionFlow from={available.currentVersion} to={available.version} />
              ) : upToDate ? (
                <span className="font-mono text-[22px] font-semibold tabular-nums text-ink">
                  <span className="mr-1 font-mono text-[12px] font-medium text-ink-faint">v</span>
                  {upToDate.currentVersion}
                </span>
              ) : null}
            </div>
          </div>

          {cal ? (
            <span className="shrink-0 whitespace-nowrap text-[10px] text-ink-faint">{cal}</span>
          ) : null}
        </div>

        {/* ③ 说明区：唯一滚动区；错误详情卡 / Markdown 说明 / 无内容不占位 */}
        <div className="min-h-0 flex-1 overflow-y-auto px-6 pb-1.5 pt-3.5">
          {errMessage ? (
            <div className="mt-1 rounded-card border border-danger/30 bg-danger/5 px-4 py-3">
              <div className="flex items-center gap-2">
                <WarnTriangleIcon size={13} className="text-danger" />
                <b className="text-[12px] font-semibold text-danger">{t("update.titleError")}</b>
              </div>
              <p className="mt-1.5 break-words text-[11.5px] leading-relaxed text-ink-dim">
                {errMessage}
              </p>
            </div>
          ) : notesMd ? (
            <>
              <div className="mb-2 text-[10.5px] font-semibold uppercase tracking-wide text-ink-faint">
                {t("update.releaseNotes")}
              </div>
              <Markdown source={notesMd} />
            </>
          ) : upToDate ? (
            <p className="mt-1 text-[12px] text-ink-faint">{t("update.noNotes")}</p>
          ) : null}
        </div>

        {/* ④ 底栏：左下载进度 / 右按钮组 */}
        <div className="flex h-14 shrink-0 items-center gap-3.5 border-t border-hairline-soft pl-5 pr-3.5">
          {installing ? (
            <div
              className="flex min-w-0 flex-1 flex-col gap-1.5"
              title={t("update.progressTip")}
            >
              <div className="flex items-baseline justify-between">
                <span className="text-[11px] text-ink-dim">{t("update.downloading")}</span>
                <span className="font-mono text-[10.5px] tabular-nums text-ink-faint">
                  {progress?.total ? (
                    <>
                      <b className="mr-1.5 font-mono text-[12px] font-semibold text-accent">
                        {Math.min(100, Math.round((progress.downloaded / progress.total) * 100))}%
                      </b>
                      {fmt(progress.downloaded)} / {fmt(progress.total)}
                    </>
                  ) : (
                    fmt(progress?.downloaded ?? 0)
                  )}
                </span>
              </div>
              <div className="h-[5px] w-full overflow-hidden rounded-full bg-surface">
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
          ) : (
            <span className="flex-1" />
          )}
          {installing && !done ? (
            // 下载中：主按钮位换成不可点的状态反馈，避免「点了没变化」的假按钮
            <button type="button" disabled className={`${BTN_SECONDARY} opacity-55`}>
              {t("update.downloadingBtn")}
            </button>
          ) : (
            footer
          )}
        </div>
      </div>
    </div>
  );
}
