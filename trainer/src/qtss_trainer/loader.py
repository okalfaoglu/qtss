"""Pull the training surface from Postgres into a pandas DataFrame."""
from __future__ import annotations

import pandas as pd
import psycopg


# Columns we carry through untouched; anything else lives under
# `features_by_source` and gets flattened by `features.flatten_jsonb`.
_CARRIED_COLUMNS = (
    "setup_id",
    "detection_id",
    "venue_class",
    "exchange",
    "symbol",
    "timeframe",
    "profile",
    "direction",
    "state",
    "created_at",
    "closed_at",
    "confluence_id",
    "risk_mode",
    "mode",
    "label",
    "close_reason",
    "category",
    "realized_rr",
    "outcome_pnl_pct",
    "max_favorable_r",
    "max_adverse_r",
    "time_to_outcome_bars",
    "outcome_bars_to_first_tp",
    "features_by_source",
    "feature_sources",
)


def load_closed(conn: psycopg.Connection) -> pd.DataFrame:
    """One row per labeled+closed setup, features as a JSONB map."""
    query = f"""
        SELECT {', '.join(_CARRIED_COLUMNS)}
        FROM v_qtss_training_set_closed
        ORDER BY created_at ASC
    """
    with conn.cursor() as cur:
        cur.execute(query)
        rows = cur.fetchall()
        cols = [d.name for d in cur.description]
    return pd.DataFrame(rows, columns=cols)


def label_counts(conn: psycopg.Connection) -> dict[str, int]:
    with conn.cursor() as cur:
        cur.execute(
            """
            SELECT COALESCE(label, 'unlabeled'), COUNT(*)::bigint
            FROM v_qtss_training_set
            GROUP BY 1
            """
        )
        return {row[0]: int(row[1]) for row in cur.fetchall()}
