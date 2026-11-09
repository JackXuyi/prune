/**
 * D1：批量取应用图标。
 *
 * 为什么是模块级缓存 + hook 而不是 store：图标是**只增不改**的资源，
 * 同一家族在一次会话里不会变；进 store 会多一份全局状态要清，
 * 反而更容易在切 tab 时抖动。
 *
 * 约定：`cache[family] = ""` 表示「已尝试但失败」——本会话不再重试，
 * 避免每帧滚动都拿系统进程去撞一次 IPC。
 */
import { useEffect, useMemo, useRef, useState } from "react";

import type { IconMap, IconQuery } from "../types/appHistory";
import { appIcons, isTauri } from "./ipc";

const cache: Record<string, string> = {};
const pending = new Set<string>();

export function useAppIcons(items: IconQuery[]): IconMap {
  // items 每次渲染都是新数组：用内容 key 作为依赖，避免无限请求
  const key = useMemo(
    () => items.map((i) => `${i.family}\u0000${i.exe ?? ""}`).join("|"),
    [items],
  );
  const [version, setVersion] = useState(0);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    const missing = items.filter(
      (i) => i.exe && cache[i.family] === undefined && !pending.has(i.family),
    );
    if (missing.length === 0) return;

    for (const i of missing) pending.add(i.family);
    void appIcons(missing)
      .then((map) => {
        for (const i of missing) {
          pending.delete(i.family);
          // 失败记空串：本会话不再重试，前端落字母徽标
          cache[i.family] = map[i.family] ?? "";
        }
        if (alive.current) {
          setVersion((v) => v + 1);
        }
      })
      .catch(() => {
        // 整批失败（IPC 不可用）：不写 cache，下次挂载还会再试一次
        for (const i of missing) pending.delete(i.family);
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  return useMemo(() => {
    const out: IconMap = {};
    for (const i of items) {
      const v = cache[i.family];
      if (v) out[i.family] = v;
    }
    return out;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, version]);
}
