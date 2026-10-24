import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { dictionaries, type Lang, type StringKey } from "./strings";

export type ThemePref = "system" | "dark" | "light";

interface Prefs {
  lang: Lang;
  theme: ThemePref;
  setLang(l: Lang): void;
  setTheme(t: ThemePref): void;
  t(key: StringKey, vars?: Record<string, string | number>): string;
}

const PrefsContext = createContext<Prefs | null>(null);

function read<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v && (allowed as readonly string[]).includes(v) ? (v as T) : fallback;
  } catch {
    return fallback;
  }
}

function write(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Preferences are a convenience; the app works without storage.
  }
}

export function PrefsProvider({ children }: { children: ReactNode }) {
  const systemLang: Lang = navigator.language.toLowerCase().startsWith("ko") ? "ko" : "en";
  const [lang, setLangState] = useState<Lang>(() => read("uwm.lang", ["ko", "en"] as const, systemLang));
  const [theme, setThemeState] = useState<ThemePref>(() => read("uwm.theme", ["system", "dark", "light"] as const, "system"));

  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);

  useEffect(() => {
    const root = document.documentElement;
    if (theme === "system") root.removeAttribute("data-theme");
    else root.setAttribute("data-theme", theme);
  }, [theme]);

  const setLang = useCallback((l: Lang) => {
    setLangState(l);
    write("uwm.lang", l);
  }, []);
  const setTheme = useCallback((t: ThemePref) => {
    setThemeState(t);
    write("uwm.theme", t);
  }, []);

  const t = useCallback(
    (key: StringKey, vars?: Record<string, string | number>) => {
      let s: string = dictionaries[lang][key] ?? key;
      if (vars) for (const [k, v] of Object.entries(vars)) s = s.replaceAll(`{${k}}`, String(v));
      return s;
    },
    [lang],
  );

  const value = useMemo(() => ({ lang, theme, setLang, setTheme, t }), [lang, theme, setLang, setTheme, t]);
  return <PrefsContext.Provider value={value}>{children}</PrefsContext.Provider>;
}

export function usePrefs(): Prefs {
  const ctx = useContext(PrefsContext);
  if (!ctx) throw new Error("usePrefs outside PrefsProvider");
  return ctx;
}

export function useT() {
  return usePrefs().t;
}

export type { StringKey, Lang };
