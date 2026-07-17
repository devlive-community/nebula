import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { getPref, setPref } from "./api";
import { DEFAULT_LOCALE, isLocale, localeDef } from "./locales";

/** 语言代码(如 `zh` / `en` / `ja`);具体支持哪些见 [`LOCALES`](./locales)。 */
export type Locale = string;

interface I18nCtx {
  locale: Locale;
  setLocale: (l: Locale) => void;
  /**
   * 翻译。以**中文原文为 key**:当前语言有译文就用,查不到回退中文(不会出现空缺)。
   * 基准语言(zh)无翻译表,原样返回。`params` 用于 `{name}` 形式的占位替换。
   */
  t: (zh: string, params?: Record<string, string | number>) => string;
}

const LocaleContext = createContext<I18nCtx | null>(null);

function applyHtmlLang(code: string) {
  document.documentElement.lang = localeDef(code).htmlLang ?? code;
}

export function LocaleProvider({ children }: { children: ReactNode }) {
  // 语言偏好持久化在 SQLite(ui_prefs 表);先以默认语言渲染,挂载后加载并回填。
  const [locale, setLocaleState] = useState<Locale>(DEFAULT_LOCALE);
  const hydrated = useRef(false);

  const setLocale = useCallback((l: Locale) => {
    if (!isLocale(l)) return;
    setLocaleState(l);
    if (hydrated.current) setPref("locale", l).catch(() => {});
    applyHtmlLang(l);
  }, []);

  useEffect(() => {
    getPref("locale")
      .then((v) => {
        if (v && isLocale(v)) {
          setLocaleState(v);
          applyHtmlLang(v);
        }
      })
      .catch(() => {})
      .finally(() => {
        hydrated.current = true;
      });
  }, []);

  // 当前语言的翻译表(基准语言无表 → undefined,t() 直接回退中文原文)。
  const table = useMemo(() => localeDef(locale).table, [locale]);

  const t = useCallback(
    (zh: string, params?: Record<string, string | number>) => {
      let s = table?.[zh] ?? zh;
      if (params) {
        for (const [k, v] of Object.entries(params)) {
          s = s.split(`{${k}}`).join(String(v));
        }
      }
      return s;
    },
    [table],
  );

  return (
    <LocaleContext.Provider value={{ locale, setLocale, t }}>
      {children}
    </LocaleContext.Provider>
  );
}

export function useI18n(): I18nCtx {
  const ctx = useContext(LocaleContext);
  if (!ctx) throw new Error("useI18n must be used within LocaleProvider");
  return ctx;
}
