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
};

export const update = { zh, en } as const;
