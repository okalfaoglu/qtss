import { useEffect, useMemo } from "react";

import { filterHelpTopics, HELP_TOPICS } from "./helpCatalog";

type Props = {
  /** Çekmece arama kutusu (süzgeç). Boşsa tüm konular. */
  query: string;
  /** Açılışta scroll / details aç */
  focusTopicId: string | null;
};

export function HelpPanel({ query, focusTopicId }: Props) {
  const topics = useMemo(() => filterHelpTopics(query), [query]);

  useEffect(() => {
    if (!focusTopicId) return;
    const t = window.setTimeout(() => {
      const wrap = document.getElementById(`help-topic-${focusTopicId}`);
      if (!wrap) return;
      const det = wrap.querySelector("details");
      if (det) det.open = true;
      wrap.scrollIntoView({ block: "nearest", behavior: "smooth" });
    }, 80);
    return () => window.clearTimeout(t);
  }, [focusTopicId]);

  return (
    <div className="help-panel card">
      <p className="tv-drawer__section-head">Yardım ve SSS</p>
      <p className="muted" style={{ fontSize: "0.78rem", lineHeight: 1.45, marginBottom: "0.55rem" }}>
        Çekmece üstündeki arama kutusu bu başlıkları süzer ({HELP_TOPICS.length} konu). Kısayol düğmeleri ilgili panelden buraya
        bağlanır.
      </p>
      {topics.length === 0 ? <p className="muted">Aramanızla eşleşen konu yok — süzmeyi azaltın.</p> : null}
      <div className="help-panel__topics">
        {topics.map((t) => (
          <div key={t.id} id={`help-topic-${t.id}`} className="help-panel__topic">
            <details>
              <summary className="help-panel__summary">{t.title}</summary>
              <div className="help-panel__body">{t.body}</div>
            </details>
          </div>
        ))}
      </div>
    </div>
  );
}
