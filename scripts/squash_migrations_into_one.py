#!/usr/bin/env python3
"""Concatenate migrations/*.sql in version order into a single baseline file.

Run from repo root:
  python3 scripts/squash_migrations_into_one.py

Writes migrations/0001_qtss_baseline.sql and removes all other migrations/*.sql.
**Breaking:** Existing databases with _sqlx_migrations must be recreated or migrated manually.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path


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

    sql_files = [p for p in mig_dir.glob("*.sql") if p.is_file()]
    # Exclude target output if re-run
    sql_files = [p for p in sql_files if not p.name.startswith("0001_qtss_baseline")]
    sql_files.sort(key=migration_sort_key)

    if not sql_files:
        print("No migration .sql files to squash.", file=sys.stderr)
        return 1

    parts: list[str] = [
        "-- QTSS baseline: single migration squashed from historical NNNN_*.sql chain.\n",
        "-- Fresh databases only (or drop _sqlx_migrations / full DB reset).\n",
        "-- Regenerate: python3 scripts/squash_migrations_into_one.py\n\n",
    ]
    for p in sql_files:
        parts.append(f"-- >>> merged from: {p.name}\n")
        text = p.read_text(encoding="utf-8")
        if text and not text.endswith("\n"):
            text += "\n"
        parts.append(text)
        parts.append("\n")

    out_path = mig_dir / "0001_qtss_baseline.sql"
    out_path.write_text("".join(parts), encoding="utf-8")

    for p in sql_files:
        p.unlink()

    print(f"Wrote {out_path.relative_to(root)} ({len(sql_files)} files merged).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
