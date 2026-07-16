import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { EN } from "./translations";
import { getPref, setPref } from "./api";

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

export function LocaleProvider({ children }: { children: ReactNode }) {
  // 语言偏好持久化在 SQLite(ui_prefs 表);先以中文渲染,挂载后加载并回填。
  const [locale, setLocaleState] = useState<Locale>("zh");
  const hydrated = useRef(false);

  const setLocale = useCallback((l: Locale) => {
    setLocaleState(l);
    if (hydrated.current) setPref("locale", l).catch(() => {});
    document.documentElement.lang = l === "zh" ? "zh-CN" : "en";
  }, []);

  useEffect(() => {
    getPref("locale")
      .then((v) => {
        if (v === "en" || v === "zh") {
          setLocaleState(v);
          document.documentElement.lang = v === "zh" ? "zh-CN" : "en";
        }
      })
      .catch(() => {})
      .finally(() => {
        hydrated.current = true;
      });
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
