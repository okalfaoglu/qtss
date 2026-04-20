// Rapor — Faz 12.R
//
// Backtest performance dashboard. Upper strip: per-family KPI cards
// (detections, win rate, avg pnl%, wins/losses/expired). Middle: per-
// pivot-level bar chart per family (grouped bars: wins / losses /
// expired / tp1 / tp2). Lower: filterable subkind detail table.
//
// All data comes from `/v2/reports/backtest-performance` which joins
// qtss_v2_detections (mode=backtest) with qtss_v2_detection_outcomes.
// The endpoint accepts optional family/symbol/timeframe filters; this
// page currently only wires the family filter (kept deliberately
// minimal; symbol/timeframe drill-down can land in a follow-up).

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { apiFetch } from "../lib/api";

interface FamilySummary {
  family: string;
  detections: number;
  evaluated: number;
  wins: number;
  losses: number;
  expired: number;
  win_rate: number;
  avg_pnl_pct: number;
}

interface ReportBucket {
  family: string;
  pivot_level: string;
  subkind: string;
  detections: number;
  evaluated: number;
  wins: number;
  losses: number;
  expired: number;
  tp1_hits: number;
  tp2_hits: number;
  win_rate: number;
  avg_pnl_pct: number;
  median_pnl_pct: number;
  total_pnl_pct: number;
}

interface TierSummary {
  tier: string;
  event: string;
  detections: number;
  evaluated: number;
  wins: number;
  losses: number;
  expired: number;
  win_rate: number;
  avg_pnl_pct: number;
  median_pnl_pct: number;
}

interface ReportSummary {
  families: FamilySummary[];
  buckets: ReportBucket[];
  tier_summary?: TierSummary[];
}

const FAMILIES = ["harmonic", "classical", "elliott", "wyckoff", "pivot_reversal"] as const;
const LEVELS = ["L0", "L1", "L2", "L3"] as const;

function pct(x: number) {
  return `${(x * 100).toFixed(1)}%`;
}

function fmtNum(x: number, digits = 2) {
  return x.toFixed(digits);
}

export function Reports() {
  const [familyFilter, setFamilyFilter] = useState<string>("");
  // Faz 13 — pivot_reversal tier / event / direction filtreleri.
  const [tierFilter, setTierFilter] = useState<string>("");
  const [eventFilter, setEventFilter] = useState<string>("");
  const [directionFilter, setDirectionFilter] = useState<string>("");

  const { data, isLoading, isError, refetch } = useQuery<ReportSummary>({
    queryKey: [
      "reports/backtest-performance",
      familyFilter, tierFilter, eventFilter, directionFilter,
    ],
    queryFn: () => {
      const p = new URLSearchParams();
      if (familyFilter)    p.set("family", familyFilter);
      if (tierFilter)      p.set("tier", tierFilter);
      if (eventFilter)     p.set("event", eventFilter);
      if (directionFilter) p.set("direction", directionFilter);
      const qs = p.toString() ? `?${p.toString()}` : "";
      return apiFetch<ReportSummary>(`/v2/reports/backtest-performance${qs}`);
    },
    refetchOnWindowFocus: false,
  });

  // Per-family × level grid for the middle bar block.
  const levelGrid = useMemo(() => {
    if (!data) return {} as Record<string, Record<string, ReportBucket[]>>;
    const g: Record<string, Record<string, ReportBucket[]>> = {};
    for (const b of data.buckets) {
      g[b.family] ??= {};
      g[b.family][b.pivot_level] ??= [];
      g[b.family][b.pivot_level].push(b);
    }
    return g;
  }, [data]);

  return (
    <div className="flex flex-col gap-6">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold text-zinc-100">
            Rapor · Backtest Performansı
          </h1>
          <p className="text-sm text-zinc-400">
            Faz 12 sweep + outcome-eval çıktılarının pivot-seviyesi özeti.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <select
            value={familyFilter}
            onChange={(e) => setFamilyFilter(e.target.value)}
            className="rounded border border-zinc-700 bg-zinc-900 px-3 py-1 text-sm text-zinc-200"
          >
            <option value="">Tüm Aileler</option>
            {FAMILIES.map((f) => (
              <option key={f} value={f}>
                {f}
              </option>
            ))}
          </select>
          <select
            value={tierFilter}
            onChange={(e) => setTierFilter(e.target.value)}
            className="rounded border border-zinc-700 bg-zinc-900 px-3 py-1 text-sm text-zinc-200"
            title="Tier (pivot_reversal)"
          >
            <option value="">Tüm Tier</option>
            <option value="major">Major (L2/L3)</option>
            <option value="reactive">Reactive (L0/L1)</option>
          </select>
          <select
            value={eventFilter}
            onChange={(e) => setEventFilter(e.target.value)}
            className="rounded border border-zinc-700 bg-zinc-900 px-3 py-1 text-sm text-zinc-200"
            title="Structural event"
          >
            <option value="">Tüm Event</option>
            <option value="choch">CHoCH</option>
            <option value="bos">BOS</option>
            <option value="neutral">Neutral</option>
          </select>
          <select
            value={directionFilter}
            onChange={(e) => setDirectionFilter(e.target.value)}
            className="rounded border border-zinc-700 bg-zinc-900 px-3 py-1 text-sm text-zinc-200"
            title="Direction"
          >
            <option value="">Tüm Yön</option>
            <option value="bull">Bull</option>
            <option value="bear">Bear</option>
          </select>
          <button
            type="button"
            onClick={() => refetch()}
            className="rounded border border-zinc-700 px-3 py-1 text-sm text-zinc-300 hover:border-zinc-500 hover:text-zinc-100"
          >
            Yenile
          </button>
        </div>
      </header>

      {isLoading && <div className="text-zinc-400">Yükleniyor…</div>}
      {isError && <div className="text-rose-400">Veri alınamadı.</div>}

      {data && (
        <>
          {/* FAZ 13 — TIER DASHBOARD (pivot_reversal) */}
          {data.tier_summary && data.tier_summary.length > 0 && (
            <section className="space-y-2">
              <h2 className="text-sm font-semibold uppercase tracking-wide text-zinc-400">
                Pivot Reversal · Tier × Event
              </h2>
              <div className="grid gap-3 md:grid-cols-3 lg:grid-cols-6">
                {data.tier_summary.map((t) => (
                  <TierCard key={`${t.tier}-${t.event}`} row={t} />
                ))}
              </div>
            </section>
          )}

          {/* FAMILY KPI CARDS */}
          <section className="grid gap-3 md:grid-cols-2 lg:grid-cols-4">
            {FAMILIES.map((fam) => {
              const s = data.families.find((f) => f.family === fam);
              return (
                <FamilyCard key={fam} family={fam} summary={s} />
              );
            })}
          </section>

          {/* PER-FAMILY × LEVEL COVERAGE BARS */}
          <section className="grid gap-4 md:grid-cols-2">
            {FAMILIES.map((fam) =>
              familyFilter === "" || familyFilter === fam ? (
                <LevelBreakdown
                  key={fam}
                  family={fam}
                  levels={levelGrid[fam] ?? {}}
                />
              ) : null,
            )}
          </section>

          {/* DETAIL TABLE */}
          <DetailTable
            buckets={
              familyFilter
                ? data.buckets.filter((b) => b.family === familyFilter)
                : data.buckets
            }
          />
        </>
      )}
    </div>
  );
}

function TierCard({ row }: { row: TierSummary }) {
  const tierColor = row.tier === "major" ? "text-cyan-300" : "text-zinc-300";
  const eventColor =
    row.event === "choch" ? "text-emerald-400"
    : row.event === "bos" ? "text-sky-400"
    : "text-zinc-500";
  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-3">
      <div className="mb-1 flex items-baseline justify-between">
        <h4 className={`text-xs font-semibold uppercase tracking-wide ${tierColor}`}>
          {row.tier}
        </h4>
        <span className={`text-[11px] uppercase ${eventColor}`}>{row.event}</span>
      </div>
      <div className="grid grid-cols-2 gap-y-0.5 text-[11px] text-zinc-300">
        <span className="text-zinc-500">Adet</span>
        <span className="text-right">{row.detections}</span>
        <span className="text-zinc-500">WR</span>
        <span className="text-right">{pct(row.win_rate)}</span>
        <span className="text-zinc-500">Avg PnL</span>
        <span className="text-right">{fmtNum(row.avg_pnl_pct)}%</span>
        <span className="text-zinc-500">Med PnL</span>
        <span className="text-right">{fmtNum(row.median_pnl_pct)}%</span>
      </div>
    </div>
  );
}

function FamilyCard({
  family,
  summary,
}: {
  family: string;
  summary?: FamilySummary;
}) {
  const hasData = !!summary && summary.detections > 0;
  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
      <div className="mb-2 flex items-baseline justify-between">
        <h3 className="text-sm font-medium uppercase tracking-wide text-zinc-300">
          {family}
        </h3>
        {hasData ? (
          <span
            className={`text-xs ${
              (summary!.win_rate ?? 0) >= 0.5 ? "text-emerald-400" : "text-amber-400"
            }`}
          >
            WR {pct(summary!.win_rate)}
          </span>
        ) : (
          <span className="text-xs text-zinc-500">no data</span>
        )}
      </div>
      {hasData ? (
        <div className="grid grid-cols-2 gap-y-1 text-xs text-zinc-300">
          <span className="text-zinc-500">Toplam</span>
          <span className="text-right">{summary!.detections}</span>
          <span className="text-zinc-500">Değerlendirilen</span>
          <span className="text-right">{summary!.evaluated}</span>
          <span className="text-emerald-400">Kazanç</span>
          <span className="text-right">{summary!.wins}</span>
          <span className="text-rose-400">Kayıp</span>
          <span className="text-right">{summary!.losses}</span>
          <span className="text-zinc-500">Süresi doldu</span>
          <span className="text-right">{summary!.expired}</span>
          <span className="text-zinc-500">Ort. PnL%</span>
          <span
            className={`text-right ${
              summary!.avg_pnl_pct >= 0 ? "text-emerald-400" : "text-rose-400"
            }`}
          >
            {fmtNum(summary!.avg_pnl_pct)}%
          </span>
        </div>
      ) : (
        <div className="text-xs text-zinc-500">Hiç formasyon üretilmedi.</div>
      )}
    </div>
  );
}

function LevelBreakdown({
  family,
  levels,
}: {
  family: string;
  levels: Record<string, ReportBucket[]>;
}) {
  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
      <h3 className="mb-3 text-sm font-medium uppercase tracking-wide text-zinc-300">
        {family} · Pivot Seviyesi Dağılımı
      </h3>
      <table className="w-full text-xs">
        <thead className="text-zinc-500">
          <tr>
            <th className="px-1 py-1 text-left">Seviye</th>
            <th className="px-1 py-1 text-right">Toplam</th>
            <th className="px-1 py-1 text-right text-emerald-400">W</th>
            <th className="px-1 py-1 text-right text-rose-400">L</th>
            <th className="px-1 py-1 text-right text-zinc-500">E</th>
            <th className="px-1 py-1 text-right">WR</th>
            <th className="px-1 py-1 text-right">TP1</th>
            <th className="px-1 py-1 text-right">TP2</th>
          </tr>
        </thead>
        <tbody className="text-zinc-200">
          {LEVELS.map((lv) => {
            const rows = levels[lv] ?? [];
            if (rows.length === 0) {
              return (
                <tr key={lv} className="border-t border-zinc-800/60">
                  <td className="px-1 py-1">{lv}</td>
                  <td className="px-1 py-1 text-right text-zinc-600" colSpan={7}>
                    —
                  </td>
                </tr>
              );
            }
            const tot = rows.reduce((a, r) => a + r.detections, 0);
            const ev = rows.reduce((a, r) => a + r.evaluated, 0);
            const w = rows.reduce((a, r) => a + r.wins, 0);
            const l = rows.reduce((a, r) => a + r.losses, 0);
            const e = rows.reduce((a, r) => a + r.expired, 0);
            const tp1 = rows.reduce((a, r) => a + r.tp1_hits, 0);
            const tp2 = rows.reduce((a, r) => a + r.tp2_hits, 0);
            const wr = ev > 0 ? w / ev : 0;
            return (
              <tr key={lv} className="border-t border-zinc-800/60">
                <td className="px-1 py-1 font-mono">{lv}</td>
                <td className="px-1 py-1 text-right">{tot}</td>
                <td className="px-1 py-1 text-right text-emerald-400">{w}</td>
                <td className="px-1 py-1 text-right text-rose-400">{l}</td>
                <td className="px-1 py-1 text-right text-zinc-500">{e}</td>
                <td className="px-1 py-1 text-right">{pct(wr)}</td>
                <td className="px-1 py-1 text-right">{tp1}</td>
                <td className="px-1 py-1 text-right">{tp2}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function DetailTable({ buckets }: { buckets: ReportBucket[] }) {
  const [sortKey, setSortKey] = useState<keyof ReportBucket>("detections");
  const [dir, setDir] = useState<"asc" | "desc">("desc");

  const sorted = useMemo(() => {
    const s = [...buckets];
    s.sort((a, b) => {
      const av = a[sortKey] as number | string;
      const bv = b[sortKey] as number | string;
      if (typeof av === "number" && typeof bv === "number") {
        return dir === "asc" ? av - bv : bv - av;
      }
      return dir === "asc"
        ? String(av).localeCompare(String(bv))
        : String(bv).localeCompare(String(av));
    });
    return s;
  }, [buckets, sortKey, dir]);

  const toggle = (k: keyof ReportBucket) => {
    if (sortKey === k) setDir((d) => (d === "asc" ? "desc" : "asc"));
    else {
      setSortKey(k);
      setDir("desc");
    }
  };

  const th = (k: keyof ReportBucket, label: string) => (
    <th
      onClick={() => toggle(k)}
      className="cursor-pointer px-2 py-1 text-right hover:text-zinc-100"
    >
      {label}
      {sortKey === k ? (dir === "asc" ? " ▲" : " ▼") : ""}
    </th>
  );

  return (
    <section className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
      <h3 className="mb-3 text-sm font-medium uppercase tracking-wide text-zinc-300">
        Detay · Subkind Kırılımı
      </h3>
      <div className="max-h-[480px] overflow-auto">
        <table className="w-full text-xs">
          <thead className="sticky top-0 bg-zinc-900 text-zinc-500">
            <tr>
              <th className="px-2 py-1 text-left">Family</th>
              <th className="px-2 py-1 text-left">Level</th>
              <th className="px-2 py-1 text-left">Subkind</th>
              {th("detections", "Toplam")}
              {th("evaluated", "Eval")}
              {th("wins", "W")}
              {th("losses", "L")}
              {th("expired", "E")}
              {th("tp1_hits", "TP1")}
              {th("tp2_hits", "TP2")}
              {th("win_rate", "WR")}
              {th("avg_pnl_pct", "Ort%")}
              {th("median_pnl_pct", "Med%")}
              {th("total_pnl_pct", "Topl%")}
            </tr>
          </thead>
          <tbody className="text-zinc-200">
            {sorted.map((r) => (
              <tr
                key={`${r.family}|${r.pivot_level}|${r.subkind}`}
                className="border-t border-zinc-800/60 hover:bg-zinc-800/30"
              >
                <td className="px-2 py-1">{r.family}</td>
                <td className="px-2 py-1 font-mono">{r.pivot_level}</td>
                <td className="px-2 py-1 font-mono text-zinc-300">
                  {r.subkind}
                </td>
                <td className="px-2 py-1 text-right">{r.detections}</td>
                <td className="px-2 py-1 text-right">{r.evaluated}</td>
                <td className="px-2 py-1 text-right text-emerald-400">
                  {r.wins}
                </td>
                <td className="px-2 py-1 text-right text-rose-400">
                  {r.losses}
                </td>
                <td className="px-2 py-1 text-right text-zinc-500">
                  {r.expired}
                </td>
                <td className="px-2 py-1 text-right">{r.tp1_hits}</td>
                <td className="px-2 py-1 text-right">{r.tp2_hits}</td>
                <td className="px-2 py-1 text-right">{pct(r.win_rate)}</td>
                <td
                  className={`px-2 py-1 text-right ${
                    r.avg_pnl_pct >= 0 ? "text-emerald-400" : "text-rose-400"
                  }`}
                >
                  {fmtNum(r.avg_pnl_pct)}
                </td>
                <td className="px-2 py-1 text-right">
                  {fmtNum(r.median_pnl_pct)}
                </td>
                <td className="px-2 py-1 text-right">
                  {fmtNum(r.total_pnl_pct)}
                </td>
              </tr>
            ))}
            {sorted.length === 0 && (
              <tr>
                <td
                  className="px-2 py-3 text-center text-zinc-500"
                  colSpan={14}
                >
                  Kayıt yok — sweep ve outcome-eval tamamlandıktan sonra
                  veriler burada görünecek.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </section>
  );
}
