-- 0157_faz10_classical_stage1.sql
--
-- Faz 10 Aşama 1 — Triple top/bottom, Broadening top/bottom/triangle,
-- V-top/V-bottom, Measured Move ABCD detector config seeds.
-- CLAUDE.md #2: tüm eşikler system_config üzerinden, hard-code yok.
-- Idempotent: ON CONFLICT DO NOTHING ile çift çalıştırmaya güvenli.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('detection', 'classical.triple_peak_tol', '0.03'::jsonb,
   'Triple top/bottom — 3 tepe/dip arası max göreli sapma.'),
  ('detection', 'classical.triple_min_span_bars', '10'::jsonb,
   'Triple top/bottom — pattern ilk→son pivot min bar sayısı.'),
  ('detection', 'classical.triple_neckline_slope_max', '0.003'::jsonb,
   'Triple top/bottom — neckline max |slope|/mid per bar.'),
  ('detection', 'classical.broadening_min_slope_pct', '0.002'::jsonb,
   'Broadening — diverging iki kenar min |slope| (fraction/bar).'),
  ('detection', 'classical.broadening_flat_slope_pct', '0.0015'::jsonb,
   'Broadening triangle — "flat" kenar için üst sınır |slope|.'),
  ('detection', 'classical.v_max_total_bars', '20'::jsonb,
   'V-top/V-bottom — toplam pattern max bar sayısı.'),
  ('detection', 'classical.v_min_amplitude_pct', '0.03'::jsonb,
   'V-top/V-bottom — her kenarın min göreli genliği.'),
  ('detection', 'classical.v_symmetry_tol', '0.4'::jsonb,
   'V-top/V-bottom — iki kenar genlik simetri toleransı.'),
  ('detection', 'classical.abcd_c_min_retrace', '0.382'::jsonb,
   'ABCD — B→C min retracement (AB oranı).'),
  ('detection', 'classical.abcd_c_max_retrace', '0.886'::jsonb,
   'ABCD — B→C max retracement (AB oranı).'),
  ('detection', 'classical.abcd_d_projection_tol', '0.15'::jsonb,
   'ABCD — CD/AB projection 1.0 etrafında ±tol.'),
  ('detection', 'classical.abcd_min_bars_per_leg', '3'::jsonb,
   'ABCD — her bacak için minimum bar sayısı.')
ON CONFLICT (module, config_key) DO NOTHING;

-- Feature store source enable flags (default on); yeni subkind'lar aynı
-- "classical" source altında toplandığı için ayrı satır gerekmiyor.
