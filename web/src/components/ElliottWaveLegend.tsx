import { useState } from "react";
import type { ElliottLegendRow } from "../lib/elliottWaveLegend";

type Props = {
  rows: ElliottLegendRow[];
  visible: boolean;
};

export function ElliottWaveLegend({ rows, visible }: Props) {
  if (!visible || rows.length === 0) return null;
  const [open, setOpen] = useState(false);

  return (
    <div className={`tv-elliott-help ${open ? "is-open" : "is-closed"}`} aria-label="Elliott dalga yardımı">
      <button
        type="button"
        className="tv-elliott-help__btn"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        aria-label={open ? "Elliott yardımını kapat" : "Elliott yardımını aç"}
        title={open ? "Kapat" : "Yardım"}
      >
        ?
      </button>
      {open ? (
        <div className="tv-elliott-legend card" aria-label="Elliott dalga lejantı">
          <ul className="tv-elliott-legend__list">
            {rows.map((r) => (
              <li key={r.id} className="tv-elliott-legend__item">
                <span className={`tv-elliott-legend__swatch tv-elliott-legend__swatch--${r.swatch}`} aria-hidden />
                <div>
                  <div className="tv-elliott-legend__row-title">{r.title}</div>
                  <div className="tv-elliott-legend__row-detail muted">{r.detail}</div>
                </div>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </div>
  );
}
