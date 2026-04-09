import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type { ConfigEditorView, ConfigEntry } from "../lib/types";

function valuePreview(entry: ConfigEntry): string {
  if (entry.masked) return "••• masked •••";
  try {
    return JSON.stringify(entry.value);
  } catch {
    return String(entry.value);
  }
}

function GroupSection({
  module,
  entries,
  filter,
}: {
  module: string;
  entries: ConfigEntry[];
  filter: string;
}) {
  const [open, setOpen] = useState(true);
  const filtered = useMemo(() => {
    if (!filter) return entries;
    const q = filter.toLowerCase();
    return entries.filter(
      (e) =>
        e.config_key.toLowerCase().includes(q) ||
        (e.description ?? "").toLowerCase().includes(q),
    );
  }, [entries, filter]);

  if (filter && filtered.length === 0) return null;

  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900/60">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center justify-between px-4 py-3 text-left"
      >
        <div className="text-sm font-semibold text-zinc-100">
          {module}
          <span className="ml-2 text-xs text-zinc-500">({filtered.length})</span>
        </div>
        <span className="text-xs text-zinc-500">{open ? "−" : "+"}</span>
      </button>
      {open && (
        <table className="w-full border-t border-zinc-800 text-sm">
          <thead className="text-xs uppercase text-zinc-500">
            <tr>
              <th className="px-4 py-2 text-left">Key</th>
              <th className="px-4 py-2 text-left">Value</th>
              <th className="px-4 py-2 text-left">Description</th>
              <th className="px-4 py-2 text-left">Updated</th>
            </tr>
          </thead>
          <tbody>
            {filtered.map((e) => (
              <tr key={e.config_key} className="border-t border-zinc-800/60">
                <td className="px-4 py-2 font-mono text-zinc-200">
                  {e.config_key}
                  {e.is_secret && (
                    <span className="ml-2 rounded border border-amber-500/40 bg-amber-500/10 px-1 py-0.5 text-[10px] uppercase text-amber-300">
                      secret
                    </span>
                  )}
                </td>
                <td className="px-4 py-2 font-mono text-xs text-zinc-300">
                  {valuePreview(e)}
                </td>
                <td className="px-4 py-2 text-xs text-zinc-400">{e.description ?? "—"}</td>
                <td className="px-4 py-2 font-mono text-xs text-zinc-500">{e.updated_at}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

export function Config() {
  const [filter, setFilter] = useState("");
  const query = useQuery({
    queryKey: ["v2", "config"],
    queryFn: () => apiFetch<ConfigEditorView>("/v2/config?limit=500"),
    refetchInterval: 30_000,
  });

  return (
    <div className="space-y-4">
      <input
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
        placeholder="Filter by key or description…"
        className="w-full max-w-md rounded border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm text-zinc-100"
      />

      {query.isLoading && <div className="text-sm text-zinc-400">Loading config…</div>}
      {query.isError && (
        <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
          Failed: {(query.error as Error).message}
        </div>
      )}
      {query.data &&
        query.data.groups.map((g) => (
          <GroupSection
            key={g.module}
            module={g.module}
            entries={g.entries}
            filter={filter}
          />
        ))}
      {query.data && (
        <div className="text-xs text-zinc-500">Generated at {query.data.generated_at}</div>
      )}
    </div>
  );
}
