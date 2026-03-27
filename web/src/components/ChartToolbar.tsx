export type ChartTool =
  | "crosshair"
  | "hline"
  | "vline"
  | "trend"
  | "ray"
  | "rect"
  | "fibo"
  | "calc";

type Props = {
  active: ChartTool;
  onSelect: (t: ChartTool) => void;
  onClearDrawings: () => void;
  /** Sol şerit (TradingView tarzı) */
  variant?: "horizontal" | "vertical";
};

const tools: { id: ChartTool; label: string; title: string }[] = [
  { id: "crosshair", label: "✛", title: "İmleç / çapraz" },
  { id: "hline", label: "—", title: "Yatay çizgi (fiyat)" },
  { id: "vline", label: "│", title: "Dikey çizgi (zaman)" },
  { id: "trend", label: "╱", title: "Trend çizgisi (2 nokta)" },
  { id: "ray", label: "↗", title: "Ray (2 nokta, sağa uzar)" },
  { id: "rect", label: "▭", title: "Bölge kutusu (2 nokta)" },
  { id: "fibo", label: "ɸ", title: "Fibonacci geri çekilme" },
  { id: "calc", label: "₿", title: "Kar / zarar hesabı" },
];

export function ChartToolbar({ active, onSelect, onClearDrawings, variant = "horizontal" }: Props) {
  const vert = variant === "vertical";
  return (
    <div
      className={`tv-chart-toolbar${vert ? " tv-chart-toolbar--vertical" : ""}`}
      role="toolbar"
      aria-label="Grafik araçları"
    >
      {tools.map((t) => (
        <button
          key={t.id}
          type="button"
          className={`tv-tool-btn${active === t.id ? " tv-tool-btn--active" : ""}`}
          title={t.title}
          onClick={() => onSelect(t.id)}
        >
          <span className="tv-tool-btn__glyph">{t.label}</span>
        </button>
      ))}
      <div className="tv-chart-toolbar__sep" aria-hidden />
      <button type="button" className="tv-tool-btn tv-tool-btn--ghost" title="Tüm çizimleri temizle" onClick={onClearDrawings}>
        <span className="tv-tool-btn__glyph">⌫</span>
      </button>
    </div>
  );
}
