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

// Fee schedule shape served by /v2/fees. Setup drawer pulls this once
// so SL/TP impact is rendered net-of-commission without hard-coding
// the bps schedule in the client.
interface FeeSchedule {
  venue: string;
  maker_bps: number | null;
  taker_bps: number | null;
}
interface FeeSnapshot {
  schedules: FeeSchedule[];
  default_taker_bps: number | null;
}

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
  // Fee schedule — fetched once for the drawer and cached by React Query.
  // Cheap: 1 row per venue, static until an admin edits system_config.
  const feeQ = useQuery<FeeSnapshot>({
    queryKey: ["v2-fees"],
    queryFn: () => apiFetch("/v2/fees"),
    staleTime: 60_000,
  });
  const takerBps = feeQ.data?.default_taker_bps ?? 5; // futures default
  // Round-trip (entry + exit) taker commission as a fraction of notional.
  const roundTripPct = (takerBps * 2) / 100; // bps × 2 / 10000 × 100

  const tpLadder = ((row.raw_meta?.["tp_ladder"] as unknown[]) ?? []) as Array<{
    price?: number;
    qty?: number;
    size_pct?: number;
    idx?: number;
  }>;
  const entry = row.entry_price ?? null;
  const sl = row.entry_sl ?? null;
  const target = row.target_ref ?? null;
  const isLong = row.direction === "long" || row.direction === "buy";

  // Signed move percentages — positive = favourable, negative = adverse.
  // For longs, TP is (target-entry)/entry; for shorts it's (entry-target)/entry.
  const slPct = pctMove(entry, sl, isLong, /*favourable=*/ false);
  const targetPct = pctMove(entry, target, isLong, /*favourable=*/ true);
  const rMultiple =
    slPct != null && targetPct != null && slPct !== 0
      ? Math.abs(targetPct / slPct)
      : null;
  // Net-of-commission — TP must outrun round-trip taker, SL gets worse.
  const netTargetPct = targetPct != null ? targetPct - roundTripPct : null;
  const netSlPct = slPct != null ? slPct - roundTripPct : null;
  const netR =
    netSlPct != null && netTargetPct != null && netSlPct !== 0
      ? Math.abs(netTargetPct / netSlPct)
      : null;

  return (
    <div
      className="fixed inset-0 z-40 flex justify-end bg-black/60 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="h-full w-[560px] overflow-y-auto border-l border-zinc-800 bg-zinc-950 p-4 text-sm"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
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

        {/* Price panel — Giris / SL / Target with signed % against entry */}
        <section className="mt-4 rounded border border-zinc-800 bg-gradient-to-br from-zinc-900 to-zinc-950 p-3">
          <div className="mb-3 flex items-center justify-between">
            <h3 className="text-xs uppercase tracking-widest text-zinc-500">
              Fiyat Seviyeleri
            </h3>
            <div className="flex gap-2">
              <ModeBadge mode={row.mode ?? ""} />
              <DirBadge dir={row.direction} />
              <StateBadge state={row.state} />
            </div>
          </div>
          <div className="grid grid-cols-3 gap-2 text-center">
            <PriceCell label="Giriş" value={entry} accent="text-zinc-100" />
            <PriceCell
              label="Stop (SL)"
              value={sl}
              pct={slPct}
              accent="text-rose-300"
            />
            <PriceCell
              label="Kâr Al (TP)"
              value={target}
              pct={targetPct}
              accent="text-emerald-300"
            />
          </div>
          {rMultiple != null && (
            <div className="mt-3 flex items-center justify-between border-t border-zinc-800 pt-2 text-[11px]">
              <span className="uppercase tracking-widest text-zinc-500">
                Risk:Ödül
              </span>
              <span className="font-mono text-zinc-200">
                1 : {rMultiple.toFixed(2)}
              </span>
            </div>
          )}
        </section>

        {/* TP Ladder with % */}
        {tpLadder.length > 0 && (
          <section className="mt-4">
            <h3 className="mb-2 text-xs uppercase tracking-widest text-zinc-500">
              TP Merdiveni
            </h3>
            <table className="w-full border-collapse text-xs">
              <thead className="text-left text-zinc-500">
                <tr>
                  <th className="px-2 py-1">#</th>
                  <th className="px-2 py-1 text-right">Fiyat</th>
                  <th className="px-2 py-1 text-right">Δ %</th>
                  <th className="px-2 py-1 text-right">Qty%</th>
                </tr>
              </thead>
              <tbody>
                {tpLadder.map((t, i) => {
                  const p = pctMove(entry, t.price ?? null, isLong, true);
                  const qty = t.size_pct ?? t.qty;
                  return (
                    <tr key={i} className="border-t border-zinc-900">
                      <td className="px-2 py-1">TP{i + 1}</td>
                      <td className="px-2 py-1 text-right font-mono text-emerald-300">
                        {fmt(t.price ?? null)}
                      </td>
                      <td className="px-2 py-1 text-right font-mono text-emerald-300">
                        {p != null ? `+${p.toFixed(2)}%` : "—"}
                      </td>
                      <td className="px-2 py-1 text-right font-mono">
                        {qty != null
                          ? `${(Number(qty) * 100).toFixed(0)}%`
                          : "—"}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </section>
        )}

        {/* Commission card — round-trip taker on the active schedule */}
        <section className="mt-4 rounded border border-amber-900/40 bg-amber-950/20 p-3">
          <div className="mb-2 flex items-center justify-between">
            <h3 className="text-xs uppercase tracking-widest text-amber-300/80">
              Komisyon (Tahmini)
            </h3>
            <span className="text-[10px] text-zinc-500">
              Binance {row.venue_class === "crypto" ? "futures" : "spot"} · taker
            </span>
          </div>
          <dl className="grid grid-cols-2 gap-y-1 text-xs">
            <KV
              k="Tek yön (bps)"
              v={<span className="font-mono">{takerBps.toFixed(1)}</span>}
            />
            <KV
              k="Round trip %"
              v={
                <span className="font-mono text-amber-200">
                  {roundTripPct.toFixed(3)}%
                </span>
              }
            />
            <KV
              k="Başabaş hareket"
              v={
                <span className="font-mono text-amber-200">
                  ±{roundTripPct.toFixed(3)}%
                </span>
              }
            />
            <KV
              k="Giriş + çıkış"
              v={
                <span className="text-[10px] text-zinc-400">
                  entry_fill + exit_fill = 2 × taker
                </span>
              }
            />
          </dl>
        </section>

        {/* Net expectation after commission */}
        {(netTargetPct != null || netSlPct != null) && (
          <section className="mt-4 rounded border border-zinc-800 bg-zinc-900/40 p-3">
            <h3 className="mb-2 text-xs uppercase tracking-widest text-zinc-500">
              Net Beklenti (Komisyon Sonrası)
            </h3>
            <dl className="grid grid-cols-2 gap-y-1 text-xs">
              <KV
                k="Net TP %"
                v={
                  <span className="font-mono text-emerald-300">
                    {netTargetPct != null
                      ? `+${netTargetPct.toFixed(3)}%`
                      : "—"}
                  </span>
                }
              />
              <KV
                k="Net SL %"
                v={
                  <span className="font-mono text-rose-300">
                    {netSlPct != null ? `${netSlPct.toFixed(3)}%` : "—"}
                  </span>
                }
              />
              <KV
                k="Net R:R"
                v={
                  <span className="font-mono text-zinc-100">
                    {netR != null ? `1 : ${netR.toFixed(2)}` : "—"}
                  </span>
                }
              />
              <KV
                k="Komisyon payı"
                v={
                  <span className="text-[10px] text-amber-200">
                    {targetPct != null && targetPct !== 0
                      ? `${((roundTripPct / targetPct) * 100).toFixed(1)}% of gross TP`
                      : "—"}
                  </span>
                }
              />
            </dl>
          </section>
        )}

        {/* Meta */}
        <section className="mt-4">
          <h3 className="mb-2 text-xs uppercase tracking-widest text-zinc-500">
            Meta
          </h3>
          <dl className="grid grid-cols-2 gap-y-1 text-xs">
            <KV k="Profile" v={row.profile} />
            <KV
              k="AI Score"
              v={row.ai_score != null ? row.ai_score.toFixed(3) : "—"}
            />
            <KV k="Close" v={fmt(row.close_price)} />
            <KV k="Reason" v={row.close_reason ?? "—"} />
            <KV
              k="PnL %"
              v={
                <span className={pnlClass(row.pnl_pct)}>
                  {row.pnl_pct != null ? `${row.pnl_pct.toFixed(2)}%` : "—"}
                </span>
              }
            />
            <KV k="Created" v={new Date(row.created_at).toLocaleString("tr-TR")} />
            <KV
              k="Closed"
              v={
                row.closed_at
                  ? new Date(row.closed_at).toLocaleString("tr-TR")
                  : "—"
              }
            />
          </dl>
        </section>

        {/* Raw JSON (collapsible affordance would be nice; keeping flat so
            it's copy-paste friendly during test runs) */}
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

// Compute the signed % move from entry to `other` for the given
// direction. `favourable=true` → positive means money; `favourable=false`
// → positive means adverse. Returns null when any input is missing.
function pctMove(
  entry: number | null,
  other: number | null,
  isLong: boolean,
  favourable: boolean,
): number | null {
  if (entry == null || other == null || entry === 0) return null;
  const raw = ((other - entry) / entry) * 100;
  // Longs profit when price rises; shorts profit when price falls.
  const signed = isLong ? raw : -raw;
  return favourable ? signed : signed; // caller interprets sign
}

function PriceCell({
  label,
  value,
  pct,
  accent,
}: {
  label: string;
  value: number | null;
  pct?: number | null;
  accent?: string;
}) {
  return (
    <div className="rounded border border-zinc-800 bg-zinc-950 p-2">
      <div className="text-[10px] uppercase tracking-widest text-zinc-500">
        {label}
      </div>
      <div className={`mt-1 font-mono text-base font-semibold ${accent ?? "text-zinc-100"}`}>
        {fmt(value)}
      </div>
      {pct != null && (
        <div
          className={`mt-0.5 text-[11px] font-mono ${
            pct >= 0 ? "text-emerald-300" : "text-rose-300"
          }`}
        >
          {pct >= 0 ? "+" : ""}
          {pct.toFixed(2)}%
        </div>
      )}
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
