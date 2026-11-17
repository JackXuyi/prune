/**
 * `update` namespace：面板底部更新卡（docs/updater-plan.md）。
 */
import type { MessageTree } from "../../types";

const zh = {
  availablePrefix: "发现新版本",
  currentVersion: "（当前 v{v}）",
  installNow: "立即更新",
  ignore: "本次忽略",
  downloading: "正在下载更新…",
  done: "更新完成，正在重启…",
  checkFailed: "检查更新失败：{err}",
  upToDate: "已是最新版本",
  checking: "检查中…",
  check: "检查更新",
  // 更新弹窗（独立 update 窗口）
  dialogTitle: "检查更新",
  titleChecking: "正在检查更新…",
  titleAvailable: "发现新版本",
  titleUpToDate: "已是最新版本",
  titleError: "检查更新失败",
  versionLine: "v{v} · 当前 v{c}",
  releaseNotes: "更新说明",
  noNotes: "（本版本暂无更新说明）",
  later: "稍后",
  close: "好",
  releasedOn: "发布于 {d}",
} as const;

const en: MessageTree<typeof zh> = {
  availablePrefix: "Version",
  currentVersion: " (current v{v})",
  installNow: "Update now",
  ignore: "Later",
  downloading: "Downloading update…",
  done: "Update installed, restarting…",
  checkFailed: "Update check failed: {err}",
  upToDate: "You're up to date",
  checking: "Checking…",
  check: "Check for updates",
  // Update dialog (dedicated update window)
  dialogTitle: "Check for Updates",
  titleChecking: "Checking for updates…",
  titleAvailable: "Update Available",
  titleUpToDate: "You're Up to Date",
  titleError: "Update Check Failed",
  versionLine: "v{v} · current v{c}",
  releaseNotes: "Release Notes",
  noNotes: "(No release notes for this version)",
  later: "Later",
  close: "OK",
  releasedOn: "Released {d}",
};

export const update = { zh, en } as const;
