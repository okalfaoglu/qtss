-- Remove all LLM failure rows (same effect as AI → Performans → «Hata satırlarını sil», admin).
-- Child rows (tactical / directives / outcomes) CASCADE from ai_decisions.
DELETE FROM ai_decisions
WHERE status = 'error';
