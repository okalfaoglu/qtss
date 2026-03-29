-- RBAC / güvenlik bağlamı (ör. user_permissions önce/sonra). HTTP audit ile aynı tablo.

ALTER TABLE audit_log ADD COLUMN IF NOT EXISTS details JSONB;

COMMENT ON COLUMN audit_log.details IS 'Yapılandırılmış denetim: user_permissions_replace vb.';
