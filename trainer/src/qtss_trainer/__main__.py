"""CLI entry point — `qtss-trainer <cmd>`.

Sub-commands live in a dispatch table (CLAUDE.md #1: no scattered
if/else). Adding a new command = one table row + one function.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
from collections.abc import Callable

import numpy as np

from . import db, features, loader, model, registry


def _insert_training_run(
    conn,
    *,
    trigger_source: str,
    status: str,
    n_closed_setups: int | None = None,
    n_features: int | None = None,
    feature_coverage_pct: float | None = None,
    label_balance: float | None = None,
    model_id: str | None = None,
    error_msg: str | None = None,
    notes: str | None = None,
) -> None:
    """Write one qtss_ml_training_runs audit row (Faz 9B migration 0169)."""
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO qtss_ml_training_runs
                (finished_at, trigger_source, status, n_closed_setups, n_features,
                 feature_coverage_pct, label_balance, model_id, error_msg, notes)
            VALUES (now(), %s, %s, %s, %s, %s, %s, %s, %s, %s)
            """,
            (
                trigger_source, status, n_closed_setups, n_features,
                feature_coverage_pct, label_balance, model_id, error_msg, notes,
            ),
        )
    conn.commit()


def _auto_activate_decision(conn, new_auc: float, min_lift: float) -> tuple[bool, str]:
    """Return (should_activate, reason).

    Rules (matches retraining playbook §3):
      - No active model → activate if AUC >= 0.55 (bootstrap).
      - Active exists    → activate if new AUC > old AUC + min_lift.
      - AUC < 0.55       → never (random-guess guard).
    """
    if new_auc < 0.55:
        return False, f"auc {new_auc:.3f} < 0.55 (random-guess floor)"
    with conn.cursor() as cur:
        cur.execute(
            "SELECT (metrics_json->>'auc')::float FROM qtss_models WHERE active=true LIMIT 1"
        )
        row = cur.fetchone()
    if row is None or row[0] is None:
        return True, f"bootstrap (no active model); auc={new_auc:.3f}"
    old_auc = float(row[0])
    if new_auc >= old_auc + min_lift:
        return True, f"auc {new_auc:.3f} ≥ old {old_auc:.3f} + {min_lift}"
    return False, f"auc {new_auc:.3f} < old {old_auc:.3f} + {min_lift} (shadow)"


# --------------------------------------------------------------------------
# Commands
# --------------------------------------------------------------------------

def cmd_stats(args: argparse.Namespace) -> int:
    with db.connect() as conn:
        counts = loader.label_counts(conn)
        closed_df = loader.load_closed(conn)
    payload = {
        "label_counts": counts,
        "closed_rows": len(closed_df),
    }
    print(json.dumps(payload, indent=2, default=str))
    return 0


def cmd_train(args: argparse.Namespace) -> int:
    trigger_source = getattr(args, "trigger_source", None) or os.getenv(
        "QTSS_TRAINER_TRIGGER", "manual"
    )
    with db.connect() as conn:
        cfg = db.TrainerConfig.load(conn)
        spec_version = int(
            db.resolve_config(conn, "ai", "feature_store.spec_version", 1)
        )

        df = loader.load_closed(conn)
        n_closed = len(df)

        # ---- Gate 1: minimum rows (Faz 9B migration 0169 trainer.min_rows) ----
        if n_closed < cfg.min_rows:
            msg = (
                f"insufficient rows: {n_closed} < min_rows={cfg.min_rows}"
            )
            print(msg, file=sys.stderr)
            _insert_training_run(
                conn,
                trigger_source=trigger_source,
                status="skipped_insufficient_data",
                n_closed_setups=n_closed,
                error_msg=msg,
                notes=args.notes,
            )
            return 2

        X = features.flatten_jsonb(df, "features_by_source")
        y = features.encode_label(df, "label")

        if X.shape[1] == 0:
            msg = "no features — qtss_features_snapshot empty?"
            print(msg, file=sys.stderr)
            _insert_training_run(
                conn,
                trigger_source=trigger_source,
                status="skipped_low_coverage",
                n_closed_setups=n_closed,
                n_features=0,
                error_msg=msg,
                notes=args.notes,
            )
            return 3

        # ---- Gate 2: feature coverage (per-feature non-null ratio) ----
        # Each column's non-null ratio; drop columns under threshold before
        # training so LightGBM doesn't see sparse noise. Aggregate coverage
        # is the mean of kept columns' non-null ratio.
        coverage = X.notna().mean(axis=0)  # Series: column → ratio
        keep_cols = coverage[coverage >= cfg.min_feature_coverage_pct].index.tolist()
        dropped = [c for c in X.columns if c not in keep_cols]
        agg_coverage = float(coverage.mean()) if len(coverage) else 0.0

        if not keep_cols:
            msg = (
                f"all {X.shape[1]} features below coverage threshold "
                f"{cfg.min_feature_coverage_pct}"
            )
            print(msg, file=sys.stderr)
            _insert_training_run(
                conn,
                trigger_source=trigger_source,
                status="skipped_low_coverage",
                n_closed_setups=n_closed,
                n_features=X.shape[1],
                feature_coverage_pct=agg_coverage,
                error_msg=msg,
                notes=args.notes,
            )
            return 4

        if dropped:
            print(
                f"dropped {len(dropped)} low-coverage features "
                f"(threshold={cfg.min_feature_coverage_pct}): "
                f"{dropped[:10]}{'…' if len(dropped) > 10 else ''}",
                file=sys.stderr,
            )
            X = X[keep_cols]

        # ---- Gate 3: label balance (minority class ratio) ----
        n_pos = int(y.sum())
        n_neg = int(len(y) - n_pos)
        minority = min(n_pos, n_neg) / max(len(y), 1)
        if minority < cfg.min_label_balance:
            msg = (
                f"label imbalance: minority={minority:.3f} < "
                f"{cfg.min_label_balance} (pos={n_pos}, neg={n_neg})"
            )
            print(msg, file=sys.stderr)
            _insert_training_run(
                conn,
                trigger_source=trigger_source,
                status="skipped_imbalance",
                n_closed_setups=n_closed,
                n_features=len(keep_cols),
                feature_coverage_pct=agg_coverage,
                label_balance=float(minority),
                error_msg=msg,
                notes=args.notes,
            )
            return 5

        # ---- Train ----
        try:
            result = model.train_lgbm(
                X,
                y,
                params=cfg.lgbm_params,
                num_boost_round=cfg.num_boost_round,
                valid_fraction=cfg.validation_fraction,
            )
        except Exception as exc:  # noqa: BLE001
            _insert_training_run(
                conn,
                trigger_source=trigger_source,
                status="failed",
                n_closed_setups=n_closed,
                n_features=len(keep_cols),
                feature_coverage_pct=agg_coverage,
                label_balance=float(minority),
                error_msg=str(exc)[:500],
                notes=args.notes,
            )
            raise

        # ---- Activation policy (retraining playbook §3) ----
        # CLI --activate flag forces; otherwise consult auto_activate_if_better.
        should_activate = bool(args.activate)
        activation_reason = "cli --activate flag" if should_activate else ""
        new_auc = float(result.metrics.get("auc", 0.0))
        if not should_activate and cfg.auto_activate_if_better:
            should_activate, activation_reason = _auto_activate_decision(
                conn, new_auc, cfg.auto_activate_min_auc_lift
            )

        version, path = registry.save(
            conn,
            result,
            model_family=cfg.model_family,
            artifact_dir=cfg.artifact_dir,
            feature_spec_version=spec_version,
            trained_by="cli",
            notes=args.notes,
            activate=should_activate,
        )

        # Audit row — success path.
        with conn.cursor() as cur:
            cur.execute(
                "SELECT id FROM qtss_models WHERE model_family=%s AND model_version=%s",
                (cfg.model_family, version),
            )
            row = cur.fetchone()
            model_id = str(row[0]) if row else None

        _insert_training_run(
            conn,
            trigger_source=trigger_source,
            status="success",
            n_closed_setups=n_closed,
            n_features=len(keep_cols),
            feature_coverage_pct=agg_coverage,
            label_balance=float(minority),
            model_id=model_id,
            notes=(args.notes or "") + f" | activation: {activation_reason}",
        )

        print(
            json.dumps(
                {
                    "model_family": cfg.model_family,
                    "model_version": version,
                    "artifact_path": str(path),
                    "n_train": result.n_train,
                    "n_valid": result.n_valid,
                    "n_features": len(keep_cols),
                    "feature_coverage_pct": agg_coverage,
                    "label_balance": minority,
                    "metrics": result.metrics,
                    "activated": should_activate,
                    "activation_reason": activation_reason,
                    "trigger_source": trigger_source,
                },
                indent=2,
                default=str,
            )
        )
    return 0


def cmd_list(args: argparse.Namespace) -> int:
    with db.connect() as conn:
        rows = registry.list_models(conn, args.family)
    print(json.dumps(rows, indent=2, default=str))
    return 0


def cmd_activate(args: argparse.Namespace) -> int:
    with db.connect() as conn:
        registry.activate(conn, args.family, args.version)
    print(f"activated {args.family}/{args.version}")
    return 0


# --------------------------------------------------------------------------
# Dispatch
# --------------------------------------------------------------------------

Handler = Callable[[argparse.Namespace], int]


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="qtss-trainer")
    sub = parser.add_subparsers(dest="cmd", required=True)

    sub.add_parser("stats", help="Print label counts and closed row count.")

    p_train = sub.add_parser("train", help="Train a new LightGBM model.")
    p_train.add_argument("--notes", default=None)
    p_train.add_argument(
        "--activate",
        action="store_true",
        help="Force active=true (bypass auto-activate gate).",
    )
    p_train.add_argument(
        "--trigger-source",
        default=None,
        choices=["manual", "cron", "drift", "backfill", "outcome_milestone"],
        help="Audit trigger for qtss_ml_training_runs (default: manual or $QTSS_TRAINER_TRIGGER).",
    )

    p_list = sub.add_parser("list", help="List registered models.")
    p_list.add_argument("--family", default=None)

    p_act = sub.add_parser("activate", help="Switch the active model.")
    p_act.add_argument("family")
    p_act.add_argument("version")

    return parser


_COMMANDS: dict[str, Handler] = {
    "stats": cmd_stats,
    "train": cmd_train,
    "list": cmd_list,
    "activate": cmd_activate,
}


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    handler = _COMMANDS[args.cmd]
    return handler(args)


if __name__ == "__main__":
    sys.exit(main())
