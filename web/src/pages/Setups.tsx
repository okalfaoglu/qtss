// Setups listing page — qtss_setups viewer.
//
// Data: GET /v2/setups?mode=&state=&symbol=&limit=200
// Renders active + closed setup rows with mode / state / symbol filters,
// a summary strip (count by state × mode), and a drawer that shows
// tp_ladder + raw_meta for a clicked row. React Query refetches every
// 20s so the view tracks the live pipeline without hammering the API.

import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";

import { apiFetch } from "../lib/api";

type Mode = "all" | "live" | "dry" | "backtest";
type StateFilter = "all" | "armed" | "closed";

interface SetupEntry {
  id: string;
  created_at: string;
  updated_at: string;
  venue_class: string;
  exchange: string;
  symbol: string;
  timeframe: string;
  profile: string;
  alt_type: string | null;
  state: string;
  direction: string;
  mode?: string; // dry / live / backtest — populated by the API but not
  // declared in the Rust struct today; treat as optional.
  entry_price: number | null;
  entry_sl: number | null;
  target_ref: number | null;
  close_reason: string | null;
  close_price: number | null;
  closed_at: string | null;
  pnl_pct: number | null;
  ai_score: number | null;
  trail_mode: boolean | null;
  raw_meta: Record<string, unknown>;
}

interface SetupFeed {
  generated_at: string;
  entries: SetupEntry[];
}

const CLOSED_STATES = new Set([
  "closed",
  "closed_win",
  "closed_loss",
  "closed_manual",
  "closed_partial_win",
  "closed_scratch",
]);

export default function Setups() {
  const [mode, setMode] = useState<Mode>("all");
  const [stateFilter, setStateFilter] = useState<StateFilter>("all");
  const [symbol, setSymbol] = useState("");
  const [selected, setSelected] = useState<SetupEntry | null>(null);

  const q = useQuery<SetupFeed>({
    queryKey: ["setups", mode, symbol],
    queryFn: () => {
      const params = new URLSearchParams({ limit: "300" });
      if (mode !== "all") params.set("mode", mode);
      if (symbol.trim()) params.set("symbol", symbol.trim().toUpperCase());
      return apiFetch(`/v2/setups?${params.toString()}`);
    },
    refetchInterval: 20_000,
  });

  const rows = useMemo(() => {
    const all = q.data?.entries ?? [];
    if (stateFilter === "all") return all;
    if (stateFilter === "armed") {
      return all.filter((r) => !CLOSED_STATES.has(r.state));
    }
    return all.filter((r) => CLOSED_STATES.has(r.state));
  }, [q.data, stateFilter]);

  const summary = useMemo(() => buildSummary(q.data?.entries ?? []), [q.data]);

  return (
    <div className="flex h-full flex-col gap-4 p-4 text-zinc-200">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-bold tracking-wide">
            <span className="text-emerald-400">QTSS</span> SETUPS
          </h1>
          <p className="text-xs text-zinc-500">
            Detector → Confluence → AI Gate → Execution pipeline. Canlı state
            her 20 saniyede bir yenilenir.
          </p>
        </div>
        <div className="text-right text-[10px] uppercase tracking-widest text-zinc-500">
          son güncelleme:
          <br />
          <span className="text-zinc-300">
            {q.data ? new Date(q.data.generated_at).toLocaleString("tr-TR") : "—"}
          </span>
        </div>
      </header>

      {/* Summary strip */}
      <div className="grid grid-cols-2 gap-2 md:grid-cols-5">
        <SumCard label="Toplam" value={summary.total} />
        <SumCard label="Armed" value={summary.armed} accent="text-emerald-300" />
        <SumCard label="Kapalı" value={summary.closed} accent="text-zinc-400" />
        <SumCard label="Dry" value={summary.dry} accent="text-sky-300" />
        <SumCard label="Live" value={summary.live} accent="text-amber-300" />
      </div>

      {/* Filters */}
      <div className="flex flex-wrap items-center gap-3 rounded border border-zinc-800 bg-zinc-950/40 p-3 text-xs">
        <FilterGroup
          label="Mode"
          options={["all", "live", "dry", "backtest"]}
          value={mode}
          onChange={(v) => setMode(v as Mode)}
        />
        <FilterGroup
          label="State"
          options={["all", "armed", "closed"]}
          value={stateFilter}
          onChange={(v) => setStateFilter(v as StateFilter)}
        />
        <label className="flex items-center gap-2 text-zinc-400">
          <span className="uppercase tracking-widest text-zinc-500">Symbol</span>
          <input
            value={symbol}
            onChange={(e) => setSymbol(e.target.value)}
            placeholder="BTCUSDT"
            className="rounded border border-zinc-800 bg-zinc-900 px-2 py-1 text-xs text-zinc-200 focus:border-emerald-500 focus:outline-none"
          />
        </label>
      </div>

      {q.isLoading && (
        <div className="rounded border border-zinc-800 bg-zinc-950/40 p-8 text-center text-zinc-500">
          Yükleniyor…
        </div>
      )}
      {q.isError && (
        <div className="rounded border border-rose-900/60 bg-rose-950/40 p-4 text-rose-300">
          Setup listesi yüklenemedi: {(q.error as Error).message}
        </div>
      )}

      {/* Table */}
      {rows.length > 0 && (
        <div className="overflow-auto rounded border border-zinc-800">
          <table className="w-full border-collapse text-xs">
            <thead className="sticky top-0 bg-zinc-900 text-left uppercase tracking-widest text-zinc-500">
              <tr>
                <Th>Tarih</Th>
                <Th>Mode</Th>
                <Th>Symbol / TF</Th>
                <Th>Yön</Th>
                <Th className="text-right">Entry</Th>
                <Th className="text-right">SL</Th>
                <Th className="text-right">TP</Th>
                <Th>State</Th>
                <Th className="text-right">AI</Th>
                <Th className="text-right">PnL%</Th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr
                  key={r.id}
                  onClick={() => setSelected(r)}
                  className="cursor-pointer border-t border-zinc-900 hover:bg-zinc-900/60"
                >
                  <Td>{new Date(r.created_at).toLocaleString("tr-TR")}</Td>
                  <Td>
                    <ModeBadge mode={r.mode ?? "?"} />
                  </Td>
                  <Td>
                    <div className="font-mono text-zinc-200">{r.symbol}</div>
                    <div className="text-[10px] text-zinc-500">{r.timeframe}</div>
                  </Td>
                  <Td>
                    <DirBadge dir={r.direction} />
                  </Td>
                  <Td className="text-right font-mono">{fmt(r.entry_price)}</Td>
                  <Td className="text-right font-mono text-rose-300">
                    {fmt(r.entry_sl)}
                  </Td>
                  <Td className="text-right font-mono text-emerald-300">
                    {fmt(r.target_ref)}
                  </Td>
                  <Td>
                    <StateBadge state={r.state} />
                  </Td>
                  <Td className="text-right font-mono text-zinc-400">
                    {r.ai_score != null ? r.ai_score.toFixed(2) : "—"}
                  </Td>
                  <Td className={`text-right font-mono ${pnlClass(r.pnl_pct)}`}>
                    {r.pnl_pct != null ? `${r.pnl_pct.toFixed(2)}%` : "—"}
                  </Td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {rows.length === 0 && !q.isLoading && !q.isError && (
        <div className="rounded border border-zinc-800 bg-zinc-950/40 p-8 text-center text-zinc-500">
          Seçilen filtreye uyan setup yok.
        </div>
      )}

      {/* Drawer */}
      {selected && <Drawer row={selected} onClose={() => setSelected(null)} />}

      <div className="mt-auto border-t border-zinc-800 pt-2 text-center text-[10px] uppercase tracking-widest text-zinc-600">
        QTSS SETUPS · {rows.length} satır
      </div>
    </div>
  );
}

// — helpers ---------------------------------------------------------------

function buildSummary(rows: SetupEntry[]) {
  let armed = 0,
    closed = 0,
    dry = 0,
    live = 0;
  for (const r of rows) {
    if (CLOSED_STATES.has(r.state)) closed++;
    else armed++;
    if (r.mode === "dry") dry++;
    if (r.mode === "live") live++;
  }
  return { total: rows.length, armed, closed, dry, live };
}

function fmt(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return "—";
  const abs = Math.abs(n);
  if (abs >= 1000) return n.toFixed(2);
  if (abs >= 1) return n.toFixed(4);
  return n.toFixed(6);
}

function pnlClass(n: number | null): string {
  if (n == null) return "text-zinc-500";
  if (n > 0) return "text-emerald-300";
  if (n < 0) return "text-rose-300";
  return "text-zinc-400";
}

// — sub-components --------------------------------------------------------

function Th({
  children,
  className = "",
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return <th className={`px-2 py-2 ${className}`}>{children}</th>;
}
function Td({
  children,
  className = "",
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return <td className={`px-2 py-1.5 ${className}`}>{children}</td>;
}

function SumCard({
  label,
  value,
  accent = "text-zinc-200",
}: {
  label: string;
  value: number;
  accent?: string;
}) {
  return (
    <div className="rounded border border-zinc-800 bg-zinc-950/40 p-3">
      <div className="text-[10px] uppercase tracking-widest text-zinc-500">
        {label}
      </div>
      <div className={`text-2xl font-bold ${accent}`}>{value}</div>
    </div>
  );
}

function ModeBadge({ mode }: { mode: string }) {
  const cls =
    mode === "live"
      ? "bg-amber-500/20 text-amber-200"
      : mode === "dry"
        ? "bg-sky-500/20 text-sky-200"
        : "bg-zinc-800 text-zinc-400";
  return (
    <span className={`rounded px-2 py-0.5 text-[10px] uppercase ${cls}`}>
      {mode || "?"}
    </span>
  );
}

function DirBadge({ dir }: { dir: string }) {
  const bull = dir === "long" || dir === "buy";
  return (
    <span
      className={`rounded px-2 py-0.5 text-[10px] uppercase ${
        bull
          ? "bg-emerald-500/20 text-emerald-200"
          : "bg-rose-500/20 text-rose-200"
      }`}
    >
      {dir}
    </span>
  );
}

function StateBadge({ state }: { state: string }) {
  const isClosed = CLOSED_STATES.has(state);
  const win = state === "closed_win" || state === "closed_partial_win";
  const cls = !isClosed
    ? "bg-emerald-500/20 text-emerald-200"
    : win
      ? "bg-sky-500/15 text-sky-300"
      : "bg-rose-500/15 text-rose-300";
  return (
    <span className={`rounded px-2 py-0.5 text-[10px] uppercase ${cls}`}>
      {state}
    </span>
  );
}

function FilterGroup({
  label,
  options,
  value,
  onChange,
}: {
  label: string;
  options: string[];
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <div className="flex items-center gap-2">
      <span className="uppercase tracking-widest text-zinc-500">{label}</span>
      <div className="flex gap-1">
        {options.map((opt) => (
          <button
            key={opt}
            type="button"
            onClick={() => onChange(opt)}
            className={`rounded px-2 py-1 text-[11px] uppercase ${
              value === opt
                ? "bg-emerald-500/20 text-emerald-200"
                : "bg-zinc-900 text-zinc-400 hover:bg-zinc-800"
            }`}
          >
            {opt}
          </button>
        ))}
      </div>
    </div>
  );
}

function Drawer({
  row,
  onClose,
}: {
  row: SetupEntry;
  onClose: () => void;
}) {
  const tpLadder = ((row.raw_meta?.["tp_ladder"] as unknown[]) ?? []) as Array<{
    price?: number;
    qty?: number;
  }>;
  return (
    <div
      className="fixed inset-0 z-40 flex justify-end bg-black/60 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="h-full w-[520px] overflow-y-auto border-l border-zinc-800 bg-zinc-950 p-4 text-sm"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-zinc-800 pb-2">
          <div>
            <div className="text-lg font-bold text-emerald-300">
              {row.symbol}{" "}
              <span className="text-xs text-zinc-500">· {row.timeframe}</span>
            </div>
            <div className="text-[10px] uppercase tracking-widest text-zinc-500">
              {row.id}
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded bg-zinc-800 px-3 py-1 text-xs text-zinc-300 hover:bg-zinc-700"
          >
            ×
          </button>
        </div>

        <dl className="mt-3 grid grid-cols-2 gap-y-2 text-xs">
          <KV k="Mode" v={<ModeBadge mode={row.mode ?? ""} />} />
          <KV k="State" v={<StateBadge state={row.state} />} />
          <KV k="Direction" v={<DirBadge dir={row.direction} />} />
          <KV k="Profile" v={row.profile} />
          <KV k="Entry" v={fmt(row.entry_price)} />
          <KV k="SL" v={<span className="text-rose-300">{fmt(row.entry_sl)}</span>} />
          <KV k="Target" v={<span className="text-emerald-300">{fmt(row.target_ref)}</span>} />
          <KV k="AI Score" v={row.ai_score?.toFixed(3) ?? "—"} />
          <KV k="Close" v={fmt(row.close_price)} />
          <KV k="Reason" v={row.close_reason ?? "—"} />
          <KV k="PnL %" v={row.pnl_pct?.toFixed(2) ?? "—"} />
          <KV
            k="Created"
            v={new Date(row.created_at).toLocaleString("tr-TR")}
          />
          <KV
            k="Closed"
            v={
              row.closed_at
                ? new Date(row.closed_at).toLocaleString("tr-TR")
                : "—"
            }
          />
        </dl>

        {tpLadder.length > 0 && (
          <section className="mt-4">
            <h3 className="mb-2 text-xs uppercase tracking-widest text-zinc-500">
              TP Ladder
            </h3>
            <table className="w-full border-collapse text-xs">
              <thead className="text-left text-zinc-500">
                <tr>
                  <th className="px-2 py-1">#</th>
                  <th className="px-2 py-1 text-right">Price</th>
                  <th className="px-2 py-1 text-right">Qty%</th>
                </tr>
              </thead>
              <tbody>
                {tpLadder.map((t, i) => (
                  <tr key={i} className="border-t border-zinc-900">
                    <td className="px-2 py-1">TP{i + 1}</td>
                    <td className="px-2 py-1 text-right font-mono text-emerald-300">
                      {fmt(t.price ?? null)}
                    </td>
                    <td className="px-2 py-1 text-right font-mono">
                      {t.qty != null ? `${(Number(t.qty) * 100).toFixed(0)}%` : "—"}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        )}

        <section className="mt-4">
          <h3 className="mb-2 text-xs uppercase tracking-widest text-zinc-500">
            Raw Meta
          </h3>
          <pre className="max-h-96 overflow-auto rounded bg-zinc-900 p-2 text-[10px] text-zinc-400">
            {JSON.stringify(row.raw_meta, null, 2)}
          </pre>
        </section>
      </div>
    </div>
  );
}

function KV({ k, v }: { k: string; v: React.ReactNode }) {
  return (
    <>
      <dt className="uppercase tracking-widest text-zinc-500">{k}</dt>
      <dd className="font-mono text-zinc-200">{v}</dd>
    </>
  );
}
