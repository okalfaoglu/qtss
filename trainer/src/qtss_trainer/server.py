"""FastAPI inference sidecar for Faz 9.3.3.

Loads the active LightGBM booster once at startup, exposes three endpoints:

    GET  /health        → {"ok": true, "active": {...}}
    GET  /active        → metadata of the currently-loaded model
    POST /reload        → re-read qtss_models + re-load the active booster
    POST /score         → {features_by_source: {...}} → {score: 0.73, ...}

The score endpoint accepts the same nested JSONB shape the feature store
writes to `qtss_features_snapshot`, so the Rust worker can forward what
it already has without reshaping.

Design rules (CLAUDE.md):
  * #1: tree lookup + dict dispatch instead of if/else chains
  * #2: connection string + artifact dir read from system_config, not
        hard-coded in the binary
  * #4: sidecar knows nothing about asset class; features are flat
        `source.feature` names
"""
from __future__ import annotations

import json
import logging
import threading
from pathlib import Path
from typing import Any

import lightgbm as lgb
import numpy as np
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel

from .db import connect, resolve_config

log = logging.getLogger("qtss_trainer.server")

# ---- single-entry holder keeps the booster + feature order. ---------------
# Locked on reload so in-flight /score calls don't see a half-swapped booster.
_LOCK = threading.RLock()
_STATE: dict[str, Any] = {
    "booster": None,
    "feature_names": [],
    "model_family": None,
    "model_version": None,
    "feature_spec_version": None,
    "metrics": None,
    "artifact_path": None,
    "meta": {},  # Faz 9.4.2: loaded from meta.json beside artifact; holds training_distributions
}


def _fetch_active(conn) -> dict[str, Any] | None:
    with conn.cursor() as cur:
        cur.execute(
            """
            SELECT model_family, model_version, feature_spec_version,
                   feature_names, artifact_path, metrics_json
            FROM qtss_models
            WHERE active = true
            ORDER BY trained_at DESC
            LIMIT 1
            """
        )
        row = cur.fetchone()
        if not row:
            return None
        cols = [d.name for d in cur.description]
        return dict(zip(cols, row))


def _load_active() -> None:
    """Refresh _STATE from the DB. Safe to call repeatedly."""
    with connect() as conn:
        active = _fetch_active(conn)
    if not active:
        log.warning("no active model in qtss_models — /score will 503")
        with _LOCK:
            _STATE["booster"] = None
        return
    path = Path(active["artifact_path"])
    if not path.exists():
        raise FileNotFoundError(f"artifact missing on disk: {path}")
    booster = lgb.Booster(model_file=str(path))
    # Faz 9.4.2: load meta.json (training_distributions) from beside the artifact.
    meta: dict[str, Any] = {}
    meta_path = path.with_suffix(".meta.json")
    if meta_path.exists():
        try:
            meta = json.loads(meta_path.read_text())
        except Exception:
            log.warning("failed to parse %s — drift endpoint will 422", meta_path)
    with _LOCK:
        _STATE.update(
            booster=booster,
            feature_names=list(active["feature_names"] or []),
            model_family=active["model_family"],
            model_version=active["model_version"],
            feature_spec_version=active["feature_spec_version"],
            metrics=active["metrics_json"],
            artifact_path=str(path),
            meta=meta,
        )
    log.info(
        "loaded model %s/%s (%d features)",
        active["model_family"],
        active["model_version"],
        len(_STATE["feature_names"]),
    )


# ---- request / response models -------------------------------------------

class ScoreRequest(BaseModel):
    # Nested shape matches qtss_features_snapshot.features_by_source.
    features_by_source: dict[str, dict[str, Any]]


class ScoreResponse(BaseModel):
    score: float
    model_family: str
    model_version: str
    # Faz 9.3.5 — Rust side stamps these on `qtss_ml_predictions`. The
    # sidecar already loads them from `qtss_models`; relaying keeps the
    # Rust client schema-free.
    model_name: str
    feature_spec_version: str
    missing_features: int  # how many of feature_names weren't supplied
    n_features: int


class ShapEntry(BaseModel):
    feature: str
    value: float
    contribution: float


class ExplainResponse(BaseModel):
    shap_top10: list[ShapEntry]
    base_value: float
    model_version: str


# ---- feature flattening (kept in sync with features.py) ------------------

def _coerce(v: Any) -> float:
    if v is True:
        return 1.0
    if v is False:
        return 0.0
    if v is None:
        return float("nan")
    try:
        return float(v)
    except (TypeError, ValueError):
        return float("nan")


def _vector(features_by_source: dict[str, dict[str, Any]], order: list[str]) -> tuple[np.ndarray, int]:
    # Flatten once into a dict so lookup is O(1) per expected name.
    flat: dict[str, float] = {}
    for source, feats in features_by_source.items():
        if not isinstance(feats, dict):
            continue
        for name, val in feats.items():
            flat[f"{source}.{name}"] = _coerce(val)
    missing = 0
    vec = np.empty(len(order), dtype=np.float64)
    for i, fname in enumerate(order):
        if fname in flat:
            vec[i] = flat[fname]
        else:
            vec[i] = float("nan")
            missing += 1
    return vec.reshape(1, -1), missing


# ---- app --------------------------------------------------------------------

app = FastAPI(title="qtss-inference", version="0.1.0")


@app.on_event("startup")
def _startup() -> None:
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s: %(message)s")
    try:
        _load_active()
    except Exception as exc:  # noqa: BLE001
        log.error("startup load failed: %s", exc)


@app.get("/health")
def health() -> dict[str, Any]:
    with _LOCK:
        loaded = _STATE["booster"] is not None
        return {
            "ok": True,
            "model_loaded": loaded,
            "model_family": _STATE["model_family"],
            "model_version": _STATE["model_version"],
        }


@app.get("/active")
def active() -> dict[str, Any]:
    with _LOCK:
        if _STATE["booster"] is None:
            raise HTTPException(status_code=503, detail="no active model loaded")
        return {
            "model_family": _STATE["model_family"],
            "model_version": _STATE["model_version"],
            "feature_spec_version": _STATE["feature_spec_version"],
            "artifact_path": _STATE["artifact_path"],
            "metrics": _STATE["metrics"],
            "n_features": len(_STATE["feature_names"]),
        }


@app.post("/reload")
def reload_() -> dict[str, Any]:
    _load_active()
    return active()


@app.post("/score", response_model=ScoreResponse)
def score(req: ScoreRequest) -> ScoreResponse:
    with _LOCK:
        booster = _STATE["booster"]
        order = _STATE["feature_names"]
        mf, mv = _STATE["model_family"], _STATE["model_version"]
    if booster is None:
        raise HTTPException(status_code=503, detail="no active model loaded")
    with _LOCK:
        fsv = _STATE["feature_spec_version"] or ""
    x, missing = _vector(req.features_by_source, order)
    p = float(booster.predict(x)[0])
    return ScoreResponse(
        score=p,
        model_family=mf or "",
        model_version=mv or "",
        model_name=mf or "",  # Model registry uses "family" as the logical name.
        feature_spec_version=fsv,
        missing_features=missing,
        n_features=len(order),
    )


@app.post("/explain", response_model=ExplainResponse)
def explain(req: ScoreRequest) -> ExplainResponse:
    """Return top-10 SHAP contributions for the same vector as /score.

    Faz 9.3.4 — LightGBM's `pred_contrib=True` returns an array of shape
    (n_samples, n_features + 1) where the last column is the model's
    expected value (base / intercept). We sort by |contribution| desc
    and keep the 10 most impactful features.
    """
    with _LOCK:
        booster = _STATE["booster"]
        order = _STATE["feature_names"]
        mv = _STATE["model_version"]
    if booster is None:
        raise HTTPException(status_code=503, detail="no active model loaded")
    x, _missing = _vector(req.features_by_source, order)
    # shape: (1, n_features + 1); last column = base value.
    contrib = booster.predict(x, pred_contrib=True)
    row = np.asarray(contrib)[0]
    base_value = float(row[-1])
    feat_contribs = row[:-1]
    # Flatten the input dict once so we can echo back the raw value.
    flat: dict[str, float] = {}
    for source, feats in req.features_by_source.items():
        if not isinstance(feats, dict):
            continue
        for name, val in feats.items():
            flat[f"{source}.{name}"] = _coerce(val)
    # Top-10 by absolute contribution.
    idxs = np.argsort(-np.abs(feat_contribs))[: min(10, len(order))]
    top = [
        ShapEntry(
            feature=order[i],
            value=float(flat.get(order[i], float("nan"))),
            contribution=float(feat_contribs[i]),
        )
        for i in idxs
    ]
    return ExplainResponse(shap_top10=top, base_value=base_value, model_version=mv or "")


# ---- entrypoint -----------------------------------------------------------

def main() -> None:
    import uvicorn

    # Sidecar bind config is resolved from system_config via the shared
    # resolver so the Rust worker + this sidecar agree on the URL without
    # redeploy. Fallback values match the migration defaults.
    with connect() as conn:
        host = str(resolve_config(conn, "ai", "inference.bind_host", "127.0.0.1"))
        port = int(resolve_config(conn, "ai", "inference.bind_port", 8790))
    uvicorn.run(app, host=host, port=port, log_level="info")


class DriftFeatureEntry(BaseModel):
    feature: str
    psi: float
    status: str  # "ok" | "warning" | "critical"
    buckets: list[dict[str, Any]]


class DriftResponse(BaseModel):
    features: list[DriftFeatureEntry]
    model_version: str
    computed_at: str


def _compute_psi_buckets(
    expected_counts: list[int],
    actual_values: list[float],
    bin_edges: list[float],
) -> tuple[float, list[dict[str, Any]]]:
    """Compute PSI for one feature given training bin edges/counts and live values.

    Returns (psi_total, bucket_details).
    """
    n_expected = sum(expected_counts)
    if n_expected == 0:
        return 0.0, []

    n_actual = len(actual_values)
    if n_actual == 0:
        return 0.0, []

    actual_arr = np.array(actual_values, dtype=np.float64)
    actual_hist, _ = np.histogram(actual_arr, bins=bin_edges)

    eps = 1e-6
    details: list[dict[str, Any]] = []
    psi_total = 0.0
    for i in range(len(expected_counts)):
        exp_pct = max(expected_counts[i] / n_expected, eps)
        act_pct = max(int(actual_hist[i]) / n_actual, eps)
        component = (act_pct - exp_pct) * float(np.log(act_pct / exp_pct))
        psi_total += component
        details.append({
            "lo": float(bin_edges[i]),
            "hi": float(bin_edges[i + 1]),
            "expected_pct": round(exp_pct, 6),
            "actual_pct": round(act_pct, 6),
            "psi_component": round(component, 6),
        })
    return round(psi_total, 6), details


@app.get("/drift/psi", response_model=DriftResponse)
def drift_psi() -> DriftResponse:
    """Faz 9.4.2 — Compute per-feature PSI comparing live feature
    distributions against training-time distributions.

    Requires `_STATE["meta"]` to contain `training_distributions`:
        { "feature_name": { "bin_edges": [...], "counts": [...] } }

    Live features are pulled from `qtss_features_snapshot` for the last
    N hours (configured via `ai.drift.compute_interval_hours`).
    """
    import datetime

    with _LOCK:
        booster = _STATE["booster"]
        feature_names = list(_STATE["feature_names"])
        model_version = _STATE["model_version"] or ""
        meta = _STATE.get("meta") or {}

    if booster is None:
        raise HTTPException(status_code=503, detail="no active model loaded")

    training_dists = meta.get("training_distributions")
    if not training_dists:
        raise HTTPException(
            status_code=422,
            detail={
                "error": "no_training_distributions",
                "message": (
                    "meta.json missing training_distributions "
                    "— retrain with --save-distributions"
                ),
            },
        )

    with connect() as conn:
        window_hours = float(resolve_config(conn, "ai", "drift.compute_interval_hours", 6.0))
        psi_warn = float(resolve_config(conn, "ai", "drift.psi_warning_threshold", 0.1))
        psi_crit = float(resolve_config(conn, "ai", "drift.psi_critical_threshold", 0.25))

        with conn.cursor() as cur:
            cur.execute(
                """
                SELECT features_json
                FROM qtss_features_snapshot
                WHERE created_at >= NOW() - (%s || ' hours')::interval
                """,
                (str(window_hours),),
            )
            rows = cur.fetchall()

    # Collect live feature values per feature name.
    live_values: dict[str, list[float]] = {fn: [] for fn in feature_names}
    for (fj,) in rows:
        if not isinstance(fj, dict):
            continue
        for source, feats in fj.items():
            if not isinstance(feats, dict):
                continue
            for name, val in feats.items():
                key = f"{source}.{name}"
                if key in live_values:
                    try:
                        live_values[key].append(float(val))
                    except (TypeError, ValueError):
                        pass

    entries: list[DriftFeatureEntry] = []
    now_iso = datetime.datetime.now(datetime.timezone.utc).isoformat()

    # Also persist to qtss_ml_drift_snapshots for the Rust API to read.
    insert_rows: list[tuple] = []

    for fn in feature_names:
        dist = training_dists.get(fn)
        if not dist:
            continue
        bin_edges = dist.get("bin_edges", [])
        counts = dist.get("counts", [])
        if len(bin_edges) < 2 or len(counts) != len(bin_edges) - 1:
            continue

        vals = live_values.get(fn, [])
        psi_val, buckets = _compute_psi_buckets(counts, vals, bin_edges)

        status = "ok"
        if psi_val >= psi_crit:
            status = "critical"
        elif psi_val >= psi_warn:
            status = "warning"

        entries.append(DriftFeatureEntry(
            feature=fn, psi=psi_val, status=status, buckets=buckets,
        ))
        insert_rows.append((model_version, fn, psi_val, json.dumps(buckets)))

    # Persist snapshots.
    if insert_rows:
        with connect() as conn:
            with conn.cursor() as cur:
                cur.executemany(
                    """
                    INSERT INTO qtss_ml_drift_snapshots
                        (model_version, feature_name, psi, bucket_detail)
                    VALUES (%s, %s, %s, %s::jsonb)
                    """,
                    insert_rows,
                )
            conn.commit()

    return DriftResponse(
        features=entries, model_version=model_version, computed_at=now_iso,
    )


if __name__ == "__main__":
    main()
