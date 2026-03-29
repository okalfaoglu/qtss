-- PLAN §1 / Phase A — Hyperliquid public `metaAndAssetCtxs` (POST). Varsayılan kapalı; açınca worker `external_fetch` tick ile `data_snapshots` yazar.

INSERT INTO external_data_sources (key, enabled, method, url, headers_json, body_json, tick_secs, description)
VALUES (
    'hl_meta_asset_ctxs',
    false,
    'POST',
    'https://api.hyperliquid.xyz/info',
    '{}'::jsonb,
    '{"type": "metaAndAssetCtxs"}'::jsonb,
    120,
    'HL perp universe funding/OI context — büyük JSON; confluence ileride bu anahtarı okuyabilir.'
)
ON CONFLICT (key) DO NOTHING;
