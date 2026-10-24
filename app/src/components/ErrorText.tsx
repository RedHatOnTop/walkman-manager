import type { ErrorReport } from "../api/types";
import { usePrefs, type StringKey } from "../i18n";

/** A failed command's sentence in the owner's language; the backend's English only when the
 * app has no sentence for it. */
export function errorSentence(error: ErrorReport, t: ReturnType<typeof usePrefs>["t"]): string {
  const key = `err.${error.say ?? error.kind}`;
  const said = t(key as StringKey, error.vars);
  return said === key ? error.message : said;
}

/** The sentence, then the value it is about (a path, an address, a tool's words) as is. */
export function ErrorText({ error }: { error: ErrorReport }) {
  const { t } = usePrefs();
  return (
    <>
      {errorSentence(error, t)}
      {error.detail && <span className="err-detail selectable">{error.detail}</span>}
    </>
  );
}
