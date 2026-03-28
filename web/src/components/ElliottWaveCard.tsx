import { useCallback, useMemo, useState } from "react";
import {
  ELLIOTT_DEGREE_LEVELS,
  ELLIOTT_IMPULSE_RULES,
  ELLIOTT_IMPULSE_RULE_ENGINE_SCOPE,
  ELLIOTT_REFERENCE_RANKIA_URL,
  ELLIOTT_REFERENCE_TXT_URL,
  ELLIOTT_SUBDIVISION_RULE,
  ELLIOTT_TEZ_2531_INTRO_PARAGRAPHS,
  ELLIOTT_TEZ_2532_EXTENSION,
  ELLIOTT_TEZ_2533_FAILED_FIFTH,
  ELLIOTT_TEZ_EXTENDED_SECTIONS,
  ELLIOTT_TEZ_IMPULSE_GUIDELINES,
  ELLIOTT_TECHNICAL_ANALYSIS_SUMMARY,
  elliottImpulseRuleScopeLabelTr,
  type ElliottTezSection,
} from "../lib/elliottRulesCatalog";
import { buildSwingPivots } from "../lib/elliottImpulseDetect";
import type { ChartOhlcRow } from "../lib/marketBarsToCandles";
import type { ElliottWaveConfig } from "../lib/elliottWaveAppConfig";

function elliottHexForColorInput(hex: string): string {
  const t = hex.trim();
  if (/^#[0-9A-Fa-f]{6}$/i.test(t)) return t.toLowerCase();
  if (/^#[0-9A-Fa-f]{3}$/i.test(t)) {
    const a = t.slice(1);
    return `#${a[0]}${a[0]}${a[1]}${a[1]}${a[2]}${a[2]}`.toLowerCase();
  }
  return "#e53935";
}
import { chartOhlcRowsSortedChrono } from "../lib/chartRowsToOhlcBars";
import type { ElliottEngineOutputV2 } from "../lib/elliottEngineV2";

function TezExtendedBlock({ section }: { section: ElliottTezSection }) {
  return (
    <section className="tv-elliott-tez-block" aria-labelledby={`tez-h-${section.id}`}>
      <p className="muted" id={`tez-h-${section.id}`} style={{ fontWeight: 600, marginTop: "0.45rem" }}>
        {section.heading}
      </p>
      {section.paragraphs?.map((t, i) => (
        <p key={i} className="muted" style={{ fontSize: "0.72rem", marginTop: "0.3rem", lineHeight: 1.45 }}>
          {t}
        </p>
      ))}
      {section.bullets?.length ? (
        <ul className="muted" style={{ fontSize: "0.72rem", lineHeight: 1.45, marginTop: "0.25rem" }}>
          {section.bullets.map((b, i) => (
            <li key={i}>{b}</li>
          ))}
        </ul>
      ) : null}
      {section.rules?.length ? (
        <>
          <p className="muted" style={{ fontWeight: 600, marginTop: "0.35rem" }}>
            {section.rulesHeading ?? "Kurallar"}
          </p>
          <ul className="muted" style={{ fontSize: "0.72rem", lineHeight: 1.45, marginTop: "0.2rem" }}>
            {section.rules.map((r) => (
              <li key={r.id}>{r.text}</li>
            ))}
          </ul>
        </>
      ) : null}
      {section.guidelines?.length ? (
        <>
          <p className="muted" style={{ fontWeight: 600, marginTop: "0.35rem" }}>
            {section.guidelinesHeading ?? "Rehber İlkeler"}
          </p>
          <ul className="muted" style={{ fontSize: "0.72rem", lineHeight: 1.45, marginTop: "0.2rem" }}>
            {section.guidelines.map((g) => (
              <li key={g.id}>{g.text}</li>
            ))}
          </ul>
        </>
      ) : null}
    </section>
  );
}

type Props = {
  value: ElliottWaveConfig;
  onChange: (next: ElliottWaveConfig) => void;
  bars: ChartOhlcRow[] | null;
  /** Elliott V2 ZigZag + panel swing sayımı için kullanılan derinlik (mum). */
  effectiveSwingDepth: number;
  v2Output?: ElliottEngineOutputV2 | null;
  loadErr: string;
  saveErr: string;
  saveBusy: boolean;
  refreshBusy: boolean;
  onSaveToDb: () => void;
  onRefreshFromServer: () => void;
  /** `false`: veritabanına yazma kapalı (admin değil). */
  allowPersistConfig?: boolean;
  /**
   * `impulse` / `corrective`: menü sekmeleri — yalnız ilgili çizim ve parametreler; JSON/kurallar yok.
   * İçerik `details` ile açılıp kapanabilir.
   */
  layout?: "full" | "impulse" | "corrective";
};

export function ElliottWaveCard({
  value,
  onChange,
  bars,
  effectiveSwingDepth,
  v2Output,
  loadErr,
  saveErr,
  saveBusy,
  refreshBusy,
  onSaveToDb,
  onRefreshFromServer,
  allowPersistConfig = true,
  layout = "full",
}: Props) {
  const [rulesOpen, setRulesOpen] = useState(false);
  const [jsonOpen, setJsonOpen] = useState(false);
  const splitMenuLayout = layout === "impulse" || layout === "corrective";
  const showImpulseUi = layout === "full" || layout === "impulse";
  const showCorrectiveUi = layout === "full" || layout === "corrective";
  const pivotCount = useMemo(() => {
    if (!value.enabled || !bars?.length) return 0;
    return buildSwingPivots(chartOhlcRowsSortedChrono(bars), effectiveSwingDepth).length;
  }, [bars, value.enabled, effectiveSwingDepth]);

  const v2StateNote = v2Output
    ? [
        v2Output.hierarchy.macro ? `4h:${v2Output.hierarchy.macro.decision}` : null,
        v2Output.hierarchy.intermediate ? `1h:${v2Output.hierarchy.intermediate.decision}` : null,
        v2Output.hierarchy.micro ? `15m:${v2Output.hierarchy.micro.decision}` : null,
      ]
        .filter(Boolean)
        .join(" · ")
    : "";
  const v2FailedChecks = useMemo(() => {
    const order = [v2Output?.hierarchy.macro, v2Output?.hierarchy.intermediate, v2Output?.hierarchy.micro];
    const first = order.find((s) => s?.impulse?.checks?.length);
    if (!first?.impulse) return [];
    return first.impulse.checks.filter((c) => !c.passed);
  }, [v2Output]);
  const v2AllChecks = useMemo(() => {
    const order = [v2Output?.hierarchy.macro, v2Output?.hierarchy.intermediate, v2Output?.hierarchy.micro];
    const first = order.find((s) => s?.impulse?.checks?.length);
    return first?.impulse?.checks ?? [];
  }, [v2Output]);
  const hardIds = useMemo(
    () =>
      new Set([
        "structure",
        "w2_not_beyond_w1_start",
        "w3_not_shortest_135",
        "w4_no_overlap_w1",
        "w4_not_longer_than_w3",
      ]),
    [],
  );
  const v2Hard = useMemo(() => v2AllChecks.filter((c) => hardIds.has(c.id)), [hardIds, v2AllChecks]);
  const v2Soft = useMemo(() => v2AllChecks.filter((c) => !hardIds.has(c.id)), [hardIds, v2AllChecks]);
  const v2CheckLabel = useCallback((id: string): string => {
    switch (id) {
      case "structure":
        return "Pivot yapısı 6 dalgalı itkiye uygun değil";
      case "w2_not_beyond_w1_start":
        return "Dalga 2, Dalga 1 başlangıcını geçti";
      case "w3_not_shortest_135":
        return "Dalga 3, 1/3/5 arasında en kısa olamaz";
      case "w4_no_overlap_w1":
        return "Dalga 4, Dalga 1 alanına girdi";
      case "w4_not_longer_than_w3":
        return "Dalga 4, Dalga 3’ten uzun (standart itkı)";
      case "trend_shape":
        return "Trend şekli zayıf / teyitsiz";
      case "extension_w5_vs_w3":
        return "Dalga 5 / Dalga 3 uzantısı (✓ = aştı; ○ = kısaltılmış beşinci olası)";
      case "triangle_alt":
        return "Üçgen: a-b-c-d-e alternasyonu bozuk";
      case "triangle_tightening":
        return "Üçgen: daralma koşulu zayıf";
      case "triangle_converging":
        return "Üçgen: üst/alt kanal çizgileri yakınsamıyor";
      case "triangle_envelope_contract":
        return "Üçgen: zarf daralması yetersiz";
      case "triangle_expanding":
        return "Üçgen: genişleyen zarf (daralan değil)";
      case "tri_r2_channel":
        return "Üçgen (tri_r2): A–C / B–D şeridi dışında tepe/dip";
      case "tri_r2_e_deviation_15":
        return "Üçgen (tri_r2): E çizgi sapması > %15";
      case "tri_r3_apex_after_e":
        return "Üçgen (tri_r3): bant kesişimi E sonundan önce";
      case "tri_r4_not_parallel":
        return "Üçgen (tri_r4): A–C ve B–D paralel";
      case "tri_r7_expanding_shortest_ab":
        return "Üçgen (tri_r7): genişleyende en kısa dalga A/B değil";
      case "tri_r1_substructure_note":
        return "Üçgen: alt derece 3’lü yapı zigzag’ta yok (bilgi)";
      case "triangle_context_wave2_wave4_post":
        return "Üçgen: bağlam (dalga 2 / 4 / post)";
      case "triangle_context_wave4_or_b":
        return "Üçgen: yalnız wave4/B bağlamında geçerlidir";
      case "comb_alt":
        return "Kombinasyon: W-X-Y alternasyonu bozuk";
      case "comb_progress":
        return "Kombinasyon: trend yönünde ilerleme yok";
      case "comb_r1":
        return "Kombinasyon (comb_r1): üç parça / X–W bandı uyumsuz";
      case "comb_r2":
        return "Kombinasyon (comb_r2): X bağlantısı W’yi aşıyor";
      case "comb_r3":
        return "Kombinasyon (comb_r3): Y bölümü gelişimi zayıf";
      case "comb_r4":
        return "Kombinasyon (comb_r4): W–Y kontrastı yetersiz";
      case "comb_r5":
        return "Kombinasyon (comb_r5): WXYXZ oranları ayrı motor";
      case "comb_r6":
        return "Kombinasyon (comb_r6): WXYXZ post-B / bilgi";
      case "comb_confirmed":
        return "W–X–Y: teyit (comb_r1–comb_r4)";
      case "wxyxz_alt":
        return "W-X-Y-X-Z: alternasyon bozuk";
      case "wxyxz_progress":
        return "W-X-Y-X-Z: yönsel ilerleme yok";
      case "wxyxz_x_connectors":
        return "W-X-Y-X-Z: X bağlantıları aşırı geniş";
      case "wxyxz_ratio_wy":
        return "W-X-Y-X-Z: W≈Y oran bandı dışında";
      case "wxyxz_ratio_zy":
        return "W-X-Y-X-Z: Z uzatma bandı dışında";
      case "wxyxz_x_retrace_band":
        return "W-X-Y-X-Z: X retrace bandı dışında";
      case "wxyxz_post_b_context":
        return "W-X-Y-X-Z: post-B bağlamı sağlanmadı";
      case "wxyxz_confirmed":
        return "W-X-Y-X-Z: confirmed/candidate sınıflaması";
      default:
        return id;
    }
  }, []);
  const patternState = useCallback((c?: { pattern: string; checks: { id: string; passed: boolean }[] } | null) => {
    if (!c) return "-";
    if (c.pattern !== "combination") return c.pattern;
    const wxy = c.checks.find((x) => x.id === "comb_confirmed");
    const xz = c.checks.find((x) => x.id === "wxyxz_confirmed");
    return wxy?.passed || xz?.passed ? "combination✓" : "combination?";
  }, []);
  const v2Rows = useMemo(
    () =>
      [
        { tf: "4h", s: v2Output?.hierarchy.macro },
        { tf: "1h", s: v2Output?.hierarchy.intermediate },
        { tf: "15m", s: v2Output?.hierarchy.micro },
      ],
    [v2Output],
  );

  /** Motor `enabled` kapalı olsa da ZigZag derinliği düzenlenebilsin (grafik motoru bağımsız). */
  const zigzagDepthBlock = (
    <div style={{ marginTop: "0.45rem" }}>
      <p className="muted" style={{ fontSize: "0.72rem", lineHeight: 1.45, margin: "0 0 0.35rem" }}>
        ACP <code>zigzag[]</code> kanal/tarama içindir; Elliott ZigZag TF başına derinlik (Ana Elliott sekmesindeki tablo
        ile aynı alanlar).
      </p>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: "0.35rem" }}>
        <label className="muted tv-elliott-panel__field" style={{ margin: 0 }}>
          <span style={{ fontSize: "0.7rem" }}>ZigZag depth 4H</span>
          <input
            type="number"
            min={2}
            max={100}
            title="Fraktal penceresi — 4H MTF"
            value={value.elliott_zigzag_depth_4h}
            onChange={(e) => {
              const n = parseInt(e.target.value, 10);
              const z = Math.min(100, Math.max(2, Number.isFinite(n) ? n : 21));
              onChange({ ...value, elliott_zigzag_depth_4h: z, elliott_zigzag_depth: z, swing_depth: z });
            }}
            className="mono"
          />
        </label>
        <label className="muted tv-elliott-panel__field" style={{ margin: 0 }}>
          <span style={{ fontSize: "0.7rem" }}>ZigZag depth 1H</span>
          <input
            type="number"
            min={2}
            max={100}
            title="Fraktal penceresi — 1H MTF"
            value={value.elliott_zigzag_depth_1h}
            onChange={(e) => {
              const n = parseInt(e.target.value, 10);
              const z = Math.min(100, Math.max(2, Number.isFinite(n) ? n : 21));
              onChange({ ...value, elliott_zigzag_depth_1h: z });
            }}
            className="mono"
          />
        </label>
        <label className="muted tv-elliott-panel__field" style={{ margin: 0 }}>
          <span style={{ fontSize: "0.7rem" }}>ZigZag depth 15M</span>
          <input
            type="number"
            min={2}
            max={100}
            title="Fraktal penceresi — 15M MTF"
            value={value.elliott_zigzag_depth_15m}
            onChange={(e) => {
              const n = parseInt(e.target.value, 10);
              const z = Math.min(100, Math.max(2, Number.isFinite(n) ? n : 21));
              onChange({ ...value, elliott_zigzag_depth_15m: z });
            }}
            className="mono"
          />
        </label>
      </div>
      <p className="muted" style={{ fontSize: "0.72rem", margin: "0.25rem 0 0" }}>
        Bu grafik için etkin derinlik: <span className="mono">{effectiveSwingDepth}</span>
      </p>
    </div>
  );

  const drawingsBlock = (
    <>
      <div className="tv-elliott-panel__row">
        <label className="tv-elliott-panel__toggle">
          <input
            type="checkbox"
            checked={value.enabled}
            onChange={(e) => onChange({ ...value, enabled: e.target.checked })}
          />
          <span>Elliott analizi (grafik)</span>
        </label>
        <span className="muted" style={{ fontSize: "0.75rem" }}>
          {value.enabled ? `${pivotCount} swing pivot` : ""}
        </span>
      </div>
      <p className="muted" style={{ fontSize: "0.78rem", marginTop: "0.45rem", marginBottom: "0.25rem" }}>
        {layout === "impulse"
          ? "İtki dalgaları (1–5)"
          : layout === "corrective"
            ? "Düzeltme dalgaları (2 ve 4)"
            : "İtki ve düzeltme dalgaları"}
      </p>
      <div className="tv-elliott-panel__params" style={{ display: "grid", gap: "0.35rem" }}>
        {showImpulseUi ? (
          <>
            <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check">
              <input
                type="checkbox"
                checked={value.show_projection_4h}
                disabled={!value.enabled || !value.pattern_menu_by_tf["4h"].motive_impulse}
                onChange={(e) => onChange({ ...value, show_projection_4h: e.target.checked })}
              />
              <span>İleri Fib projeksiyon — 4h (makro itkiden; şema, tahmin değildir)</span>
            </label>
            <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check">
              <input
                type="checkbox"
                checked={value.show_projection_1h}
                disabled={!value.enabled || !value.pattern_menu_by_tf["1h"].motive_impulse}
                onChange={(e) => onChange({ ...value, show_projection_1h: e.target.checked })}
              />
              <span>İleri Fib projeksiyon — 1h (ara itkiden)</span>
            </label>
            <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check">
              <input
                type="checkbox"
                checked={value.show_projection_15m}
                disabled={!value.enabled || !value.pattern_menu_by_tf["15m"].motive_impulse}
                onChange={(e) => onChange({ ...value, show_projection_15m: e.target.checked })}
              />
              <span>İleri Fib projeksiyon — 15m (mikro itkiden)</span>
            </label>
            <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check">
              <input
                type="checkbox"
                checked={value.show_historical_waves}
                disabled={!value.enabled}
                onChange={(e) => onChange({ ...value, show_historical_waves: e.target.checked })}
              />
              <span>Geçmişte Elliott itki ara (tarihsel 1–5 katmanı)</span>
            </label>
          </>
        ) : null}
        {value.enabled ? (
          <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check">
            <input
              type="checkbox"
              checked={value.show_nested_formations}
              onChange={(e) => onChange({ ...value, show_nested_formations: e.target.checked })}
            />
            <span>Ana formasyon içindeki alt formasyonları göster (alt itkı; 2/4 içi a–b–c)</span>
          </label>
        ) : null}
      </div>
      {layout !== "full" ? zigzagDepthBlock : null}
    </>
  );

  const motorParamsBlock =
    value.enabled ? (
      <>
          {v2Output ? (
            <>
              <p className="muted" style={{ fontSize: "0.74rem", marginTop: "0.25rem", lineHeight: 1.35 }}>
                V2 durum: {v2StateNote || "4h:invalid · 1h:invalid · 15m:invalid"}
              </p>
              {v2FailedChecks.length ? (
                <p className="err" style={{ fontSize: "0.74rem", marginTop: "0.2rem", lineHeight: 1.35 }}>
                  V2 kural ihlali: {v2FailedChecks.map((c) => v2CheckLabel(c.id)).join(" · ")}
                </p>
              ) : null}
              {v2Rows.length ? (
                <div style={{ marginTop: "0.35rem", overflowX: "auto" }}>
                  <table className="mono" style={{ width: "100%", fontSize: "0.72rem", borderCollapse: "collapse" }}>
                    <thead>
                      <tr>
                        <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>TF</th>
                        <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>Durum</th>
                        <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>Pivot</th>
                        <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>İtki (1–5)</th>
                        <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>Düzeltme (2/4)</th>
                      </tr>
                    </thead>
                    <tbody>
                      {v2Rows.map(({ tf, s }) => (
                        <tr key={tf}>
                          <td style={{ padding: "0.12rem 0.2rem" }}>{tf}</td>
                          <td style={{ padding: "0.12rem 0.2rem" }}>{s?.decision ?? (v2Output ? "invalid" : "-")}</td>
                          <td style={{ padding: "0.12rem 0.2rem" }}>{s?.pivots.length ?? 0}</td>
                          <td style={{ padding: "0.12rem 0.2rem" }}>
                            {s?.impulse ? `${s.impulse.direction} (${s.impulse.score.toFixed(1)})` : "-"}
                          </td>
                          <td style={{ padding: "0.12rem 0.2rem" }}>
                            {s?.wave2 ? `2:${patternState(s.wave2)}` : "2:-"} · {s?.wave4 ? `4:${patternState(s.wave4)}` : "4:-"}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              ) : null}
              {v2Hard.length ? (
                <p className="muted" style={{ fontSize: "0.72rem", marginTop: "0.3rem", lineHeight: 1.35 }}>
                  Hard kurallar:{" "}
                  {v2Hard.map((c) => `${c.passed ? "✓" : "✗"} ${v2CheckLabel(c.id)}`).join(" · ")}
                </p>
              ) : null}
              {v2Soft.length ? (
                <p className="muted" style={{ fontSize: "0.72rem", marginTop: "0.2rem", lineHeight: 1.35 }}>
                  Soft kontroller:{" "}
                  {v2Soft.map((c) => `${c.passed ? "✓" : "○"} ${v2CheckLabel(c.id)}`).join(" · ")}
                </p>
              ) : null}
            </>
          ) : null}
          <p className="muted" style={{ fontSize: "0.78rem", marginTop: "0.5rem", marginBottom: "0.25rem" }}>
            {layout === "impulse"
              ? "Parametreler — itki ve ZigZag"
              : layout === "corrective"
                ? "Parametreler — düzeltme ve ZigZag"
                : "Parametreler"}
          </p>
          <div
            className="tv-elliott-panel__params"
            style={{ marginTop: "0.15rem", display: "grid", gap: "0.45rem" }}
          >
            <p className="muted" style={{ fontSize: "0.72rem", margin: 0, lineHeight: 1.4 }}>
              MTF çizgi renkleri (ZigZag, itki, düzeltme)
            </p>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: "0.45rem" }}>
              <label className="muted tv-elliott-panel__field" style={{ margin: 0 }}>
                <span style={{ fontSize: "0.7rem" }}>4h</span>
                <input
                  type="color"
                  aria-label="4h dalga rengi"
                  value={elliottHexForColorInput(value.mtf_wave_color_4h)}
                  onChange={(e) => onChange({ ...value, mtf_wave_color_4h: e.target.value })}
                  style={{ width: "100%", height: "1.85rem", padding: 0, border: "none", cursor: "pointer" }}
                />
              </label>
              <label className="muted tv-elliott-panel__field" style={{ margin: 0 }}>
                <span style={{ fontSize: "0.7rem" }}>1h</span>
                <input
                  type="color"
                  aria-label="1h dalga rengi"
                  value={elliottHexForColorInput(value.mtf_wave_color_1h)}
                  onChange={(e) => onChange({ ...value, mtf_wave_color_1h: e.target.value })}
                  style={{ width: "100%", height: "1.85rem", padding: 0, border: "none", cursor: "pointer" }}
                />
              </label>
              <label className="muted tv-elliott-panel__field" style={{ margin: 0 }}>
                <span style={{ fontSize: "0.7rem" }}>15m</span>
                <input
                  type="color"
                  aria-label="15m dalga rengi"
                  value={elliottHexForColorInput(value.mtf_wave_color_15m)}
                  onChange={(e) => onChange({ ...value, mtf_wave_color_15m: e.target.value })}
                  style={{ width: "100%", height: "1.85rem", padding: 0, border: "none", cursor: "pointer" }}
                />
              </label>
            </div>
            {showImpulseUi &&
            (value.show_projection_4h || value.show_projection_1h || value.show_projection_15m) ? (
              <>
                <label className="muted tv-elliott-panel__field">
                  <span>Projeksiyon: adım (kaç mum süresi)</span>
                  <input
                    type="number"
                    min={1}
                    max={100}
                    title="Her segment arası ortalama mum süresinin katı (Pine 22)"
                    value={value.projection_bar_hop}
                    onChange={(e) =>
                      onChange({
                        ...value,
                        projection_bar_hop: Math.min(
                          100,
                          Math.max(1, parseInt(e.target.value, 10) || 22),
                        ),
                      })
                    }
                    className="mono"
                  />
                </label>
                <label className="muted tv-elliott-panel__field">
                  <span>Projeksiyon: segment sayısı</span>
                  <input
                    type="number"
                    min={1}
                    max={24}
                    title="İleri çizgide kaç adım (Pine 12)"
                    value={value.projection_steps}
                    onChange={(e) =>
                      onChange({
                        ...value,
                        projection_steps: Math.min(
                          24,
                          Math.max(1, parseInt(e.target.value, 10) || 12),
                        ),
                      })
                    }
                    className="mono"
                  />
                </label>
              </>
            ) : null}
            <label className="muted tv-elliott-panel__field">
              <span>Geri arama (pivot penceresi)</span>
              <input
                type="number"
                min={5}
                max={400}
                title="Sondan kaç olası 6’lı pivot penceresi denenir"
                value={value.max_pivot_windows}
                onChange={(e) =>
                  onChange({
                    ...value,
                    max_pivot_windows: Math.min(400, Math.max(5, parseInt(e.target.value, 10) || 120)),
                  })
                }
                className="mono"
              />
            </label>
            {showCorrectiveUi ? (
              <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check">
                <input
                  type="checkbox"
                  checked={value.strict_wave4_overlap}
                  onChange={(e) => onChange({ ...value, strict_wave4_overlap: e.target.checked })}
                />
                <span>Düzeltme: dalga 4, dalga 1 alanına girmesin (klasik örtüşme kuralı, sıkı)</span>
              </label>
            ) : null}
          </div>
        </>
      ) : null;

  return (
    <div
      className="tv-elliott-panel tv-elliott-panel--drawer"
      aria-label={
        layout === "impulse"
          ? "Elliott — itki dalgaları"
          : layout === "corrective"
            ? "Elliott — düzeltme dalgaları"
            : "Elliott Wave ayarları"
      }
    >
      {!splitMenuLayout ? (
        <details className="tv-collapsible" style={{ marginBottom: "0.35rem" }}>
          <summary className="muted" style={{ fontSize: "0.75rem" }}>Kaynaklar</summary>
          <p className="muted" style={{ fontSize: "0.74rem", marginTop: "0.35rem", marginBottom: 0 }}>
            Tam metin:{" "}
            <a href={ELLIOTT_REFERENCE_TXT_URL} target="_blank" rel="noopener noreferrer">
              elliott_dalga_prensipleri.txt
            </a>
            {" · "}
            <a href={ELLIOTT_REFERENCE_RANKIA_URL} target="_blank" rel="noopener noreferrer">
              Rankia: Elliott dalga teorisi (Turkce ozet)
            </a>
          </p>
        </details>
      ) : (
        <p className="muted" style={{ fontSize: "0.74rem", marginBottom: "0.4rem", lineHeight: 1.4 }}>
          {layout === "impulse"
            ? "İtki dalgaları (1–5) çizimi ve projeksiyon. Bölümleri başlıktan açıp kapatabilirsiniz."
            : "Düzeltme dalgaları (2 ve 4) ABC ve kurallar. Bölümleri başlıktan açıp kapatabilirsiniz."}
        </p>
      )}

      {splitMenuLayout ? (
        <details className="tv-collapsible" open>
          <summary className="muted" style={{ fontSize: "0.8rem", fontWeight: 600 }}>
            {layout === "impulse" ? "İtki — grafik çizimleri" : "Düzeltme — grafik çizimleri"}
          </summary>
          <div style={{ marginTop: "0.35rem" }}>{drawingsBlock}</div>
        </details>
      ) : (
        drawingsBlock
      )}

      {splitMenuLayout ? (
        motorParamsBlock ? (
          <details className="tv-collapsible" open style={{ marginTop: "0.35rem" }}>
            <summary className="muted" style={{ fontSize: "0.8rem", fontWeight: 600 }}>
              {layout === "impulse" ? "İtki — motor ve parametreler" : "Düzeltme — motor ve parametreler"}
            </summary>
            <div style={{ marginTop: "0.35rem" }}>{motorParamsBlock}</div>
          </details>
        ) : null
      ) : (
        motorParamsBlock
      )}

      <div style={{ display: "flex", flexWrap: "wrap", gap: "0.5rem", marginTop: "0.55rem", alignItems: "center" }}>
        <button
          type="button"
          className="theme-toggle"
          disabled={refreshBusy}
          onClick={() => onRefreshFromServer()}
        >
          {refreshBusy ? "Yükleniyor…" : "Sunucudan yenile"}
        </button>
        <button
          type="button"
          className="theme-toggle"
          disabled={saveBusy || !allowPersistConfig}
          onClick={() => onSaveToDb()}
          title={
            allowPersistConfig
              ? "POST /api/v1/config — admin rolü gerekir"
              : "Yalnızca admin rolü app_config yazabilir"
          }
        >
          {saveBusy ? "Kaydediliyor…" : "Veritabanına kaydet"}
        </button>
      </div>
      {loadErr ? <p className="err" style={{ marginTop: "0.35rem", fontSize: "0.8rem" }}>{loadErr}</p> : null}
      {saveErr ? <p className="err" style={{ marginTop: "0.35rem", fontSize: "0.8rem" }}>{saveErr}</p> : null}
      {!loadErr ? (
        <p className="muted" style={{ marginTop: "0.35rem", fontSize: "0.75rem" }}>
          Sunucu: <code>GET /api/v1/analysis/elliott-wave-config</code> — <code>app_config.elliott_wave</code> ile
          eşleşen panel durumu yüklendi.
        </p>
      ) : null}

      {!splitMenuLayout ? (
        <>
          <button
            type="button"
            className="tv-elliott-panel__rules-btn"
            onClick={() => setJsonOpen((o) => !o)}
            aria-expanded={jsonOpen}
            style={{ marginTop: "0.35rem" }}
          >
            {jsonOpen ? "JSON önizlemesini gizle" : "app_config JSON önizleme (salt okunur)"}
          </button>
          {jsonOpen ? (
            <pre
              className="mono"
              style={{
                marginTop: "0.35rem",
                maxHeight: "10rem",
                overflow: "auto",
                fontSize: "0.72rem",
                padding: "0.5rem",
                borderRadius: "6px",
                background: "var(--tv-panel-bg, rgba(0,0,0,0.2))",
              }}
            >
              {JSON.stringify(value, null, 2)}
            </pre>
          ) : null}

          <button
            type="button"
            className="tv-elliott-panel__rules-btn"
            onClick={() => setRulesOpen((o) => !o)}
            aria-expanded={rulesOpen}
          >
            {rulesOpen ? "Kuralları gizle" : "Teknik analiz & Elliott kuralları (özet)"}
          </button>
          {rulesOpen ? (
        <div className="tv-elliott-panel__rules">
          <p className="muted" style={{ fontSize: "0.72rem", lineHeight: 1.45 }}>
            <strong>Kapsam:</strong> Grafikteki sayım <code>elliottEngineV2</code> (ZigZag + itkı/düzeltme) ile üretilir.
            Aşağıdaki etiketler tez metninin motorda ne kadarının doğrudan kontrol edildiğini gösterir; §2.5.3.4–2.5.5 ve rehber
            satırlarının tamamı otomatik doğrulanmaz.
          </p>
          <p className="muted" style={{ fontSize: "0.72rem" }}>
            Dereceler (örnek): {ELLIOTT_DEGREE_LEVELS.join(" → ")}
          </p>
          <ul>
            {ELLIOTT_TECHNICAL_ANALYSIS_SUMMARY.map((t, i) => (
              <li key={i}>{t}</li>
            ))}
          </ul>

          <p className="muted" style={{ fontWeight: 600, marginTop: "0.35rem" }}>
            2.5.3.1 İtki itici dalga
          </p>
          {ELLIOTT_TEZ_2531_INTRO_PARAGRAPHS.map((p, i) => (
            <p key={i} className="muted" style={{ fontSize: "0.72rem", marginTop: i === 0 ? "0.25rem" : "0.35rem", lineHeight: 1.45 }}>
              {p}
            </p>
          ))}

          <p className="muted" style={{ fontWeight: 600, marginTop: "0.35rem" }}>Kurallar (zorunlu)</p>
          <dl>
            {ELLIOTT_IMPULSE_RULES.map((r) => (
              <div key={r.id}>
                <dt>
                  {r.title}{" "}
                  <span
                    className="muted"
                    style={{ fontSize: "0.72rem", fontWeight: 600, marginLeft: "0.25rem" }}
                    title="elliottEngineV2 impulse kontrolleri ile örtüşme"
                  >
                    [{elliottImpulseRuleScopeLabelTr(ELLIOTT_IMPULSE_RULE_ENGINE_SCOPE[r.id])}]
                  </span>
                </dt>
                <dd className="muted">{r.detail}</dd>
              </div>
            ))}
          </dl>

          <p className="muted" style={{ fontWeight: 600, marginTop: "0.35rem" }}>Rehber ilkeler</p>
          <p className="muted" style={{ fontSize: "0.72rem", marginTop: "0.2rem", lineHeight: 1.45 }}>
            [Rehber] — Elliott hedefi/yorum; <code>elliottEngineV2</code> bu maddeleri skorlamaz.
          </p>
          <dl>
            {ELLIOTT_TEZ_IMPULSE_GUIDELINES.map((r) => (
              <div key={r.id}>
                <dt>
                  {r.title}{" "}
                  <span className="muted" style={{ fontSize: "0.72rem", fontWeight: 600, marginLeft: "0.25rem" }}>
                    [Rehber]
                  </span>
                </dt>
                <dd className="muted">{r.detail}</dd>
              </div>
            ))}
          </dl>

          <div key={ELLIOTT_SUBDIVISION_RULE.id}>
            <p className="muted" style={{ fontWeight: 600, marginTop: "0.35rem" }}>{ELLIOTT_SUBDIVISION_RULE.title}</p>
            <p className="muted" style={{ fontSize: "0.72rem", lineHeight: 1.45 }}>{ELLIOTT_SUBDIVISION_RULE.detail}</p>
          </div>

          <p className="muted" style={{ fontWeight: 600, marginTop: "0.35rem" }}>{ELLIOTT_TEZ_2532_EXTENSION.title}</p>
          {ELLIOTT_TEZ_2532_EXTENSION.paragraphs.map((p, i) => (
            <p key={i} className="muted" style={{ fontSize: "0.72rem", marginTop: "0.35rem", lineHeight: 1.45 }}>
              {p}
            </p>
          ))}

          <p className="muted" style={{ fontWeight: 600, marginTop: "0.35rem" }}>{ELLIOTT_TEZ_2533_FAILED_FIFTH.title}</p>
          {ELLIOTT_TEZ_2533_FAILED_FIFTH.paragraphs.map((p, i) => (
            <p key={i} className="muted" style={{ fontSize: "0.72rem", marginTop: "0.35rem", lineHeight: 1.45 }}>
              {p}
            </p>
          ))}

          <p className="muted" style={{ fontSize: "0.72rem", marginTop: "0.45rem", lineHeight: 1.45 }}>
            Aşağıdaki bölümler (2.5.3.4 – 2.5.5) tez kaynağıdır. Zigzag/yassı ABC için kısmi sayısal kontroller{" "}
            <code>elliottEngineV2/tezWaveChecks.ts</code> içindedir; diyagonal üçgen, ikili zigzag alt bölünmesi vb. genelde
            manuel okuma kalır.
          </p>
          {ELLIOTT_TEZ_EXTENDED_SECTIONS.map((s) => (
            <TezExtendedBlock key={s.id} section={s} />
          ))}
        </div>
      ) : null}
        </>
      ) : null}
    </div>
  );
}
