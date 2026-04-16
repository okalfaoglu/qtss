"""LightGBM train + evaluation.

Time-ordered holdout (not random KFold) because setups are an
autoregressive stream — leaking future info through random splits
would inflate AUC by ~5-10%.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Any

import lightgbm as lgb
import numpy as np
import pandas as pd
from sklearn.metrics import average_precision_score, log_loss, roc_auc_score


@dataclass
class TrainResult:
    booster: lgb.Booster
    feature_names: list[str]
    n_train: int
    n_valid: int
    metrics: dict[str, float]
    params: dict[str, Any]
    num_boost_round: int


def time_ordered_split(
    X: pd.DataFrame, y: pd.Series, valid_fraction: float
) -> tuple[pd.DataFrame, pd.Series, pd.DataFrame, pd.Series]:
    if valid_fraction <= 0 or valid_fraction >= 1:
        raise ValueError("valid_fraction must be in (0, 1)")
    n = len(X)
    cut = int(n * (1.0 - valid_fraction))
    return X.iloc[:cut], y.iloc[:cut], X.iloc[cut:], y.iloc[cut:]


def train_lgbm(
    X: pd.DataFrame,
    y: pd.Series,
    *,
    params: dict[str, Any],
    num_boost_round: int,
    valid_fraction: float,
) -> TrainResult:
    x_tr, y_tr, x_va, y_va = time_ordered_split(X, y, valid_fraction)

    d_tr = lgb.Dataset(x_tr, label=y_tr.values, free_raw_data=False)
    d_va = lgb.Dataset(x_va, label=y_va.values, reference=d_tr, free_raw_data=False)

    booster = lgb.train(
        params=params,
        train_set=d_tr,
        num_boost_round=num_boost_round,
        valid_sets=[d_tr, d_va],
        valid_names=["train", "valid"],
        callbacks=[
            lgb.early_stopping(stopping_rounds=50, verbose=False),
            lgb.log_evaluation(period=0),
        ],
    )

    pred_va = booster.predict(x_va, num_iteration=booster.best_iteration)
    metrics = _metrics(y_va.values, pred_va)
    metrics["n_features"] = float(X.shape[1])
    metrics["best_iteration"] = float(booster.best_iteration or 0)

    return TrainResult(
        booster=booster,
        feature_names=list(X.columns),
        n_train=len(x_tr),
        n_valid=len(x_va),
        metrics=metrics,
        params=params,
        num_boost_round=num_boost_round,
    )


def _metrics(y_true: np.ndarray, y_pred: np.ndarray) -> dict[str, float]:
    # Single-class degenerate case (no positives in validation) — AUC
    # undefined, return NaN instead of crashing.
    if len(np.unique(y_true)) < 2:
        return {"auc": float("nan"), "logloss": float("nan"), "pr_auc": float("nan")}
    return {
        "auc": float(roc_auc_score(y_true, y_pred)),
        "logloss": float(log_loss(y_true, y_pred, labels=[0, 1])),
        "pr_auc": float(average_precision_score(y_true, y_pred)),
    }
