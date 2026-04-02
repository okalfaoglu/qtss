#!/usr/bin/env python3
"""Concatenate migrations/*.sql in version order into a single baseline file.

Run from repo root:
  python3 scripts/squash_migrations_into_one.py

Writes migrations/0001_qtss_baseline.sql and removes every other migrations/*.sql
that was used as input.

**Breaking:** Existing databases with old multi-version _sqlx_migrations need a fresh DB.

Already squashed (only 0001_qtss_baseline.sql): prints help and exits 0.
To re-squash from history: restore old `NNNN_*.sql` from git, then run this script.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

BASELINE_NAME = "0001_qtss_baseline.sql"


def migration_sort_key(p: Path) -> tuple[int, str]:
    m = re.match(r"^(\d+)_(.+)\.sql$", p.name)
    if not m:
        return (999999, p.name)
    return (int(m.group(1)), m.group(2))


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    mig_dir = root / "migrations"
    if not mig_dir.is_dir():
        print("migrations/ not found", file=sys.stderr)
        return 1

    all_sql = [p for p in mig_dir.glob("*.sql") if p.is_file()]
    all_sql.sort(key=migration_sort_key)
    baseline_path = mig_dir / BASELINE_NAME

    # Everything except the squashed artifact (replaced on success)
    sources = [p for p in all_sql if p.name != BASELINE_NAME]
    sources.sort(key=migration_sort_key)

    if not sources:
        if baseline_path.is_file():
            print(
                "Nothing to do: only migrations/0001_qtss_baseline.sql is present (already squashed).\n"
                "To re-build the baseline from the old multi-file chain, restore that tree from git, e.g.:\n"
                "  git checkout <commit-before-squash> -- migrations/\n"
                "If `0001_qtss_baseline.sql` blocks the checkout, move it aside first:\n"
                "  mv migrations/0001_qtss_baseline.sql /tmp/\n"
                "  git checkout <commit-before-squash> -- migrations/\n"
                "  python3 scripts/squash_migrations_into_one.py",
                file=sys.stdout,
            )
            return 0
        print("migrations/*.sql yok — boş dizin.", file=sys.stderr)
        return 1

    # If baseline exists alongside 0002+, prepend it so content is not dropped
    input_chain: list[Path] = []
    if baseline_path.is_file():
        input_chain.append(baseline_path)
    input_chain.extend(sources)

    parts: list[str] = [
        "-- QTSS baseline: single migration squashed from historical NNNN_*.sql chain.\n",
        "-- Fresh databases only (or drop _sqlx_migrations / full DB reset).\n",
        "-- Regenerate: python3 scripts/squash_migrations_into_one.py\n\n",
    ]
    for p in input_chain:
        parts.append(f"-- >>> merged from: {p.name}\n")
        text = p.read_text(encoding="utf-8")
        if text and not text.endswith("\n"):
            text += "\n"
        parts.append(text)
        parts.append("\n")

    combined = "".join(parts)
    out_path = mig_dir / BASELINE_NAME
    for p in input_chain:
        if p.exists():
            p.unlink()
    out_path.write_text(combined, encoding="utf-8")

    print(f"Wrote {out_path.relative_to(root)} ({len(input_chain)} file(s) merged).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
