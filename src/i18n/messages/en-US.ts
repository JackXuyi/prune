/**
 * 英文 catalog。`satisfies Messages` 是硬闸：
 * 少一个 key、多一个 key、值不是 string —— `tsc` 直接失败。
 *
 * 各 namespace 文件内部已经用 `MessageTree<typeof zh>` 校验过一次；
 * 这里的 `satisfies` 是第二道：防止聚合层漏挂 namespace。
 */
import type { Messages } from "../types";
import { common } from "./namespaces/common";
import { nav } from "./namespaces/nav";
import { settings } from "./namespaces/settings";
import { stats } from "./namespaces/stats";
import { update } from "./namespaces/update";

export const en = {
  common: common.en,
  nav: nav.en,
  settings: settings.en,
  stats: stats.en,
  update: update.en,
} as const satisfies Messages;
