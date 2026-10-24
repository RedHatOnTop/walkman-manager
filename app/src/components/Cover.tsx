import { useState } from "react";
import { useLibrary } from "../state/library";

/** An album cover from the art cache, or a drawn blank with the title's first letter. */
export function Cover({ art, title }: { art: string | null; title: string }) {
  const { artUrl } = useLibrary();
  const [failed, setFailed] = useState(false);
  const src = art && !failed ? artUrl(art) : "";
  if (src) return <img className="cover" src={src} alt="" loading="lazy" decoding="async" draggable={false} onError={() => setFailed(true)} />;
  const initial = Array.from(title.trim())[0]?.toUpperCase() ?? "";
  return (
    <div className="cover-blank" aria-hidden="true">
      <span>{initial}</span>
    </div>
  );
}
