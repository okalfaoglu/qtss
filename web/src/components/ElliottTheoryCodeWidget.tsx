import { useMemo, useState } from "react";
import type { ElliottTheoryCodeFinding, ElliottTheoryCodeStatus } from "../lib/elliottTheoryCodeReport";
import { ELLIOTT_THEORY_CODE_FINDINGS } from "../lib/elliottTheoryCodeReport";
import {
  ELLIOTT_BEAR_MIRROR_NOTE,
  ELLIOTT_COMBINATION_SUMMARY,
  ELLIOTT_ENDING_DIAGONAL_SUMMARY,
  ELLIOTT_FIBONACCI_CYCLE_NOTE,
  ELLIOTT_FLAT_SUMMARY,
  ELLIOTT_IMPULSE_ADDITIONAL_CONSTRAINTS,
  ELLIOTT_IMPULSE_FIBONACCI_GUIDELINES,
  ELLIOTT_IMPULSE_GUIDELINE_BULLETS,
  ELLIOTT_IMPULSE_INVIOLABLE_RULES,
  ELLIOTT_LEADING_DIAGONAL_SUMMARY,
  ELLIOTT_POSITION_SUBSTRUCTURE,
  ELLIOTT_TRIANGLE_SUMMARY,
  ELLIOTT_WAVE_DEGREES_CLASSICAL,
  ELLIOTT_WAVE_REFERENCE_INTRO,
  ELLIOTT_ZIGZAG_SUMMARY,
} from "../lib/elliottWaveTheoryReference";

type StatusFilter = ElliottTheoryCodeStatus | "all";
type PanelView = "drift" | "reference";

function statusLabel(status: ElliottTheoryCodeStatus): string {
  if (status === "aligned") return "Aligned";
  if (status === "partial") return "Partial";
  return "Missing";
}

function statusMark(status: ElliottTheoryCodeStatus): string {
  if (status === "aligned") return "✓";
  if (status === "partial") return "⚠";
  return "✗";
}

function countByStatus(items: readonly ElliottTheoryCodeFinding[]): Record<ElliottTheoryCodeStatus, number> {
  const out: Record<ElliottTheoryCodeStatus, number> = { aligned: 0, partial: 0, missing: 0 };
  for (const it of items) out[it.status] += 1;
  return out;
}

export function ElliottTheoryCodeWidget() {
  const [panelView, setPanelView] = useState<PanelView>("drift");
  const [filter, setFilter] = useState<StatusFilter>("all");
  const [selectedId, setSelectedId] = useState<string>(ELLIOTT_THEORY_CODE_FINDINGS[0]?.id ?? "");

  const counts = useMemo(() => countByStatus(ELLIOTT_THEORY_CODE_FINDINGS), []);
  const filtered = useMemo(() => {
    if (filter === "all") return ELLIOTT_THEORY_CODE_FINDINGS;
    return ELLIOTT_THEORY_CODE_FINDINGS.filter((x) => x.status === filter);
  }, [filter]);

  const selected = useMemo(() => {
    const first = filtered[0];
    const byId = filtered.find((x) => x.id === selectedId);
    return byId ?? first ?? null;
  }, [filtered, selectedId]);

  return (
    <section className="tv-elliott-theory-code" aria-label="Elliott theory vs code drift">
      <div className="tv-elliott-theory-code__head">
        <div>
          <p className="tv-elliott-theory-code__title">Teori ↔ Kod (drift denetimi)</p>
          <p className="muted tv-elliott-theory-code__subtitle">
            {panelView === "drift"
              ? "Satıra tıklayın: teori metni, koddaki karşılık, etki ve referanslar. Özet sayılar `elliottTheoryCodeReport.ts` kaydına göre üstteki KPI ile hesaplanır (kod değişince drift dosyasını güncelleyin)."
              : "Klasik kurallar ve Fibonacci rehberi (EN — özet). Yatırım tavsiyesi değildir."}
          </p>
        </div>
        <div className="tv-elliott-theory-code__kpis mono" aria-label="Summary">
          {panelView === "drift" ? (
            <>
              <span className="tv-elliott-theory-code__kpi">
                ✓ {counts.aligned} aligned
              </span>
              <span className="tv-elliott-theory-code__kpi">
                ⚠ {counts.partial} partial
              </span>
              <span className="tv-elliott-theory-code__kpi">
                ✗ {counts.missing} missing
              </span>
            </>
          ) : (
            <span className="tv-elliott-theory-code__kpi">Reference</span>
          )}
        </div>
      </div>

      <div className="tv-elliott-theory-code__viewseg" role="tablist" aria-label="Panel view">
        <button
          type="button"
          role="tab"
          aria-selected={panelView === "drift"}
          className={`tv-elliott-theory-code__viewbtn ${panelView === "drift" ? "is-active" : ""}`}
          onClick={() => setPanelView("drift")}
        >
          Drift
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={panelView === "reference"}
          className={`tv-elliott-theory-code__viewbtn ${panelView === "reference" ? "is-active" : ""}`}
          onClick={() => setPanelView("reference")}
        >
          Rules &amp; Fib (EN)
        </button>
      </div>

      {panelView === "reference" ? (
        <div className="tv-elliott-theory-code__reference" aria-label="Elliott reference">
          <p className="muted tv-elliott-theory-code__refintro">{ELLIOTT_WAVE_REFERENCE_INTRO}</p>

          <details className="tv-elliott-theory-code__refblock" open>
            <summary>Inviolable impulse rules (3)</summary>
            <ol className="tv-elliott-theory-code__refol">
              {ELLIOTT_IMPULSE_INVIOLABLE_RULES.map((r) => (
                <li key={r.id}>{r.text}</li>
              ))}
            </ol>
          </details>

          <details className="tv-elliott-theory-code__refblock">
            <summary>Additional structural constraints</summary>
            <ul className="tv-elliott-theory-code__reful">
              {ELLIOTT_IMPULSE_ADDITIONAL_CONSTRAINTS.map((t, i) => (
                <li key={i}>{t}</li>
              ))}
            </ul>
          </details>

          <details className="tv-elliott-theory-code__refblock">
            <summary>Fibonacci guidelines (impulse)</summary>
            <div className="tv-elliott-theory-code__tablewrap">
              <table className="tv-elliott-theory-code__table">
                <thead>
                  <tr>
                    <th>Wave</th>
                    <th>vs</th>
                    <th>Common ratios</th>
                    <th>Notes</th>
                  </tr>
                </thead>
                <tbody>
                  {ELLIOTT_IMPULSE_FIBONACCI_GUIDELINES.map((row) => (
                    <tr key={row.wave}>
                      <td>{row.wave}</td>
                      <td>{row.measuredAgainst}</td>
                      <td className="mono">{row.commonRatios}</td>
                      <td className="muted">{row.notes ?? "—"}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <ul className="tv-elliott-theory-code__reful">
              {ELLIOTT_IMPULSE_GUIDELINE_BULLETS.map((t, i) => (
                <li key={i}>{t}</li>
              ))}
            </ul>
          </details>

          <details className="tv-elliott-theory-code__refblock">
            <summary>Leading diagonal</summary>
            <ul className="tv-elliott-theory-code__reful">
              {ELLIOTT_LEADING_DIAGONAL_SUMMARY.map((t, i) => (
                <li key={i}>{t}</li>
              ))}
            </ul>
          </details>

          <details className="tv-elliott-theory-code__refblock">
            <summary>Ending diagonal</summary>
            <ul className="tv-elliott-theory-code__reful">
              {ELLIOTT_ENDING_DIAGONAL_SUMMARY.map((t, i) => (
                <li key={i}>{t}</li>
              ))}
            </ul>
          </details>

          <details className="tv-elliott-theory-code__refblock">
            <summary>Zigzag · Flat · Triangle · Combination</summary>
            <p className="tv-elliott-theory-code__refh">Zigzag</p>
            <ul className="tv-elliott-theory-code__reful">
              {ELLIOTT_ZIGZAG_SUMMARY.map((t, i) => (
                <li key={`z-${i}`}>{t}</li>
              ))}
            </ul>
            <p className="tv-elliott-theory-code__refh">Flat</p>
            <ul className="tv-elliott-theory-code__reful">
              {ELLIOTT_FLAT_SUMMARY.map((t, i) => (
                <li key={`f-${i}`}>{t}</li>
              ))}
            </ul>
            <p className="tv-elliott-theory-code__refh">Triangle</p>
            <ul className="tv-elliott-theory-code__reful">
              {ELLIOTT_TRIANGLE_SUMMARY.map((t, i) => (
                <li key={`t-${i}`}>{t}</li>
              ))}
            </ul>
            <p className="tv-elliott-theory-code__refh">Combination</p>
            <ul className="tv-elliott-theory-code__reful">
              {ELLIOTT_COMBINATION_SUMMARY.map((t, i) => (
                <li key={`c-${i}`}>{t}</li>
              ))}
            </ul>
          </details>

          <details className="tv-elliott-theory-code__refblock">
            <summary>Position substructure (next lower degree)</summary>
            <div className="tv-elliott-theory-code__tablewrap">
              <table className="tv-elliott-theory-code__table">
                <thead>
                  <tr>
                    <th>Position</th>
                    <th>Required</th>
                    <th>Allowed</th>
                  </tr>
                </thead>
                <tbody>
                  {ELLIOTT_POSITION_SUBSTRUCTURE.map((row) => (
                    <tr key={row.position}>
                      <td>{row.position}</td>
                      <td>{row.requiredStructure}</td>
                      <td className="muted">{row.allowedPatterns}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </details>

          <details className="tv-elliott-theory-code__refblock">
            <summary>Degrees &amp; fractal Fibonacci</summary>
            <p className="muted mono" style={{ fontSize: "0.72rem", marginTop: 0 }}>
              {ELLIOTT_WAVE_DEGREES_CLASSICAL.join(" → ")}
            </p>
            <p className="muted" style={{ fontSize: "0.74rem", marginTop: "0.45rem" }}>
              {ELLIOTT_FIBONACCI_CYCLE_NOTE}
            </p>
            <p className="muted" style={{ fontSize: "0.74rem", marginTop: "0.35rem" }}>
              {ELLIOTT_BEAR_MIRROR_NOTE}
            </p>
          </details>
        </div>
      ) : null}

      {panelView === "drift" ? (
        <>
      <div className="tv-elliott-theory-code__filters">
        <button
          type="button"
          className={`tv-elliott-theory-code__pill ${filter === "all" ? "is-active" : ""}`}
          onClick={() => setFilter("all")}
        >
          All
        </button>
        <button
          type="button"
          className={`tv-elliott-theory-code__pill ${filter === "aligned" ? "is-active" : ""}`}
          onClick={() => setFilter("aligned")}
        >
          Aligned
        </button>
        <button
          type="button"
          className={`tv-elliott-theory-code__pill ${filter === "partial" ? "is-active" : ""}`}
          onClick={() => setFilter("partial")}
        >
          Partial
        </button>
        <button
          type="button"
          className={`tv-elliott-theory-code__pill ${filter === "missing" ? "is-active" : ""}`}
          onClick={() => setFilter("missing")}
        >
          Missing
        </button>
      </div>

      <div className="tv-elliott-theory-code__grid">
        <div className="tv-elliott-theory-code__list" role="listbox" aria-label="Findings list">
          {filtered.map((it) => {
            const active = selected?.id === it.id;
            return (
              <button
                key={it.id}
                type="button"
                role="option"
                aria-selected={active}
                className={`tv-elliott-theory-code__row ${active ? "is-active" : ""}`}
                onClick={() => setSelectedId(it.id)}
              >
                <span className={`tv-elliott-theory-code__mark tv-elliott-theory-code__mark--${it.status}`}>
                  {statusMark(it.status)}
                </span>
                <span className="tv-elliott-theory-code__rowtext">
                  <span className="tv-elliott-theory-code__rowtitle">{it.title}</span>
                  <span className="muted tv-elliott-theory-code__rowsummary">{it.summary}</span>
                </span>
                <span className="tv-elliott-theory-code__tag mono">{statusLabel(it.status)}</span>
              </button>
            );
          })}
        </div>

        <div className="tv-elliott-theory-code__detail" aria-label="Finding details">
          {selected ? (
            <>
              <div className="tv-elliott-theory-code__detailhead">
                <p className="tv-elliott-theory-code__detailtitle">{selected.title}</p>
                <span className={`tv-elliott-theory-code__badge tv-elliott-theory-code__badge--${selected.status} mono`}>
                  {statusMark(selected.status)} {statusLabel(selected.status)}
                </span>
              </div>

              <div className="tv-elliott-theory-code__detailblocks">
                <div className="tv-elliott-theory-code__block">
                  <p className="tv-elliott-theory-code__blocktitle">Theory</p>
                  <p className="muted tv-elliott-theory-code__blockbody">{selected.theory}</p>
                </div>
                <div className="tv-elliott-theory-code__block">
                  <p className="tv-elliott-theory-code__blocktitle">Code</p>
                  <p className="muted tv-elliott-theory-code__blockbody">{selected.code}</p>
                </div>
                <div className="tv-elliott-theory-code__block">
                  <p className="tv-elliott-theory-code__blocktitle">Impact</p>
                  <p className="muted tv-elliott-theory-code__blockbody">{selected.impact}</p>
                </div>
              </div>

              {selected.references?.length ? (
                <div className="tv-elliott-theory-code__refs">
                  <p className="tv-elliott-theory-code__blocktitle">References</p>
                  <ul className="tv-elliott-theory-code__reflist mono">
                    {selected.references.map((r, i) => (
                      <li key={`${r.file}-${i}`}>
                        <code>{r.file}</code>
                        {r.note ? <span className="muted"> — {r.note}</span> : null}
                      </li>
                    ))}
                  </ul>
                </div>
              ) : null}
            </>
          ) : (
            <p className="muted" style={{ margin: 0 }}>
              No findings in this filter.
            </p>
          )}
        </div>
      </div>
        </>
      ) : null}
    </section>
  );
}

