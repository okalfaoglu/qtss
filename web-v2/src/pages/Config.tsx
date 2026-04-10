import { FormEvent, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type { ConfigAuditEntry, ConfigEditorView, ConfigEntry } from "../lib/types";

function valuePreview(entry: ConfigEntry): string {
  if (entry.masked) return "••• masked •••";
  try {
    return JSON.stringify(entry.value);
  } catch {
    return String(entry.value);
  }
}

function jsonPreview(v: unknown): string {
  if (v == null) return "—";
  try {
    return JSON.stringify(v, null, 2);
  } catch {
    return String(v);
  }
}

interface UpsertBody {
  module: string;
  config_key: string;
  value: unknown;
  schema_version?: number;
  description?: string;
  is_secret?: boolean;
}

// ─── History Modal ──────────────────────────────────────────────────────

function HistoryModal({
  entry,
  onClose,
}: {
  entry: ConfigEntry;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const historyQuery = useQuery({
    queryKey: ["v2", "config", entry.module, entry.config_key, "history"],
    queryFn: () =>
      apiFetch<ConfigAuditEntry[]>(
        `/v2/config/${encodeURIComponent(entry.module)}/${encodeURIComponent(entry.config_key)}/history?limit=20`,
      ),
  });

  const rollback = useMutation({
    mutationFn: (auditId: number) =>
      apiFetch<ConfigEntry>(
        `/v2/config/${encodeURIComponent(entry.module)}/${encodeURIComponent(entry.config_key)}/rollback`,
        {
          method: "POST",
          body: JSON.stringify({ audit_id: auditId, snapshot: "old" }),
        },
      ),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["v2", "config"] });
      onClose();
    },
  });

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      onClick={onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="max-h-[80vh] w-full max-w-3xl overflow-y-auto rounded-lg border border-zinc-800 bg-zinc-900 p-5 shadow-2xl"
      >
        <div className="mb-4">
          <div className="text-xs uppercase text-zinc-500">{entry.module}</div>
          <div className="font-mono text-base text-zinc-100">{entry.config_key}</div>
          <div className="mt-1 text-xs text-zinc-400">Change History</div>
        </div>

        {historyQuery.isLoading && (
          <div className="text-sm text-zinc-400">Loading history…</div>
        )}
        {historyQuery.isError && (
          <div className="rounded border border-red-800 bg-red-950/40 px-3 py-2 text-xs text-red-300">
            {(historyQuery.error as Error).message}
          </div>
        )}
        {rollback.isError && (
          <div className="mb-3 rounded border border-red-800 bg-red-950/40 px-3 py-2 text-xs text-red-300">
            Rollback failed: {(rollback.error as Error).message}
          </div>
        )}

        {historyQuery.data && historyQuery.data.length === 0 && (
          <div className="text-sm text-zinc-500">No history yet.</div>
        )}

        {historyQuery.data && historyQuery.data.length > 0 && (
          <table className="w-full text-xs">
            <thead className="text-[10px] uppercase text-zinc-500">
              <tr>
                <th className="px-2 py-1 text-left">Time</th>
                <th className="px-2 py-1 text-left">Action</th>
                <th className="px-2 py-1 text-left">Old Value</th>
                <th className="px-2 py-1 text-left">New Value</th>
                <th className="px-2 py-1 text-left" />
              </tr>
            </thead>
            <tbody>
              {historyQuery.data.map((h) => (
                <tr key={h.id} className="border-t border-zinc-800/60">
                  <td className="px-2 py-1.5 font-mono text-zinc-400">
                    {new Date(h.changed_at).toLocaleString()}
                  </td>
                  <td className="px-2 py-1.5">
                    <span
                      className={`rounded px-1.5 py-0.5 text-[10px] font-medium uppercase ${
                        h.action === "create"
                          ? "bg-emerald-500/20 text-emerald-300"
                          : h.action === "delete"
                            ? "bg-red-500/20 text-red-300"
                            : "bg-blue-500/20 text-blue-300"
                      }`}
                    >
                      {h.action}
                    </span>
                  </td>
                  <td className="max-w-[200px] truncate px-2 py-1.5 font-mono text-zinc-400">
                    {jsonPreview(h.old_value)}
                  </td>
                  <td className="max-w-[200px] truncate px-2 py-1.5 font-mono text-zinc-300">
                    {jsonPreview(h.new_value)}
                  </td>
                  <td className="px-2 py-1.5">
                    {h.old_value != null && (
                      <button
                        type="button"
                        disabled={rollback.isPending}
                        onClick={() => {
                          if (confirm(`Restore old value from ${new Date(h.changed_at).toLocaleString()}?`)) {
                            rollback.mutate(h.id);
                          }
                        }}
                        className="rounded border border-amber-600/40 px-2 py-0.5 text-[10px] text-amber-300 hover:border-amber-400 hover:text-amber-200 disabled:opacity-50"
                      >
                        Restore
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}

        <div className="mt-4 flex justify-end">
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-zinc-700 px-3 py-1.5 text-sm text-zinc-300 hover:border-zinc-500"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

// ─── Edit Modal ─────────────────────────────────────────────────────────

function EditModal({
  entry,
  onClose,
}: {
  entry: ConfigEntry;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [text, setText] = useState(() => {
    try {
      return JSON.stringify(entry.value, null, 2);
    } catch {
      return String(entry.value);
    }
  });
  const [parseError, setParseError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: (body: UpsertBody) =>
      apiFetch<ConfigEntry>("/admin/system-config", {
        method: "POST",
        body: JSON.stringify(body),
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["v2", "config"] });
      onClose();
    },
  });

  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    setParseError(null);
    let parsed: unknown;
    try {
      parsed = JSON.parse(text);
    } catch (err) {
      setParseError(`Invalid JSON: ${(err as Error).message}`);
      return;
    }
    mutation.mutate({
      module: entry.module,
      config_key: entry.config_key,
      value: parsed,
      schema_version: entry.schema_version,
      description: entry.description ?? undefined,
      is_secret: entry.is_secret,
    });
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      onClick={onClose}
    >
      <form
        onClick={(e) => e.stopPropagation()}
        onSubmit={handleSubmit}
        className="w-full max-w-2xl space-y-3 rounded-lg border border-zinc-800 bg-zinc-900 p-5 shadow-2xl"
      >
        <div>
          <div className="text-xs uppercase text-zinc-500">{entry.module}</div>
          <div className="font-mono text-base text-zinc-100">{entry.config_key}</div>
          {entry.description && (
            <div className="mt-1 text-xs text-zinc-400">{entry.description}</div>
          )}
        </div>

        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          spellCheck={false}
          className="h-72 w-full resize-y rounded border border-zinc-700 bg-zinc-950 p-3 font-mono text-xs text-zinc-100 focus:border-emerald-500 focus:outline-none"
        />

        {parseError && (
          <div className="rounded border border-red-800 bg-red-950/40 px-3 py-2 text-xs text-red-300">
            {parseError}
          </div>
        )}
        {mutation.isError && (
          <div className="rounded border border-red-800 bg-red-950/40 px-3 py-2 text-xs text-red-300">
            {(mutation.error as Error).message}
          </div>
        )}

        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-zinc-700 px-3 py-1.5 text-sm text-zinc-300 hover:border-zinc-500"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={mutation.isPending}
            className="rounded bg-emerald-500 px-3 py-1.5 text-sm font-medium text-zinc-950 hover:bg-emerald-400 disabled:opacity-50"
          >
            {mutation.isPending ? "Saving…" : "Save"}
          </button>
        </div>
      </form>
    </div>
  );
}

// ─── Group Section ──────────────────────────────────────────────────────

function GroupSection({
  module,
  entries,
  filter,
  onEdit,
  onHistory,
}: {
  module: string;
  entries: ConfigEntry[];
  filter: string;
  onEdit: (entry: ConfigEntry) => void;
  onHistory: (entry: ConfigEntry) => void;
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
              <th className="px-4 py-2 text-left">Actions</th>
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
                <td className="flex gap-1.5 px-4 py-2">
                  <button
                    type="button"
                    onClick={() => onEdit(e)}
                    className="rounded border border-zinc-700 px-2 py-1 text-xs text-zinc-300 hover:border-emerald-500/40 hover:text-emerald-300"
                  >
                    Edit
                  </button>
                  <button
                    type="button"
                    onClick={() => onHistory(e)}
                    className="rounded border border-zinc-700 px-2 py-1 text-xs text-zinc-300 hover:border-blue-500/40 hover:text-blue-300"
                  >
                    History
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

// ─── Page ───────────────────────────────────────────────────────────────

export function Config() {
  const [filter, setFilter] = useState("");
  const [editing, setEditing] = useState<ConfigEntry | null>(null);
  const [historyOf, setHistoryOf] = useState<ConfigEntry | null>(null);
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
            onEdit={setEditing}
            onHistory={setHistoryOf}
          />
        ))}
      {query.data && (
        <div className="text-xs text-zinc-500">Generated at {query.data.generated_at}</div>
      )}

      {editing && <EditModal entry={editing} onClose={() => setEditing(null)} />}
      {historyOf && <HistoryModal entry={historyOf} onClose={() => setHistoryOf(null)} />}
    </div>
  );
}
