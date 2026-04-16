"""Flatten `features_by_source` JSONB into a wide numeric matrix.

Each key in `features_by_source` is a ConfluenceSource slug
(`wyckoff` / `derivatives` / `regime` / ...). Each value is the
per-source feature JSON. We prefix every field with its source slug
so collisions across sources are impossible:

    features_by_source = {
        "wyckoff":     {"phase_ordinal": 2, "spring_fired": true},
        "derivatives": {"oi_delta_1h": -0.03, "funding_z": 1.2},
    }

    ⇒ columns = [
        "wyckoff.phase_ordinal",
        "wyckoff.spring_fired",
        "derivatives.oi_delta_1h",
        "derivatives.funding_z",
    ]

Booleans → 0/1; missing keys → NaN.
"""
from __future__ import annotations

from typing import Any

import numpy as np
import pandas as pd


def _coerce_scalar(v: Any) -> float:
    if v is True:
        return 1.0
    if v is False:
        return 0.0
    if v is None:
        return np.nan
    try:
        return float(v)
    except (TypeError, ValueError):
        return np.nan


def flatten_jsonb(df: pd.DataFrame, column: str = "features_by_source") -> pd.DataFrame:
    """Return a numeric DataFrame with one column per `source.feature`.

    Non-numeric / unknown values become NaN; LightGBM handles NaN
    natively so we don't impute here.
    """
    rows: list[dict[str, float]] = []
    for raw in df[column].tolist():
        if not raw:
            rows.append({})
            continue
        flat: dict[str, float] = {}
        for source, feats in raw.items():
            if not isinstance(feats, dict):
                continue
            for name, val in feats.items():
                flat[f"{source}.{name}"] = _coerce_scalar(val)
        rows.append(flat)
    feat_df = pd.DataFrame(rows)
    feat_df.index = df.index
    return feat_df


def encode_label(df: pd.DataFrame, column: str = "label") -> pd.Series:
    """Binary encoding: `win` → 1, anything else → 0."""
    return (df[column].astype(str).str.lower() == "win").astype(int)
