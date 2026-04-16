"""CLI entry point — `qtss-trainer <cmd>`.

Sub-commands live in a dispatch table (CLAUDE.md #1: no scattered
if/else). Adding a new command = one table row + one function.
"""
from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Callable

from . import db, features, loader, model, registry


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
    with db.connect() as conn:
        cfg = db.TrainerConfig.load(conn)
        spec_version = int(
            db.resolve_config(conn, "ai", "feature_store.spec_version", 1)
        )

        df = loader.load_closed(conn)
        if len(df) < cfg.min_rows:
            print(
                f"Not enough rows: {len(df)} < min_rows={cfg.min_rows}. "
                "Waiting for more labeled setups (Faz 9.2.3 birikim).",
                file=sys.stderr,
            )
            return 2

        X = features.flatten_jsonb(df, "features_by_source")
        y = features.encode_label(df, "label")

        if X.shape[1] == 0:
            print("No features found — qtss_features_snapshot empty?", file=sys.stderr)
            return 3

        result = model.train_lgbm(
            X,
            y,
            params=cfg.lgbm_params,
            num_boost_round=cfg.num_boost_round,
            valid_fraction=cfg.validation_fraction,
        )

        version, path = registry.save(
            conn,
            result,
            model_family=cfg.model_family,
            artifact_dir=cfg.artifact_dir,
            feature_spec_version=spec_version,
            trained_by="cli",
            notes=args.notes,
            activate=args.activate,
        )
        print(
            json.dumps(
                {
                    "model_family": cfg.model_family,
                    "model_version": version,
                    "artifact_path": str(path),
                    "n_train": result.n_train,
                    "n_valid": result.n_valid,
                    "metrics": result.metrics,
                    "activated": args.activate,
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
        help="Mark the newly trained model active immediately.",
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
