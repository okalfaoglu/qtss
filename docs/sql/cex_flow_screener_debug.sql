-- Inspect `nansen_netflows` snapshot shape (`data_snapshots` column is `response_json`).
-- Table: data_snapshots

SELECT source_key, computed_at
FROM data_snapshots
WHERE source_key = 'nansen_netflows';

-- Top-level JSON keys
SELECT jsonb_object_keys(response_json) AS key
FROM data_snapshots
WHERE source_key = 'nansen_netflows';

-- `data` type: object vs array
SELECT jsonb_typeof(response_json->'data') AS data_type
FROM data_snapshots
WHERE source_key = 'nansen_netflows';

-- If `data` is object: child keys (e.g. tokens)
SELECT jsonb_object_keys(response_json->'data') AS data_key
FROM data_snapshots
WHERE source_key = 'nansen_netflows'
  AND jsonb_typeof(response_json->'data') = 'object';

-- If `data` is array: keys of first row (Nansen often: token_symbol, net_flow_24h_usd, …)
SELECT jsonb_object_keys(response_json->'data'->0) AS row_key
FROM data_snapshots
WHERE source_key = 'nansen_netflows'
  AND jsonb_typeof(response_json->'data') = 'array'
  AND jsonb_array_length(response_json->'data') > 0;

-- Screener output + notes
SELECT source_key, computed_at,
       jsonb_array_length(COALESCE(response_json->'top', '[]'::jsonb)) AS top_len,
       response_json->'parsing_notes' AS notes
FROM data_snapshots
WHERE source_key IN ('cex_flow_accumulation_top25', 'cex_flow_distribution_top25');
