-- Kullanıcı başına ek qtss:* yetenekleri (JWT rol türevi ile birleştirilir; bkz. require_jwt).
-- 0013 öneki `0013_worker_analytics_schema.sql` için ayrılmıştır; çift 0013 kullanmayın (§6).

CREATE TABLE user_permissions (
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, permission)
);

CREATE INDEX idx_user_permissions_user ON user_permissions (user_id);

COMMENT ON TABLE user_permissions IS 'JWT permissions (rol veya claim) ile birleşir; viewer + qtss:ops gibi ek yetki için.';
