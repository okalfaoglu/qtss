import type {
  AcpChartPatternsConfig,
  AcpLastPivotMode,
  AcpPatternGroups,
  AcpSizeFilters,
  AcpTriState,
  AcpZigzagRow,
} from "../lib/acpChartPatternsConfig";
import { ACP_OHLC_PRESETS, ACP_PATTERN_ROWS } from "../lib/acpChartPatternsConfig";

type Props = {
  value: AcpChartPatternsConfig;
  onChange: (next: AcpChartPatternsConfig) => void;
  onSaveToDb: () => void;
  saveBusy: boolean;
  saveHint?: string;
  /** `false`: salt okunur — `app_config` yazılamaz (admin değil). */
  allowPersistConfig?: boolean;
};

function row<K extends keyof AcpChartPatternsConfig>(
  cfg: AcpChartPatternsConfig,
  onChange: Props["onChange"],
  key: K,
  val: AcpChartPatternsConfig[K],
) {
  onChange({ ...cfg, [key]: val });
}

export function AcpTrendoscopeSettingsCard({
  value,
  onChange,
  onSaveToDb,
  saveBusy,
  saveHint,
  allowPersistConfig = true,
}: Props) {
  const setScan = (patch: Partial<AcpChartPatternsConfig["scanning"]>) =>
    onChange({ ...value, scanning: { ...value.scanning, ...patch } });
  const setSizeFilters = (patch: Partial<AcpSizeFilters>) =>
    onChange({
      ...value,
      scanning: { ...value.scanning, size_filters: { ...value.scanning.size_filters, ...patch } },
    });
  const setDisplay = (patch: Partial<AcpChartPatternsConfig["display"]>) =>
    onChange({ ...value, display: { ...value.display, ...patch } });
  const setZigRow = (i: number, patch: Partial<AcpZigzagRow>) => {
    const zigzag = value.zigzag.map((r, j) => (j === i ? { ...r, ...patch } : r));
    onChange({ ...value, zigzag });
  };
  const addZigRow = () =>
    onChange({ ...value, zigzag: [...value.zigzag, { enabled: true, length: 5, depth: 34 }] });
  const setPattern = (id: string, patch: Partial<{ enabled: boolean; last_pivot: AcpTriState }>) => {
    const prev = value.patterns[id] ?? { enabled: true, last_pivot: "both" as AcpTriState };
    onChange({
      ...value,
      patterns: { ...value.patterns, [id]: { ...prev, ...patch } },
    });
  };
  const setPatternGroups = (patch: Partial<{
    geometric: Partial<AcpPatternGroups["geometric"]>;
    direction: Partial<AcpPatternGroups["direction"]>;
    formation_dynamics: Partial<AcpPatternGroups["formation_dynamics"]>;
  }>) => {
    const pg = value.pattern_groups;
    onChange({
      ...value,
      pattern_groups: {
        geometric: { ...pg.geometric, ...patch.geometric },
        direction: { ...pg.direction, ...patch.direction },
        formation_dynamics: { ...pg.formation_dynamics, ...patch.formation_dynamics },
      },
    });
  };

  return (
    <div className="card">
      <h3 className="tv-acp__title" style={{ margin: "0 0 0.5rem", fontSize: "1rem", fontWeight: 600 }}>
        ACP [Trendoscope®] — formasyon tarama
      </h3>
      <p className="muted" style={{ marginTop: 0, fontSize: "0.85rem" }}>
        Değerler <code>app_config.acp_chart_patterns</code> anahtarında tutulur.{" "}
        <strong>Veritabanına kaydet</strong> için admin rolü gerekir; aksi hâlde yalnızca bu oturumda düzenlersiniz.
      </p>

      <details open className="tv-acp__details">
        <summary className="muted" style={{ cursor: "pointer", marginTop: "0.75rem" }}>
          Kaynak (OHLC)
        </summary>
        <div className="tv-acp__grid" style={{ marginTop: "0.5rem" }}>
          {(["open", "high", "low", "close"] as const).map((field) => (
            <label key={field} style={{ display: "block" }}>
              <span className="muted" style={{ display: "block", fontSize: "0.75rem" }}>
                {field}
              </span>
              <select
                className="tv-topstrip__select"
                style={{ width: "100%" }}
                value={value.ohlc[field]}
                onChange={(e) =>
                  row(value, onChange, "ohlc", { ...value.ohlc, [field]: e.target.value })
                }
              >
                {ACP_OHLC_PRESETS.map((p) => (
                  <option key={p} value={p}>
                    {p}
                  </option>
                ))}
              </select>
            </label>
          ))}
        </div>
      </details>

      <details open className="tv-acp__details">
        <summary className="muted" style={{ cursor: "pointer", marginTop: "0.75rem" }}>
          Zigzag
        </summary>
        <div style={{ marginTop: "0.5rem", display: "flex", flexDirection: "column", gap: "0.35rem" }}>
          {value.zigzag.map((z, i) => (
            <div key={i} className="tv-acp__zigrow" style={{ display: "flex", flexWrap: "wrap", gap: "0.5rem", alignItems: "center" }}>
              <label style={{ display: "flex", alignItems: "center", gap: "0.35rem" }}>
                <input type="checkbox" checked={z.enabled} onChange={(e) => setZigRow(i, { enabled: e.target.checked })} />
                <span className="muted">length</span>
                <input
                  className="mono"
                  type="number"
                  min={1}
                  max={99}
                  value={z.length}
                  onChange={(e) => setZigRow(i, { length: parseInt(e.target.value, 10) || 1 })}
                  style={{ width: "3.25rem" }}
                />
                <span className="muted">depth</span>
                <input
                  className="mono"
                  type="number"
                  min={1}
                  max={250}
                  value={z.depth}
                  onChange={(e) => setZigRow(i, { depth: parseInt(e.target.value, 10) || 1 })}
                  style={{ width: "3.5rem" }}
                />
              </label>
            </div>
          ))}
          <button type="button" className="theme-toggle" onClick={addZigRow}>
            Zigzag satırı ekle
          </button>
        </div>
      </details>

      <details open className="tv-acp__details">
        <summary className="muted" style={{ cursor: "pointer", marginTop: "0.75rem" }}>
          Tarama (scanning)
        </summary>
        <div className="tv-acp__grid" style={{ marginTop: "0.5rem" }}>
          <label>
            <span className="muted" style={{ display: "block", fontSize: "0.75rem" }}>
              Pivot sayısı
            </span>
            <select
              className="tv-topstrip__select"
              value={value.scanning.number_of_pivots}
              onChange={(e) => setScan({ number_of_pivots: e.target.value === "6" ? 6 : 5 })}
            >
              <option value={5}>5</option>
              <option value={6}>6</option>
            </select>
          </label>
          <label>
            <span className="muted" style={{ display: "block", fontSize: "0.75rem" }}>
              Son pivot yönü (Pine <code>lastPivotDirection</code>)
            </span>
            <select
              className="tv-topstrip__select"
              style={{ minWidth: "6.5rem" }}
              value={value.scanning.last_pivot_direction}
              onChange={(e) =>
                setScan({ last_pivot_direction: e.target.value as AcpLastPivotMode })
              }
            >
              <option value="both">both (filtre yok)</option>
              <option value="up">up (tüm desenler)</option>
              <option value="down">down (tüm desenler)</option>
              <option value="custom">custom (tablo)</option>
            </select>
          </label>
          <label>
            <span className="muted" style={{ display: "block", fontSize: "0.75rem" }}>
              Error threshold (%)
            </span>
            <input
              className="mono"
              type="number"
              min={1}
              max={100}
              value={value.scanning.error_threshold_percent}
              onChange={(e) => setScan({ error_threshold_percent: parseInt(e.target.value, 10) || 20 })}
            />
          </label>
          <label>
            <span className="muted" style={{ display: "block", fontSize: "0.75rem" }}>
              Flat threshold (%)
            </span>
            <input
              className="mono"
              type="number"
              min={1}
              max={100}
              value={value.scanning.flat_threshold_percent}
              onChange={(e) => setScan({ flat_threshold_percent: parseInt(e.target.value, 10) || 20 })}
            />
          </label>
          <label>
            <span className="muted" style={{ display: "block", fontSize: "0.75rem" }}>
              Bar ratio limit
            </span>
            <input
              className="mono"
              type="number"
              step="0.001"
              min={0.01}
              max={0.99}
              value={value.scanning.bar_ratio_limit}
                  onChange={(e) => setScan({ bar_ratio_limit: parseFloat(e.target.value) || 0.25 })}
            />
            <div style={{ display: "flex", gap: "0.35rem", marginTop: "0.35rem", flexWrap: "wrap" }}>
              <button
                type="button"
                className="theme-toggle"
                onClick={() => setScan({ verify_bar_ratio: true, bar_ratio_limit: 0.382 })}
                title="Pine default: allow 0.382..2.618 (strict)."
              >
                Sıkı (0.382)
              </button>
              <button
                type="button"
                className="theme-toggle"
                onClick={() => setScan({ verify_bar_ratio: true, bar_ratio_limit: 0.25 })}
                title="Relaxed: allow 0.25..4.0. Helps asymmetric pivot spacing."
              >
                Gevşek (0.25)
              </button>
              <button
                type="button"
                className="theme-toggle"
                onClick={() => setScan({ verify_bar_ratio: false })}
                title="Disable bar-ratio filter (for troubleshooting)."
              >
                Kapalı
              </button>
            </div>
          </label>
          <label style={{ display: "flex", alignItems: "center", gap: "0.35rem", marginTop: "1.25rem" }}>
            <input
              type="checkbox"
              checked={value.scanning.verify_bar_ratio}
              onChange={(e) => setScan({ verify_bar_ratio: e.target.checked })}
            />
            <span className="muted">Verify bar ratio</span>
          </label>
          <label style={{ display: "flex", alignItems: "center", gap: "0.35rem", marginTop: "1.25rem" }}>
            <input
              type="checkbox"
              checked={value.scanning.ratio_diff_enabled}
              onChange={(e) => setScan({ ratio_diff_enabled: e.target.checked })}
            />
            <span className="muted" title="Pine ratioDiffEnabled — üçlü tepe/dip eğim tutarlılığı (getRatioDiff).">
              Ratio diff filter
            </span>
          </label>
          <label>
            <span className="muted" style={{ display: "block", fontSize: "0.75rem" }}>
              ratio_diff_max (Pine ratioDiff)
            </span>
            <input
              className="mono"
              type="number"
              step="0.01"
              min={0.000001}
              value={value.scanning.ratio_diff_max}
              onChange={(e) => setScan({ ratio_diff_max: parseFloat(e.target.value) || 1 })}
            />
          </label>
          <label style={{ display: "flex", alignItems: "center", gap: "0.35rem", marginTop: "1.25rem" }}>
            <input type="checkbox" checked={value.scanning.avoid_overlap} onChange={(e) => setScan({ avoid_overlap: e.target.checked })} />
            <span className="muted">Avoid overlap</span>
          </label>
          <label style={{ display: "flex", alignItems: "center", gap: "0.35rem", marginTop: "1.25rem" }}>
            <input type="checkbox" checked={value.scanning.repaint} onChange={(e) => setScan({ repaint: e.target.checked })} />
            <span className="muted" title="Açık: son (oluşan) mum taramada kullanılır. Kapalı: TV gibi yalnız kapanmış mumlar.">
              Repaint (açık mum)
            </span>
          </label>
          <label
            style={{
              display: "flex",
              alignItems: "center",
              gap: "0.35rem",
              marginTop: "1.25rem",
              gridColumn: "1 / -1",
            }}
          >
            <input
              type="checkbox"
              checked={value.scanning.auto_scan_on_timeframe_change}
              onChange={(e) => setScan({ auto_scan_on_timeframe_change: e.target.checked })}
            />
            <span className="muted" title="Üst şeritteki timeframe (interval) her değiştiğinde kanal taramasını otomatik çalıştırır.">
              Timeframe değişince otomatik kanal taraması
            </span>
          </label>
          <label>
            <span className="muted" style={{ display: "block", fontSize: "0.75rem" }}>
              pivot_tail_skip_max
            </span>
            <input
              className="mono"
              type="number"
              min={0}
              max={100}
              value={value.scanning.pivot_tail_skip_max}
              onChange={(e) => setScan({ pivot_tail_skip_max: parseInt(e.target.value, 10) || 0 })}
            />
          </label>
          <label>
            <span className="muted" style={{ display: "block", fontSize: "0.75rem" }}>
              max_zigzag_levels
            </span>
            <input
              className="mono"
              type="number"
              min={0}
              max={8}
              value={value.scanning.max_zigzag_levels}
              onChange={(e) => setScan({ max_zigzag_levels: parseInt(e.target.value, 10) || 0 })}
            />
          </label>
          <label style={{ display: "flex", alignItems: "center", gap: "0.35rem", marginTop: "1.25rem", gridColumn: "1 / -1" }}>
            <input
              type="checkbox"
              checked={value.scanning.ignore_if_entry_crossed}
              onChange={(e) => setScan({ ignore_if_entry_crossed: e.target.checked })}
            />
            <span className="muted">
              ignoreIfEntryCrossed — son kapanış üst/alt band aralığında değilse elenir (abstractchartpatterns)
            </span>
          </label>
        </div>
        <div className="tv-acp__grid" style={{ marginTop: "0.75rem" }}>
          <span className="muted" style={{ gridColumn: "1 / -1", fontSize: "0.8rem" }}>
            SizeFilters (Pine <code>checkSize</code>)
          </span>
          <label style={{ display: "flex", alignItems: "center", gap: "0.35rem" }}>
            <input
              type="checkbox"
              checked={value.scanning.size_filters.filter_by_bar}
              onChange={(e) => setSizeFilters({ filter_by_bar: e.target.checked })}
            />
            <span className="muted">filter_by_bar</span>
          </label>
          <label>
            <span className="muted" style={{ display: "block", fontSize: "0.75rem" }}>
              min_pattern_bars
            </span>
            <input
              className="mono"
              type="number"
              min={0}
              value={value.scanning.size_filters.min_pattern_bars}
              onChange={(e) => setSizeFilters({ min_pattern_bars: parseInt(e.target.value, 10) || 0 })}
            />
          </label>
          <label>
            <span className="muted" style={{ display: "block", fontSize: "0.75rem" }}>
              max_pattern_bars
            </span>
            <input
              className="mono"
              type="number"
              min={0}
              value={value.scanning.size_filters.max_pattern_bars}
              onChange={(e) => setSizeFilters({ max_pattern_bars: parseInt(e.target.value, 10) || 1000 })}
            />
          </label>
          <label style={{ display: "flex", alignItems: "center", gap: "0.35rem" }}>
            <input
              type="checkbox"
              checked={value.scanning.size_filters.filter_by_percent}
              onChange={(e) => setSizeFilters({ filter_by_percent: e.target.checked })}
            />
            <span className="muted">filter_by_percent</span>
          </label>
          <label>
            <span className="muted" style={{ display: "block", fontSize: "0.75rem" }}>
              min_pattern_percent
            </span>
            <input
              className="mono"
              type="number"
              step="any"
              value={value.scanning.size_filters.min_pattern_percent}
              onChange={(e) => setSizeFilters({ min_pattern_percent: parseFloat(e.target.value) || 0 })}
            />
          </label>
          <label>
            <span className="muted" style={{ display: "block", fontSize: "0.75rem" }}>
              max_pattern_percent
            </span>
            <input
              className="mono"
              type="number"
              step="any"
              value={value.scanning.size_filters.max_pattern_percent}
              onChange={(e) => setSizeFilters({ max_pattern_percent: parseFloat(e.target.value) || 100 })}
            />
          </label>
        </div>
      </details>

      <details open className="tv-acp__details">
        <summary className="muted" style={{ cursor: "pointer", marginTop: "0.75rem" }}>
          Pattern groups (Pine allowedPatterns)
        </summary>
        <p className="muted" style={{ margin: "0.35rem 0 0", fontSize: "0.78rem" }}>
          Geometri, yön ve dinamik bayraklarının hepsi açık değilse ilgili id’ler elenir; aşağıdaki 1–13 tablosundaki{" "}
          <strong>Aktif</strong> ile kesişir (AND).
        </p>
        <div style={{ marginTop: "0.65rem", display: "flex", flexDirection: "column", gap: "0.65rem" }}>
          <div>
            <div className="muted" style={{ fontSize: "0.78rem", fontWeight: 600, marginBottom: "0.35rem" }}>
              Geometric shapes
            </div>
            <div style={{ display: "flex", flexWrap: "wrap", gap: "0.5rem 1rem" }}>
              <label style={{ display: "flex", alignItems: "center", gap: "0.35rem" }}>
                <input
                  type="checkbox"
                  checked={value.pattern_groups.geometric.channels}
                  onChange={(e) => setPatternGroups({ geometric: { channels: e.target.checked } })}
                />
                <span className="muted">Channels</span>
              </label>
              <label style={{ display: "flex", alignItems: "center", gap: "0.35rem" }}>
                <input
                  type="checkbox"
                  checked={value.pattern_groups.geometric.wedges}
                  onChange={(e) => setPatternGroups({ geometric: { wedges: e.target.checked } })}
                />
                <span className="muted">Takoz</span>
              </label>
              <label style={{ display: "flex", alignItems: "center", gap: "0.35rem" }}>
                <input
                  type="checkbox"
                  checked={value.pattern_groups.geometric.triangles}
                  onChange={(e) => setPatternGroups({ geometric: { triangles: e.target.checked } })}
                />
                <span className="muted">Üçgen</span>
              </label>
            </div>
          </div>
          <div>
            <div className="muted" style={{ fontSize: "0.78rem", fontWeight: 600, marginBottom: "0.35rem" }}>
              Direction
            </div>
            <div style={{ display: "flex", flexWrap: "wrap", gap: "0.5rem 1rem" }}>
              <label style={{ display: "flex", alignItems: "center", gap: "0.35rem" }}>
                <input
                  type="checkbox"
                  checked={value.pattern_groups.direction.rising}
                  onChange={(e) => setPatternGroups({ direction: { rising: e.target.checked } })}
                />
                <span className="muted">Rising</span>
              </label>
              <label style={{ display: "flex", alignItems: "center", gap: "0.35rem" }}>
                <input
                  type="checkbox"
                  checked={value.pattern_groups.direction.falling}
                  onChange={(e) => setPatternGroups({ direction: { falling: e.target.checked } })}
                />
                <span className="muted">Düşüş</span>
              </label>
              <label style={{ display: "flex", alignItems: "center", gap: "0.35rem" }}>
                <input
                  type="checkbox"
                  checked={value.pattern_groups.direction.flat_bidirectional}
                  onChange={(e) => setPatternGroups({ direction: { flat_bidirectional: e.target.checked } })}
                />
                <span className="muted">Flat / Bi-Directional</span>
              </label>
            </div>
          </div>
          <div>
            <div className="muted" style={{ fontSize: "0.78rem", fontWeight: 600, marginBottom: "0.35rem" }}>
              Formation dynamics
            </div>
            <div style={{ display: "flex", flexWrap: "wrap", gap: "0.5rem 1rem" }}>
              <label style={{ display: "flex", alignItems: "center", gap: "0.35rem" }}>
                <input
                  type="checkbox"
                  checked={value.pattern_groups.formation_dynamics.expanding}
                  onChange={(e) => setPatternGroups({ formation_dynamics: { expanding: e.target.checked } })}
                />
                <span className="muted">Expanding</span>
              </label>
              <label style={{ display: "flex", alignItems: "center", gap: "0.35rem" }}>
                <input
                  type="checkbox"
                  checked={value.pattern_groups.formation_dynamics.contracting}
                  onChange={(e) => setPatternGroups({ formation_dynamics: { contracting: e.target.checked } })}
                />
                <span className="muted">Contracting</span>
              </label>
              <label style={{ display: "flex", alignItems: "center", gap: "0.35rem" }}>
                <input
                  type="checkbox"
                  checked={value.pattern_groups.formation_dynamics.parallel}
                  onChange={(e) => setPatternGroups({ formation_dynamics: { parallel: e.target.checked } })}
                />
                <span className="muted">Parallel</span>
              </label>
            </div>
          </div>
        </div>
      </details>

      <details open className="tv-acp__details">
        <summary className="muted" style={{ cursor: "pointer", marginTop: "0.75rem" }}>
          Formasyonlar (1–13) — TV &quot;Specific Pattern Filters&quot;
        </summary>
        <p className="muted" style={{ margin: "0.35rem 0 0", fontSize: "0.78rem" }}>
          <strong>Son pivot</strong> sütunu yalnızca tarama bölümünde <strong>custom</strong> seçiliyken API’ye gider;{" "}
          <code>both</code>/<code>up</code>/<code>down</code> modlarında Pine’daki gibi genel yön geçerlidir.
        </p>
        <div style={{ marginTop: "0.5rem", overflowX: "auto" }}>
          <table className="tv-acp__table" style={{ width: "100%", borderCollapse: "collapse", fontSize: "0.8rem" }}>
            <thead>
              <tr>
                <th className="muted" style={{ textAlign: "left", padding: "0.25rem" }}>
                  #
                </th>
                <th className="muted" style={{ textAlign: "left", padding: "0.25rem" }}>
                  Ad
                </th>
                <th className="muted" style={{ padding: "0.25rem" }}>
                  Aktif
                </th>
                <th className="muted" style={{ textAlign: "left", padding: "0.25rem" }}>
                  Son pivot
                </th>
              </tr>
            </thead>
            <tbody>
              {ACP_PATTERN_ROWS.map(({ id, label }) => {
                const k = String(id);
                const p = value.patterns[k] ?? { enabled: true, last_pivot: "both" as AcpTriState };
                return (
                  <tr key={id}>
                    <td className="mono" style={{ padding: "0.2rem" }}>
                      {id}
                    </td>
                    <td style={{ padding: "0.2rem", maxWidth: "12rem" }}>{label}</td>
                    <td style={{ padding: "0.2rem", textAlign: "center" }}>
                      <input type="checkbox" checked={p.enabled} onChange={(e) => setPattern(k, { enabled: e.target.checked })} />
                    </td>
                    <td style={{ padding: "0.2rem" }}>
                      <select
                        className="tv-topstrip__select"
                        value={p.last_pivot}
                        onChange={(e) => setPattern(k, { last_pivot: e.target.value as AcpTriState })}
                      >
                        <option value="both">both</option>
                        <option value="up">up</option>
                        <option value="down">down</option>
                      </select>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </details>

      <details open className="tv-acp__details">
        <summary className="muted" style={{ cursor: "pointer", marginTop: "0.75rem" }}>
          Görünüm + çoklu formasyon
        </summary>
        <div className="tv-acp__grid" style={{ marginTop: "0.5rem" }}>
          <label>
            <span className="muted" style={{ display: "block", fontSize: "0.75rem" }}>
              Tema (çizim API)
            </span>
            <select
              className="tv-topstrip__select"
              value={value.display.theme}
              onChange={(e) => setDisplay({ theme: e.target.value === "light" ? "light" : "dark" })}
            >
              <option value="dark">DARK</option>
              <option value="light">LIGHT</option>
            </select>
          </label>
          <label>
            <span className="muted" style={{ display: "block", fontSize: "0.75rem" }}>
              Pattern line width
            </span>
            <input
              className="mono"
              type="number"
              min={1}
              max={8}
              value={value.display.pattern_line_width}
              onChange={(e) => setDisplay({ pattern_line_width: parseInt(e.target.value, 10) || 2 })}
            />
          </label>
          <label>
            <span className="muted" style={{ display: "block", fontSize: "0.75rem" }}>
              Zigzag line width
            </span>
            <input
              className="mono"
              type="number"
              min={1}
              max={8}
              value={value.display.zigzag_line_width}
              onChange={(e) => setDisplay({ zigzag_line_width: parseInt(e.target.value, 10) || 1 })}
            />
          </label>
          <label>
            <span className="muted" style={{ display: "block", fontSize: "0.75rem" }}>
              Max patterns
            </span>
            <input
              className="mono"
              type="number"
              min={1}
              max={32}
              value={value.display.max_patterns}
              onChange={(e) => setDisplay({ max_patterns: parseInt(e.target.value, 10) || 1 })}
            />
            <span className="muted" style={{ display: "block", fontSize: "0.65rem", marginTop: "0.2rem" }}>
              API <code>max_matches</code> — geçmişte çoklu eşleşme için &gt;1; tarama +{" "}
              <code>pivot_tail_skip_max</code> birlikte kullanın.
            </span>
          </label>
          <label style={{ display: "flex", alignItems: "center", gap: "0.35rem", marginTop: "1rem" }}>
            <input
              type="checkbox"
              checked={value.display.show_pattern_label}
              onChange={(e) => setDisplay({ show_pattern_label: e.target.checked })}
            />
            <span className="muted">Pattern label</span>
          </label>
          <label style={{ display: "flex", alignItems: "center", gap: "0.35rem", marginTop: "1rem" }}>
            <input
              type="checkbox"
              checked={value.display.show_pivot_labels}
              onChange={(e) => setDisplay({ show_pivot_labels: e.target.checked })}
            />
            <span className="muted">Pivot labels</span>
          </label>
          <label style={{ display: "flex", alignItems: "center", gap: "0.35rem", marginTop: "1rem" }}>
            <input type="checkbox" checked={value.display.show_zigzag} onChange={(e) => setDisplay({ show_zigzag: e.target.checked })} />
            <span className="muted">Zigzag</span>
          </label>
        </div>
      </details>

      <label style={{ display: "block", marginTop: "0.75rem" }}>
        <span className="muted" style={{ display: "block", fontSize: "0.75rem" }}>
          Calculated bars (taramaya gönderilecek son N mum)
        </span>
        <input
          className="mono"
          type="number"
          min={50}
          max={50000}
          value={value.calculated_bars}
          onChange={(e) =>
            onChange({ ...value, calculated_bars: parseInt(e.target.value, 10) || 5000 })
          }
          style={{ width: "100%", maxWidth: "10rem" }}
        />
      </label>

      <div style={{ marginTop: "1rem", display: "flex", flexWrap: "wrap", gap: "0.5rem", alignItems: "center" }}>
        <button
          type="button"
          className="theme-toggle"
          disabled={saveBusy || !allowPersistConfig}
          title={allowPersistConfig ? undefined : "Yalnızca admin rolü app_config yazabilir"}
          onClick={() => onSaveToDb()}
        >
          {saveBusy ? "Kaydediliyor…" : "Veritabanına kaydet (admin)"}
        </button>
        {saveHint ? <span className="muted" style={{ fontSize: "0.85rem" }}>{saveHint}</span> : null}
      </div>
    </div>
  );
}
