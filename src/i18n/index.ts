/**
 * i18n 运行时（docs/i18n-spec.md §2.2）。
 *
 * 零依赖：catalog 是纯对象，`t` 只是一次字典查找 + 占位符替换。
 * 每个 locale 的扁平字典只构造一次（`i18nFor` 记忆化），组件切换语言时
 * 拿到的仍是同一个实例 —— 引用稳定，`useMemo` 不会白算。
 */
import type {
  KeyParams,
  Locale,
  MessageKey,
  NoParamKey,
  Params,
  PluralKey,
  WithParamKey,
} from "./types";
import { en } from "./messages/en-US";
import { zh } from "./messages/zh-CN";

export interface I18n {
  locale: Locale;
  /**
   * 取文案。缺 key 显示 key 本身，缺参数保留 `{name}` —— 故意难看，便于发现。
   *
   * 两个重载是类型层的硬闸：无参 key 多传参数报错、带参 key 漏传参数也报错。
   */
  t<K extends NoParamKey>(key: K): string;
  t<K extends WithParamKey>(key: K, params: KeyParams<K>): string;
  /** 复数：英文按 Intl.PluralRules 选 `.one` / `.other`；中文恒 `.other` */
  plural(key: PluralKey, n: number, params?: Params): string;
}

/** 嵌套对象拍平成 `{ "disk.title": "…" }` */
export function flatten(
  tree: Record<string, unknown>,
  prefix = "",
  out: Record<string, string> = {},
): Record<string, string> {
  for (const [k, v] of Object.entries(tree)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (typeof v === "string") out[key] = v;
    else if (v && typeof v === "object") flatten(v as Record<string, unknown>, key, out);
  }
  return out;
}

const DICTS: Record<Locale, Record<string, string>> = {
  zh: flatten(zh as unknown as Record<string, unknown>),
  en: flatten(en as unknown as Record<string, unknown>),
};

/** 兜底链：当前语言 → 中文（母语，唯一 source of truth）→ key 本身 */
function lookup(locale: Locale, key: string): string | undefined {
  return DICTS[locale][key] ?? DICTS.zh[key];
}

function interpolate(tpl: string, params?: Params): string {
  if (!params) return tpl;
  return tpl.replace(/\{(\w+)\}/g, (m, name: string) =>
    name in params ? String(params[name]) : m,
  );
}

export function createI18n(locale: Locale): I18n {
  const dict = DICTS[locale];
  const rules = new Intl.PluralRules(locale === "en" ? "en" : "zh");

  const t = ((key: MessageKey, params?: Params): string => {
    const tpl = dict[key] ?? DICTS.zh[key];
    if (tpl == null) {
      // dev 期必须炸出来，prod 期降级为 key（绝不显示空白）
      if (import.meta.env.DEV) console.error(`[i18n] missing key: ${key}`);
      return key;
    }
    return interpolate(tpl, params);
  }) as I18n["t"];

  const plural = (key: PluralKey, n: number, params?: Params): string => {
    const cat = rules.select(n);
    const tpl = lookup(locale, `${key}.${cat}`) ?? lookup(locale, `${key}.other`);
    if (tpl == null) {
      if (import.meta.env.DEV) console.error(`[i18n] missing plural key: ${key}`);
      return key;
    }
    return interpolate(tpl, { n, ...params });
  };

  return { locale, t, plural };
}

/** 记忆化：最多两个实例（zh / en），切换语言不新建对象 */
const cache = new Map<Locale, I18n>();

export function i18nFor(locale: Locale): I18n {
  let inst = cache.get(locale);
  if (!inst) {
    inst = createI18n(locale);
    cache.set(locale, inst);
  }
  return inst;
}

/** 语言名的展示词（「跟随系统」提示里用） */
export function localeName(l: Locale): string {
  return l === "zh" ? "中文" : "English";
}

export type { Locale, MessageKey, Params } from "./types";
