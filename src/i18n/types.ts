/**
 * i18n 类型层（docs/i18n-spec.md §2.3，L2 严格档）。
 *
 * 这里的目标不是「能翻」，而是**漏翻必须编译失败**：
 * - `MessageKey` 由 zh catalog 派生 → 拼错 key 报错
 * - `Args<K>` 由模板串里的 `{name}` 反推 → 漏传 / 多传参数报错
 * - en catalog 用 `MessageTree<typeof zh>` 约束 → 少一个 key、多一个 key、
 *   值写成函数都报错（见 messages/namespaces/*.ts）
 *
 * 为什么不用 i18next：450 个 key 全量打进桌面包，懒加载与插件生态都用不上；
 * 而「编译期发现漏翻」正是本项目最看重的性质，自研 + 类型体操最省事。
 */
import type { zh } from "./messages/zh-CN";

/** 唯一的规范形态：两值。`auto` 只存在于设置层，不进 Locale */
export type Locale = "zh" | "en";

/** 设置层取值域（含 auto） */
export type LocalePref = "auto" | Locale;

export type LocaleSource = "auto" | "manual";

export type Params = Record<string, string | number>;

/** 嵌套 → 点分联合：{ disk: { title: "…" } } → "disk.title" */
type FlatKeys<T, P extends string = ""> = {
  [K in keyof T & string]: T[K] extends string
    ? `${P}${K}`
    : FlatKeys<T[K], `${P}${K}.`>;
}[keyof T & string];

export type MessageKey = FlatKeys<typeof zh>;

/** 从模板串抽参数名：`{count} 项` → "count" */
type ExtractParams<S> = S extends `${string}{${infer P}}${infer R}`
  ? P | ExtractParams<R>
  : never;

/**
 * 嵌套 → `{ key: "点分 key"; tpl: "模板串" }` 的联合。
 *
 * 为什么不做成 `{ "点分 key": "模板串" }` 的单对象再下标：那是个**联合**（每个
 * 成员只有一个键），`Union[K]` 在 TS 里拿不到值 —— 实测整个参数校验会静默退化成
 * `never`，看着有类型、其实什么都没拦住。改成按 key 抽取才真的能算出来。
 */
type FlatOf<T, P extends string = ""> = {
  [K in keyof T & string]: T[K] extends string
    ? { key: `${P}${K}`; tpl: T[K] }
    : FlatOf<T[K], `${P}${K}.`>;
}[keyof T & string];

type FlatMessages = FlatOf<typeof zh>;

/** 取某个 key 的模板串 */
type TemplateOf<K> = Extract<FlatMessages, { key: K }> extends { tpl: infer S } ? S : never;

/** `K extends keyof FlatMessages` 这一层判断是给 tsc 看的：直接下标它证明不了 */
type ParamNames<K extends MessageKey> = K extends MessageKey
  ? ExtractParams<TemplateOf<K>>
  : never;

/**
 * 复数条目的前缀：`unit.items.one` / `unit.items.other` → `"unit.items"`。
 * `plural()` 只接受这种前缀，避免把普通 key 传给复数接口。
 */
export type PluralKey = {
  [K in MessageKey]: K extends `${infer P}.other` ? P : never;
}[MessageKey];

/** 具名参数是必填的：模板里有 `{n}` 就必须传 `{ n: … }` */
export type Args<K extends MessageKey> = [ParamNames<K>] extends [never]
  ? []
  : [args: Record<ParamNames<K>, string | number>];

/**
 * 按「有没有参数」把 key 劈成两组。
 *
 * 为什么不用 `t<K>(key: K, ...args: Args<K>)`：泛型剩余参数遇到条件类型时
 * TS 会延迟求值，**漏传参数不会被发现**（实测只拦住「多传」和「拼错 key」）。
 * 拆成两个重载后参数个数是硬性的，`t("common.unit.coresApprox")` 直接编译失败。
 */
export type NoParamKey = {
  [K in MessageKey]: [ParamNames<K>] extends [never] ? K : never;
}[MessageKey];

export type WithParamKey = Exclude<MessageKey, NoParamKey>;

/** 某个 key 的具名参数表（重载里用） */
export type KeyParams<K extends MessageKey> = Record<ParamNames<K>, string | number>;

/**
 * 结构约束：把字面量类型放宽成 `string`，其余结构保持。
 *
 * 不能直接写 `en: typeof zh` —— `as const` 之后值是字面量类型，
 * 那样等于要求英文文案与中文一字不差。
 */
export type MessageTree<T> = {
  readonly [K in keyof T]: T[K] extends string ? string : MessageTree<T[K]>;
};

/** 由 zh 派生的结构约束，en 侧用 `satisfies Messages` 校验完整性 */
export type Messages = MessageTree<typeof zh>;
