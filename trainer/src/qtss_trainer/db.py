"""Database connection + config resolver.

Mirrors the Rust `system_config > config_schema` two-tier resolver so
Python reads the same knob the Rust worker would see.
"""
from __future__ import annotations

import json
import os
from dataclasses import dataclass
from typing import Any

import psycopg


def connect() -> psycopg.Connection:
    """Open a psycopg connection using QTSS_DATABASE_URL or DATABASE_URL."""
    url = os.getenv("QTSS_DATABASE_URL") or os.getenv("DATABASE_URL")
    if not url:
        raise RuntimeError(
            "Set QTSS_DATABASE_URL (e.g. "
            "postgres://qtss:***@127.0.0.1/qtss) before running the trainer."
        )
    return psycopg.connect(url, autocommit=False)


_SENTINEL = object()


def resolve_config(conn: psycopg.Connection, module: str, key: str, default: Any) -> Any:
    """system_config override, falling back to config_schema default.

    Returns the parsed JSON value; caller decides the concrete type.
    """
    with conn.cursor() as cur:
        cur.execute(
            "SELECT value FROM system_config WHERE module = %s AND config_key = %s",
            (module, key),
        )
        row = cur.fetchone()
        if row is not None:
            return _coerce(row[0])

        # config_schema stores the full key as `key` (e.g.
        # 'trainer.model_family'); `module` is not a column there.
        cur.execute(
            "SELECT default_value FROM config_schema WHERE key = %s",
            (key,),
        )
        row = cur.fetchone()
        if row is not None:
            return _coerce(row[0])

    return default


def _coerce(raw: Any) -> Any:
    """psycopg returns jsonb as python objects already; tolerate str too."""
    if isinstance(raw, (dict, list, int, float, bool)) or raw is None:
        return raw
    if isinstance(raw, str):
        try:
            return json.loads(raw)
        except json.JSONDecodeError:
            return raw
    return raw


@dataclass
class TrainerConfig:
    model_family: str
    artifact_dir: str
    min_rows: int
    validation_fraction: float
    lgbm_params: dict[str, Any]
    num_boost_round: int

    @classmethod
    def load(cls, conn: psycopg.Connection) -> "TrainerConfig":
        return cls(
            model_family=str(resolve_config(conn, "ai", "trainer.model_family", "setup_meta")),
            artifact_dir=str(
                resolve_config(conn, "ai", "trainer.artifact_dir", "/app/qtss/artifacts/models")
            ),
            min_rows=int(resolve_config(conn, "ai", "trainer.min_rows", 500)),
            validation_fraction=float(
                resolve_config(conn, "ai", "trainer.validation_fraction", 0.2)
            ),
            lgbm_params=dict(
                resolve_config(
                    conn,
                    "ai",
                    "trainer.lgbm_params",
                    {
                        "objective": "binary",
                        "metric": ["auc", "binary_logloss"],
                        "learning_rate": 0.05,
                        "num_leaves": 31,
                        "min_data_in_leaf": 20,
                        "feature_fraction": 0.9,
                        "bagging_fraction": 0.8,
                        "bagging_freq": 5,
                        "verbose": -1,
                    },
                )
            ),
            num_boost_round=int(resolve_config(conn, "ai", "trainer.num_boost_round", 500)),
        )
