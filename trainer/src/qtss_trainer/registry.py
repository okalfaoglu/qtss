"""Persist a trained model: write artifact + insert qtss_models row."""
from __future__ import annotations

import datetime as dt
import hashlib
import json
import os
from pathlib import Path

import psycopg

from .model import TrainResult


def _sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def make_version() -> str:
    """Calendar-based version; yyyy.mm.dd-NN, monotonic within a day."""
    today = dt.date.today().strftime("%Y.%m.%d")
    return f"{today}-{dt.datetime.utcnow().strftime('%H%M%S')}"


def save(
    conn: psycopg.Connection,
    result: TrainResult,
    *,
    model_family: str,
    artifact_dir: str,
    feature_spec_version: int,
    trained_by: str | None = None,
    notes: str | None = None,
    activate: bool = False,
) -> tuple[str, Path]:
    """Write booster to disk, register in `qtss_models`, return (version, path)."""
    out_dir = Path(artifact_dir) / model_family
    out_dir.mkdir(parents=True, exist_ok=True)

    version = make_version()
    artifact_path = out_dir / f"{version}.txt"
    result.booster.save_model(str(artifact_path))
    sha = _sha256(artifact_path)

    # Also dump feature names alongside the artifact for cross-language
    # (Rust) consumers that can't introspect the LightGBM text format.
    meta_path = out_dir / f"{version}.meta.json"
    meta_path.write_text(
        json.dumps(
            {
                "model_family": model_family,
                "model_version": version,
                "feature_spec_version": feature_spec_version,
                "feature_names": result.feature_names,
                "params": result.params,
                "metrics": result.metrics,
                "n_train": result.n_train,
                "n_valid": result.n_valid,
                "num_boost_round": result.num_boost_round,
                "trained_at": dt.datetime.utcnow().isoformat() + "Z",
                "sha256": sha,
            },
            indent=2,
        )
    )

    with conn.cursor() as cur:
        if activate:
            cur.execute(
                "UPDATE qtss_models SET active = false WHERE model_family = %s",
                (model_family,),
            )
        cur.execute(
            """
            INSERT INTO qtss_models
                (model_family, model_version, feature_spec_version,
                 algorithm, task, n_train, n_valid,
                 metrics_json, params_json, feature_names,
                 artifact_path, artifact_sha256,
                 trained_by, notes, active)
            VALUES
                (%s, %s, %s, 'lightgbm', %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
            """,
            (
                model_family,
                version,
                feature_spec_version,
                result.params.get("objective", "binary"),
                result.n_train,
                result.n_valid,
                json.dumps(result.metrics),
                json.dumps(result.params),
                result.feature_names,
                str(artifact_path),
                sha,
                trained_by or os.getenv("USER", "unknown"),
                notes,
                activate,
            ),
        )
    conn.commit()
    return version, artifact_path


def activate(conn: psycopg.Connection, model_family: str, version: str) -> None:
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE qtss_models SET active = false WHERE model_family = %s",
            (model_family,),
        )
        cur.execute(
            "UPDATE qtss_models SET active = true WHERE model_family = %s AND model_version = %s",
            (model_family, version),
        )
        if cur.rowcount != 1:
            conn.rollback()
            raise LookupError(f"model {model_family}/{version} not found")
    conn.commit()


def list_models(conn: psycopg.Connection, model_family: str | None = None) -> list[dict]:
    with conn.cursor() as cur:
        if model_family:
            cur.execute(
                """
                SELECT model_family, model_version, feature_spec_version, n_train, n_valid,
                       metrics_json, active, trained_at
                FROM qtss_models
                WHERE model_family = %s
                ORDER BY trained_at DESC
                """,
                (model_family,),
            )
        else:
            cur.execute(
                """
                SELECT model_family, model_version, feature_spec_version, n_train, n_valid,
                       metrics_json, active, trained_at
                FROM qtss_models
                ORDER BY trained_at DESC
                """
            )
        cols = [d.name for d in cur.description]
        return [dict(zip(cols, row)) for row in cur.fetchall()]
