import { useCallback } from "react";
import {
  ELLIOTT_PATTERN_MENU_GROUPS,
  type ElliottPatternMenuItem,
  type ElliottPatternMenuToggles,
} from "../lib/elliottPatternMenuCatalog";
import type { ElliottWaveConfig } from "../lib/elliottWaveAppConfig";

type Props = {
  value: ElliottWaveConfig;
  onChange: (next: ElliottWaveConfig) => void;
};

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

  const toggleGroup = useCallback(
    (groupId: string, checked: boolean) => {
      const g = ELLIOTT_PATTERN_MENU_GROUPS.find((x) => x.id === groupId);
      if (!g) return;
      const patch: Partial<ElliottPatternMenuToggles> = {};
      for (const it of g.items) patch[it.id] = checked;
      setMenu(patch);
    },
    [setMenu],
  );

  const renderItem = useCallback(
    (item: ElliottPatternMenuItem) => {
      const checked = value.pattern_menu[item.id];
      return (
        <div
          key={item.id}
          className="tv-elliott-pattern-item"
          style={{
            marginTop: "0.45rem",
            paddingLeft: "0.35rem",
            borderLeft: "2px solid var(--tv-border, rgba(255,255,255,0.12))",
          }}
        >
          <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check" style={{ alignItems: "flex-start" }}>
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
      {ELLIOTT_PATTERN_MENU_GROUPS.map((g) => {
        const allOn = g.items.every((it) => value.pattern_menu[it.id]);
        return (
          <details key={g.id} className="tv-collapsible" open style={{ marginBottom: "0.45rem" }}>
            <summary style={{ cursor: "pointer", listStyle: "none" }}>
              <label
                className="muted tv-elliott-panel__field tv-elliott-panel__field--check"
                style={{ display: "inline-flex", alignItems: "center", gap: "0.35rem" }}
                onClick={(e) => e.stopPropagation()}
              >
                <input type="checkbox" checked={allOn} onChange={(e) => toggleGroup(g.id, e.target.checked)} />
                <span style={{ fontWeight: 700, fontSize: "0.82rem" }}>{g.titleTr}</span>
                <span className="muted" style={{ fontSize: "0.74rem" }}>
                  ({g.titleEn})
                </span>
              </label>
            </summary>
            {g.items.map((it) => renderItem(it))}
          </details>
        );
      })}
    </div>
  );
}
