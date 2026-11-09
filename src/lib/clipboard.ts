/**
 * 写剪贴板的三段兜底（D7）。
 *
 * 顺序不能反：Tauri 自定义协议下 `navigator.clipboard` 常被拒，
 * `execCommand("copy")` 又要求同步用户手势——而摘要文本要先拼装，
 * 手势已失效。所以原生命令优先，Web 环境才走浏览器 API。
 */
import { clipboardWrite, isTauri } from "./ipc";

function viaDom(text: string): boolean {
  try {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.setAttribute("readonly", "");
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    return ok;
  } catch {
    return false;
  }
}

export async function writeClipboard(text: string): Promise<boolean> {
  if (!text) return false;

  if (isTauri()) {
    try {
      const n = await clipboardWrite(text);
      if (n > 0) return true;
    } catch {
      // 落到下面的浏览器兜底（dev / 平台未实现）
    }
  }

  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // Safari / 无权限：继续 execCommand
  }

  return viaDom(text);
}
