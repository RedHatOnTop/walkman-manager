import { Icon } from "../components/Icon";
import { GetTools, usePlatformTools } from "../components/PlatformTools";
import { usePrefs, useT, type ThemePref } from "../i18n";
import type { Lang } from "../i18n/strings";
import { useData } from "../state/data";
import { Section } from "./device/sections";

function Segmented<T extends string>({ value, options, onChange, label }: { value: T; options: [T, string][]; onChange(v: T): void; label: string }) {
  return (
    <div className="segmented" role="radiogroup" aria-label={label}>
      {options.map(([v, text]) => (
        <button key={v} role="radio" aria-checked={v === value} className={v === value ? "is-on" : ""} onClick={() => onChange(v)}>
          {text}
        </button>
      ))}
    </div>
  );
}

export function Settings() {
  const t = useT();
  const { lang, setLang, theme, setTheme } = usePrefs();
  const { env } = useData();
  const tools = usePlatformTools();
  const pt = tools.view;
  const adb = pt ? pt.adb : (env?.adbPath ?? null);
  return (
    <div className="page settings">
      <header className="page-head">
        <div className="page-titles">
          <h1 className="page-title">{t("settings.title")}</h1>
        </div>
      </header>

      <div className="doc">
        <Section kind="theme" title={t("settings.theme")}>
          <Segmented<ThemePref>
            label={t("settings.theme")}
            value={theme}
            onChange={setTheme}
            options={(["system", "dark", "light"] as const).map((v) => [v, t(`settings.theme.${v}`)])}
          />
        </Section>

        <Section kind="language" title={t("settings.language")}>
          <Segmented<Lang> label={t("settings.language")} value={lang} onChange={setLang} options={[["ko", "한국어"], ["en", "English"]]} />
        </Section>

        <Section kind="tools" title={t("settings.tools")}>
          <dl className="specs specs--tools">
            <div className="spec">
              <dt>{t("settings.adb")}</dt>
              <dd>
                {adb ? (
                  <span className="tool-ok">
                    <Icon name="check" size={14} />
                    <span className="mono selectable">{adb}</span>
                    {env?.adbVersion && env.adbPath === adb && <span className="mono tool-version">{env.adbVersion}</span>}
                  </span>
                ) : (
                  <span className="flag flag--warn">
                    <Icon name="alert" size={14} />
                    {t("settings.adbMissing")}
                  </span>
                )}
              </dd>
            </div>
            {pt && (
              <div className="spec">
                <dt>{t("settings.fastboot")}</dt>
                <dd>
                  {pt.fastboot ? (
                    <span className="tool-ok">
                      <Icon name={pt.flasher === "formats" ? "check" : "alert"} size={14} />
                      <span className="mono selectable">{pt.fastboot}</span>
                      <span className="tool-version">{t(pt.flasher === "formats" ? "settings.fastbootFormats" : "settings.fastbootWrites")}</span>
                    </span>
                  ) : (
                    <span className="flag flag--warn">
                      <Icon name="alert" size={14} />
                      {t("settings.fastbootMissing")}
                    </span>
                  )}
                </dd>
              </div>
            )}
            {pt && (
              <div className="spec">
                <dt>{t("settings.platformTools")}</dt>
                <dd>{pt.installed ? <span className="tool-ok">{t("settings.platformToolsHave", { revision: pt.installed })}</span> : <GetTools tools={tools} />}</dd>
              </div>
            )}
          </dl>
          <p className="sec-note">{t("settings.toolsNote")}</p>
        </Section>

        <Section kind="about" title={t("settings.about")}>
          <dl className="specs">
            <div className="spec">
              <dt>{t("settings.version")}</dt>
              <dd className="mono">{env?.version ?? "—"}</dd>
            </div>
          </dl>
          <p className="sec-note settings-about">{t("settings.aboutBody")}</p>
        </Section>
      </div>
    </div>
  );
}
