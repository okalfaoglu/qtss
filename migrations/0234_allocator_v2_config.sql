-- Allocator v2 config (PR-FAZ13E).
-- Bridges the Faz 11-13 detection stack (confluence_snapshots +
-- detections + pattern_outcomes) to the existing dry-trade pipeline
-- (qtss_setups → selected_candidates → live_positions via
-- execution_bridge). Disabled by default — operators opt in.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('allocator_v2', 'enabled',            '{"enabled": false}'::jsonb,
     'Otonom dry-mode setup oluşturucu. true yapınca her tick''te confluence_snapshots taranır, AI multi-gate''den geçen sinyaller qtss_setups (mode=''dry'') olarak armlanır; execution_bridge otomatik dry order açar; Telegram bildirimi notify_outbox''a düşer.'),

    ('allocator_v2', 'tick_secs',          '{"secs": 60}'::jsonb,
     'Allocator tick periyodu (saniye). 60 = her dakika.'),

    ('allocator_v2', 'lookback_minutes',   '{"value": 10}'::jsonb,
     'Taranacak confluence_snapshots penceresi (dakika). Son 10 dk''nın güçlü verdict''leri alınır.'),

    ('allocator_v2', 'min_abs_net_score',  '{"value": 1.5}'::jsonb,
     'Confluence |net_score| eşiği. Altında setup açılmaz. 1.5 confluence.strong_threshold ile aynı — ama allocator kendi eşiğini farklı tutabilir (daha konservatif).'),

    ('allocator_v2', 'risk_pct_per_trade', '{"value": 0.01}'::jsonb,
     'İşlem başına risk yüzdesi (equity oranı). Dry modda informasyonel, pozisyon boyutu hesabı için kullanılır.'),

    -- Bağımlı: dry execution defaults tarafında zaten seed var —
    -- yoksa allocator fallback olarak `organizations` tablosundan ilk
    -- satırı alır.
    ('dry', 'default_org_id', '{"value": null}'::jsonb,
     'Dry mod pozisyonları için varsayılan org_id. Null bırakılırsa allocator organizations tablosundan ilk satırı kullanır.')
ON CONFLICT (module, config_key) DO NOTHING;
