-- RADAR open exposure column — Faz 20 fix.
--
-- The original `cash_position_pct` was stuck at 100% because the
-- aggregator multiplied total_notional (which tracks CLOSED trades) by
-- 0.0 in the cash formula. The real quantity that should be subtracted
-- from capital is the USD exposure of still-open live_positions. We
-- materialise that into a new `open_exposure_usd` column so the GUI
-- can display "Toplam Yatırım (açık)" alongside closed-trade metrics.

ALTER TABLE radar_reports
    ADD COLUMN IF NOT EXISTS open_exposure_usd DOUBLE PRECISION NOT NULL DEFAULT 0;

COMMENT ON COLUMN radar_reports.open_exposure_usd IS
    'USD notional of still-open live_positions (entry_avg × qty_remaining) at period_end for this mode. Used to compute cash_position_pct = (capital - open_exposure) / capital.';
