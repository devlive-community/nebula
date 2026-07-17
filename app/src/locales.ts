/**
 * 语言注册表 —— 新增一门语言**只改这一个文件**。
 *
 * 设计:以**中文原文为 key**。中文(zh)是基准,不需要翻译表;其它语言各给一张
 * `Record<中文, 译文>`,查不到的 key 自动回退中文(渐进覆盖,永不半坏)。
 *
 * ## 新增一门语言(以「法语 fr」为例)
 * 1. 新建 `translations/fr.ts`,`export const FR: Record<string, string> = { 取消: "Annuler", … }`
 *    (照抄 `translations/en.ts` 的 key,逐行翻译;没译的行删掉即可,会回退中文)。
 * 2. 在下面 `LOCALES` 数组里加一项:`{ code: "fr", label: "Français", table: FR }`。
 * 3. 完成 —— 设置里的语言下拉、持久化、回退全部自动生效,无需改其它任何代码。
 */
import { EN } from "./translations/en";
import { JA } from "./translations/ja";

/** 一门语言的定义:代码、下拉里显示的名字、翻译表(zh 为基准,无表)。 */
export interface LocaleDef {
  /** BCP-47 简码,如 `zh` / `en` / `ja`。 */
  code: string;
  /** 语言选择器里显示的名字(用该语言自己的写法)。 */
  label: string;
  /** 中文原文 → 译文;`zh` 为基准语言,无需翻译表。 */
  table?: Record<string, string>;
  /** `<html lang>` 用的值,默认取 `code`。 */
  htmlLang?: string;
}

/** 所有支持的语言。**加语言就往这里加一项。** */
export const LOCALES: LocaleDef[] = [
  { code: "zh", label: "中文", htmlLang: "zh-CN" },
  { code: "en", label: "English", table: EN },
  { code: "ja", label: "日本語", table: JA },
];

/** 默认语言(首次启动 / 未设置偏好时)。 */
export const DEFAULT_LOCALE = "zh";

const BY_CODE = new Map(LOCALES.map((l) => [l.code, l]));

/** 是否为受支持的语言代码。 */
export function isLocale(code: string): boolean {
  return BY_CODE.has(code);
}

/** 取某语言定义(无则回退默认)。 */
export function localeDef(code: string): LocaleDef {
  return BY_CODE.get(code) ?? BY_CODE.get(DEFAULT_LOCALE)!;
}
