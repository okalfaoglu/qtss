-- P22 — TBM staleness: auto-invalidate forming bottom_setup/top_setup
-- detections whose anchor bar aged past N bars on the same TF, or
-- whose invalidation_price has been breached. Before this patch a
-- bottom_setup could linger indefinitely in state='forming' because
-- no downstream process closed it; the chart kept drawing the label
-- at a price zone the market had long since left (user report: 35%
-- Weak bottom_setup floating mid-chart after a full rally).

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('tbm', 'setup.max_anchor_age_bars', '"12"',
   'P22 — forming TBM detection auto-invalidated after this many bars since anchor. Bar count applies on the same TF: 12h on H1, ~2 days on 4h, 12 days on 1d.')
ON CONFLICT (module, config_key) DO NOTHING;

-- One-shot cleanup for existing zombies: anything older than 14 days
-- in state=forming from the tbm family is safely past any reasonable
-- TF window and should be closed so the chart stops rendering it.
UPDATE qtss_v2_detections
   SET state = 'invalidated',
       updated_at = NOW()
 WHERE family = 'tbm'
   AND state = 'forming'
   AND detected_at < NOW() - INTERVAL '14 days';
