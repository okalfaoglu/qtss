import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";

// Manuel engine_symbols yönetimi. Yeni satır ekleme, enable/disable
// toggle, silme. RBAC: backend admin role gate'liyor.

interface EngineSymbolEntry {
  id: string;
  exchange: string;
  segment: string;
  symbol: string;
  interval: string;
  enabled: boolean;
  label: string | null;
  signal_direction_mode: string;
  lifecycle_state: string;
  created_at: string;
  updated_at: string;
}

interface EngineSymbolFeed {
  generated_at: string;
  entries: EngineSymbolEntry[];
}

const EXCHANGES = ["binance", "bybit", "okx", "bist"];
const SEGMENTS = ["spot", "futures", "usdm", "coinm", "equity"];
const INTERVALS = ["1m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "12h", "1d", "1w"];
const DIRECTION_MODES = ["auto_segment", "both", "long_only", "short_only"];

function fmtTs(iso: string): string {
  return iso.replace("T", " ").replace(/\.\d+Z$/, "Z");
}

export function EngineSymbols() {
  const qc = useQueryClient();
  const query = useQuery({
    queryKey: ["v2", "engine-symbols"],
    queryFn: () => apiFetch<EngineSymbolFeed>("/v2/engine-symbols"),
    refetchInterval: 10_000,
  });

  const [form, setForm] = useState({
    exchange: "binance",
    segment: "futures",
    symbol: "",
    interval: "1h",
    label: "",
    signal_direction_mode: "auto_segment",
  });
  const [filter, setFilter] = useState("");

  const createMut = useMutation({
    mutationFn: (body: typeof form) =>
      apiFetch<EngineSymbolEntry>("/v2/engine-symbols", {
        method: "POST",
        body: JSON.stringify({
          ...body,
          symbol: body.symbol.trim().toUpperCase(),
          label: body.label.trim() || null,
        }),
      }),
    onSuccess: () => {
      setForm((f) => ({ ...f, symbol: "", label: "" }));
      qc.invalidateQueries({ queryKey: ["v2", "engine-symbols"] });
    },
  });

  const patchMut = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      apiFetch(`/v2/engine-symbols/${id}`, {
        method: "PATCH",
        body: JSON.stringify({ enabled }),
      }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["v2", "engine-symbols"] }),
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) =>
      apiFetch(`/v2/engine-symbols/${id}`, { method: "DELETE" }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["v2", "engine-symbols"] }),
  });

  const entries = (query.data?.entries ?? []).filter((e) => {
    if (!filter) return true;
    const q = filter.toLowerCase();
    return (
      e.symbol.toLowerCase().includes(q) ||
      e.exchange.toLowerCase().includes(q) ||
      e.segment.toLowerCase().includes(q)
    );
  });

  return (
    <div className="space-y-4">
      <div className="flex items-baseline justify-between">
        <div className="text-xs uppercase tracking-wide text-zinc-500">
          Engine Symbols — {entries.length} kayıt
        </div>
        <input
          type="text"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="ara: BTC, binance, futures..."
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-xs text-zinc-200"
        />
      </div>

      {/* Create form */}
      <form
        onSubmit={(e) => {
          e.preventDefault();
          createMut.mutate(form);
        }}
        className="grid grid-cols-1 gap-2 rounded-lg border border-zinc-800 bg-zinc-900/40 p-3 text-xs md:grid-cols-7"
      >
        <select
          value={form.exchange}
          onChange={(e) => setForm({ ...form, exchange: e.target.value })}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-zinc-200"
        >
          {EXCHANGES.map((x) => (
            <option key={x} value={x}>
              {x}
            </option>
          ))}
        </select>
        <select
          value={form.segment}
          onChange={(e) => setForm({ ...form, segment: e.target.value })}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-zinc-200"
        >
          {SEGMENTS.map((x) => (
            <option key={x} value={x}>
              {x}
            </option>
          ))}
        </select>
        <input
          type="text"
          required
          value={form.symbol}
          onChange={(e) => setForm({ ...form, symbol: e.target.value })}
          placeholder="BTCUSDT"
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 font-mono uppercase text-zinc-200"
        />
        <select
          value={form.interval}
          onChange={(e) => setForm({ ...form, interval: e.target.value })}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-zinc-200"
        >
          {INTERVALS.map((x) => (
            <option key={x} value={x}>
              {x}
            </option>
          ))}
        </select>
        <select
          value={form.signal_direction_mode}
          onChange={(e) =>
            setForm({ ...form, signal_direction_mode: e.target.value })
          }
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-zinc-200"
        >
          {DIRECTION_MODES.map((x) => (
            <option key={x} value={x}>
              {x}
            </option>
          ))}
        </select>
        <input
          type="text"
          value={form.label}
          onChange={(e) => setForm({ ...form, label: e.target.value })}
          placeholder="label (opsiyonel)"
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-zinc-200"
        />
        <button
          type="submit"
          disabled={createMut.isPending}
          className="rounded border border-emerald-600 bg-emerald-600/20 px-3 py-1 text-emerald-200 hover:bg-emerald-600/30 disabled:opacity-50"
        >
          {createMut.isPending ? "Ekleniyor..." : "Ekle"}
        </button>
        {createMut.isError && (
          <div className="md:col-span-7 text-red-300">
            {(createMut.error as Error).message}
          </div>
        )}
      </form>

      {query.isLoading && <div className="text-sm text-zinc-400">Yükleniyor…</div>}
      {query.isError && (
        <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
          {(query.error as Error).message}
        </div>
      )}

      {entries.length > 0 && (
        <div className="overflow-x-auto rounded-lg border border-zinc-800 bg-zinc-900/40">
          <table className="min-w-full">
            <thead className="bg-zinc-900/80 text-[10px] uppercase tracking-wide text-zinc-500">
              <tr>
                <th className="px-2 py-2 text-left">Exchange</th>
                <th className="px-2 py-2 text-left">Segment</th>
                <th className="px-2 py-2 text-left">Symbol</th>
                <th className="px-2 py-2 text-left">TF</th>
                <th className="px-2 py-2 text-left">Dir</th>
                <th className="px-2 py-2 text-left">Lifecycle</th>
                <th className="px-2 py-2 text-left">Label</th>
                <th className="px-2 py-2 text-left">Updated</th>
                <th className="px-2 py-2 text-right">Aksiyon</th>
              </tr>
            </thead>
            <tbody>
              {entries.map((e) => (
                <tr key={e.id} className="border-b border-zinc-800/60 text-xs">
                  <td className="px-2 py-1.5 text-zinc-400">{e.exchange}</td>
                  <td className="px-2 py-1.5 text-zinc-500">{e.segment}</td>
                  <td className="px-2 py-1.5 font-mono text-zinc-200">{e.symbol}</td>
                  <td className="px-2 py-1.5 text-zinc-400">{e.interval}</td>
                  <td className="px-2 py-1.5 text-[10px] text-zinc-500">
                    {e.signal_direction_mode}
                  </td>
                  <td className="px-2 py-1.5 text-[10px] text-zinc-500">
                    {e.lifecycle_state}
                  </td>
                  <td className="px-2 py-1.5 text-zinc-500">{e.label ?? "—"}</td>
                  <td className="px-2 py-1.5 text-[10px] text-zinc-600">
                    {fmtTs(e.updated_at)}
                  </td>
                  <td className="px-2 py-1.5 text-right">
                    <div className="flex justify-end gap-1">
                      <button
                        onClick={() =>
                          patchMut.mutate({ id: e.id, enabled: !e.enabled })
                        }
                        disabled={patchMut.isPending}
                        className={`rounded border px-2 py-0.5 text-[10px] ${
                          e.enabled
                            ? "border-emerald-600/50 bg-emerald-600/15 text-emerald-300"
                            : "border-zinc-700 text-zinc-500"
                        }`}
                      >
                        {e.enabled ? "enabled" : "disabled"}
                      </button>
                      <button
                        onClick={() => {
                          if (confirm(`${e.symbol} ${e.interval} silinsin mi?`)) {
                            deleteMut.mutate(e.id);
                          }
                        }}
                        disabled={deleteMut.isPending}
                        className="rounded border border-red-700/50 bg-red-700/15 px-2 py-0.5 text-[10px] text-red-300 hover:bg-red-700/30"
                      >
                        sil
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
