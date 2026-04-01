import { useCallback } from "react";
import {
  ELLIOTT_PATTERN_MENU_PANEL_SECTIONS,
  ELLIOTT_PATTERN_MENU_ROWS,
  type ElliottPatternMenuRow,
  type ElliottPatternMenuToggles,
} from "../lib/elliottPatternMenuCatalog";
import type { ElliottWaveConfig } from "../lib/elliottWaveAppConfig";

type Props = {
  value: ElliottWaveConfig;
  onChange: (next: ElliottWaveConfig) => void;
};

function rowsForSection(section: ElliottPatternMenuRow["section"]): readonly ElliottPatternMenuRow[] {
  return ELLIOTT_PATTERN_MENU_ROWS.filter((r) => r.section === section);
}

export function ElliottPatternMenuPanel({ value, onChange }: Props) {
  const setMenu = useCallback(
    (patch: Partial<ElliottPatternMenuToggles>) => {
      const motiveImpulse =
        typeof patch.motive_impulse === "boolean"
          ? patch.motive_impulse
          : value.pattern_menu.motive_impulse;
      onChange({
        ...value,
        formations: { ...value.formations, impulse: motiveImpulse },
        pattern_menu: { ...value.pattern_menu, ...patch },
      });
    },
    [onChange, value],
  );

  const toggleSection = useCallback(
    (sectionId: string, checked: boolean) => {
      const sec = ELLIOTT_PATTERN_MENU_PANEL_SECTIONS.find((x) => x.id === sectionId);
      if (!sec) return;
      const patch: Partial<ElliottPatternMenuToggles> = {};
      for (const id of sec.toggleIds) patch[id] = checked;
      setMenu(patch);
    },
    [setMenu],
  );

  const renderToggleRow = useCallback(
    (item: Extract<ElliottPatternMenuRow, { type: "toggle" }>) => {
      const checked = value.pattern_menu[item.id];
      return (
        <div
          key={item.id}
          className="tv-elliott-pattern-item"
          style={{
            marginTop: "0.35rem",
            paddingLeft: `${0.25 + item.depth * 0.65}rem`,
            borderLeft: "2px solid var(--tv-border, rgba(255,255,255,0.12))",
          }}
        >
          <label
            className="muted tv-elliott-panel__field tv-elliott-panel__field--check"
            style={{ alignItems: "flex-start" }}
          >
            <input
              type="checkbox"
              checked={checked}
              onChange={(e) => setMenu({ [item.id]: e.target.checked } as Partial<ElliottPatternMenuToggles>)}
            />
            <span>
              <strong style={{ fontWeight: 600 }}>{item.titleTr}</strong>
              {item.titleEn ? <span className="muted"> — {item.titleEn}</span> : null}
              {item.structure ? (
                <span className="mono muted" style={{ fontSize: "0.72rem", marginLeft: "0.35rem" }}>
                  {item.structure}
                </span>
              ) : null}
            </span>
          </label>
        </div>
      );
    },
    [setMenu, value.pattern_menu],
  );

  return (
    <div className="tv-elliott-pattern-menu" aria-label="Elliott dalga türleri menüsü">
      {ELLIOTT_PATTERN_MENU_PANEL_SECTIONS.map((sec) => {
        const rows = rowsForSection(sec.id);
        const allOn = sec.toggleIds.every((id) => value.pattern_menu[id]);
        return (
          <details key={sec.id} className="tv-collapsible" open style={{ marginBottom: "0.45rem" }}>
            <summary style={{ cursor: "pointer", listStyle: "none" }}>
              <label
                className="muted tv-elliott-panel__field tv-elliott-panel__field--check"
                style={{ display: "inline-flex", alignItems: "center", gap: "0.35rem" }}
                onClick={(e) => e.stopPropagation()}
              >
                <input type="checkbox" checked={allOn} onChange={(e) => toggleSection(sec.id, e.target.checked)} />
                <span style={{ fontWeight: 700, fontSize: "0.82rem" }}>{sec.titleTr}</span>
                <span className="muted" style={{ fontSize: "0.74rem" }}>
                  ({sec.titleEn})
                </span>
              </label>
            </summary>
            {rows.map((r) =>
              r.type === "label" ? (
                <div
                  key={r.id}
                  style={{
                    marginTop: "0.42rem",
                    paddingLeft: `${0.2 + r.depth * 0.65}rem`,
                    fontWeight: r.depth <= 1 ? 650 : 600,
                    fontSize: r.depth === 0 ? "0.8rem" : "0.76rem",
                  }}
                >
                  {r.titleTr}
                  <span className="muted" style={{ marginLeft: "0.35rem", fontWeight: 500, fontSize: "0.72rem" }}>
                    {r.titleEn}
                  </span>
                </div>
              ) : (
                renderToggleRow(r)
              ),
            )}
          </details>
        );
      })}
    </div>
  );
}
