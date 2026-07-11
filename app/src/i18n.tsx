import {
  createContext,
  useCallback,
  useContext,
  useState,
  type ReactNode,
} from "react";
import { EN } from "./translations";

export type Locale = "zh" | "en";

interface I18nCtx {
  locale: Locale;
  setLocale: (l: Locale) => void;
  /**
   * 翻译。以**中文原文为 key**:英文下查 [`EN`] 表,查不到就回退中文(不会出现空缺);
   * 中文下原样返回。`params` 用于 `{name}` 形式的占位替换。
   */
  t: (zh: string, params?: Record<string, string | number>) => string;
}

const LocaleContext = createContext<I18nCtx | null>(null);

const storedLocale = (): Locale =>
  localStorage.getItem("nebula-locale") === "en" ? "en" : "zh";

export function LocaleProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(storedLocale);

  const setLocale = useCallback((l: Locale) => {
    setLocaleState(l);
    localStorage.setItem("nebula-locale", l);
    document.documentElement.lang = l === "zh" ? "zh-CN" : "en";
  }, []);

  const t = useCallback(
    (zh: string, params?: Record<string, string | number>) => {
      let s = locale === "en" ? (EN[zh] ?? zh) : zh;
      if (params) {
        for (const [k, v] of Object.entries(params)) {
          s = s.split(`{${k}}`).join(String(v));
        }
      }
      return s;
    },
    [locale],
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
