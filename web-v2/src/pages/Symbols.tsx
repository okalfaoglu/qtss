import { Fragment, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";

// Faz 14.A8 — Symbol Intelligence panel. qtss_symbol_profile satırlarını
// listeler, operatör risk_tier/category override'ı ve manual_override
// toggle'ı yapabilir. Kategori + tier dağılımı özeti en üstte kart olarak
// durur; görseldeki 13-kategori matrisini okumayı kolaylaştırmak için.

interface SymbolRow {
  exchange: string;
  symbol: string;
  asset_class: string;
  category: string;
  risk_tier: string;
  sector: string | null;
  country: string | null;
  market_cap_usd: number | null;
  avg_daily_vol_usd: number | null;
  price_usd: number | null;
  fundamental_score: number | null;
  liquidity_score: number | null;
  volatility_score: number | null;
  manual_override: boolean;
  notes: string | null;
  source: string | null;
  updated_at: string;
}

interface SymbolFeed {
  generated_at: string;
  total: number;
  entries: SymbolRow[];
}

interface BudgetResponse {
  exchange: string;
  symbol: string;
  risk_tier: string;
  regime: string;
  tier_cap_pct: string;
  regime_multiplier: string;
  fundamental_multiplier: string;
  effective_risk_pct: string;
  notes: string[];
  blocked: string | null;
}

// Canlı risk bütçesi detay satırı: SymbolIntel::compute_budget döküm.
// Detector/strategy'nin setup açarken kullanacağı `effective_risk_pct`'i
// aynı formülle hesaplıyoruz; operatör neyin neden kesildiğini burada görsün.
function BudgetPanel({ exchange, symbol }: { exchange: string; symbol: string }) {
  const q = useQuery({
    queryKey: ["v2", "symbol-budget", exchange, symbol],
    queryFn: () =>
      apiFetch<BudgetResponse>(`/v2/symbols/${exchange}/${symbol}/budget`),
    staleTime: 15_000,
  });
  if (q.isLoading) return <div className="text-zinc-500">Hesaplanıyor…</div>;
  if (q.isError) return <div className="text-rose-400">Hata: {String(q.error)}</div>;
  const b = q.data!;
  const pct = (v: string) => `${(parseFloat(v) * 100).toFixed(3)}%`;
  const mul = (v: string) => `×${parseFloat(v).toFixed(3)}`;
  return (
    <div className="space-y-2 rounded border border-zinc-800 bg-zinc-900/40 p-3">
      {b.blocked && (
        <div className="rounded bg-rose-900/40 px-2 py-1 text-rose-200">
          ENGELLİ: {b.blocked}
        </div>
      )}
      <div className="grid grid-cols-2 gap-2 md:grid-cols-5">
        <div><div className="text-zinc-500">Rejim</div><div className="text-zinc-100">{b.regime || "—"}</div></div>
        <div><div className="text-zinc-500">Tier cap</div><div className="text-zinc-100">{pct(b.tier_cap_pct)}</div></div>
        <div><div className="text-zinc-500">Rejim ×</div><div className="text-zinc-100">{mul(b.regime_multiplier)}</div></div>
        <div><div className="text-zinc-500">Fund ×</div><div className="text-zinc-100">{mul(b.fundamental_multiplier)}</div></div>
        <div><div className="text-zinc-500">Etkin risk %</div><div className="font-semibold text-emerald-300">{pct(b.effective_risk_pct)}</div></div>
      </div>
      {b.notes.length > 0 && (
        <ul className="list-inside list-disc text-zinc-400">
          {b.notes.map((n, i) => <li key={i}>{n}</li>)}
        </ul>
      )}
    </div>
  );
}

const EXCHANGES = ["", "binance", "nasdaq", "bist"];
const TIERS = ["", "core", "balanced", "growth", "speculative", "extreme"];
const CATEGORIES = [
  "",
  "mega_cap",
  "large_cap",
  "mid_cap",
  "small_cap",
  "growth",
  "speculative",
  "micro_penny",
  "holding",
  "endeks",
  "emtia",
  "forex",
  "vadeli",
  "kripto",
];

function fmtMoney(v: number | null): string {
  if (v == null) return "—";
  if (v >= 1e9) return `$${(v / 1e9).toFixed(2)}B`;
  if (v >= 1e6) return `$${(v / 1e6).toFixed(2)}M`;
  if (v >= 1e3) return `$${(v / 1e3).toFixed(1)}K`;
  return `$${v.toFixed(2)}`;
}

function tierColor(tier: string): string {
  const m: Record<string, string> = {
    core: "bg-emerald-900/50 text-emerald-200",
    balanced: "bg-sky-900/50 text-sky-200",
    growth: "bg-amber-900/50 text-amber-200",
    speculative: "bg-orange-900/50 text-orange-200",
    extreme: "bg-rose-900/50 text-rose-200",
  };
  return m[tier] ?? "bg-zinc-800 text-zinc-300";
}

export function Symbols() {
  const qc = useQueryClient();
  const [filter, setFilter] = useState({ exchange: "binance", tier: "", category: "", q: "" });
  const [expanded, setExpanded] = useState<string | null>(null);

  const query = useQuery({
    queryKey: ["v2", "symbols", filter],
    queryFn: () => {
      const qs = new URLSearchParams();
      if (filter.exchange) qs.set("exchange", filter.exchange);
      if (filter.tier) qs.set("tier", filter.tier);
      if (filter.category) qs.set("category", filter.category);
      if (filter.q) qs.set("q", filter.q);
      return apiFetch<SymbolFeed>(`/v2/symbols?${qs.toString()}`);
    },
    refetchInterval: 30_000,
  });

  const patchMut = useMutation({
    mutationFn: ({
      exchange,
      symbol,
      patch,
    }: {
      exchange: string;
      symbol: string;
      patch: Partial<Pick<SymbolRow, "manual_override" | "risk_tier" | "category" | "notes">>;
    }) =>
      apiFetch(`/v2/symbols/${exchange}/${symbol}/override`, {
        method: "POST",
        body: JSON.stringify(patch),
      }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["v2", "symbols"] }),
  });

  const entries = query.data?.entries ?? [];

  const breakdown = useMemo(() => {
    const byTier: Record<string, number> = {};
    const byCat: Record<string, number> = {};
    for (const e of entries) {
      byTier[e.risk_tier] = (byTier[e.risk_tier] ?? 0) + 1;
      byCat[e.category] = (byCat[e.category] ?? 0) + 1;
    }
    return { byTier, byCat };
  }, [entries]);

  return (
    <div className="space-y-4">
      <div className="flex items-baseline justify-between">
        <div className="text-xs uppercase tracking-wide text-zinc-500">
          Symbol Intelligence — {entries.length} / {query.data?.total ?? 0} kayıt
        </div>
        <div className="text-xs text-zinc-500">
          {query.data?.generated_at ? new Date(query.data.generated_at).toLocaleString() : "—"}
        </div>
      </div>

      {/* Breakdown */}
      <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
        <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-3">
          <div className="mb-2 text-xs font-semibold text-zinc-400">Risk Tier</div>
          <div className="flex flex-wrap gap-2 text-xs">
            {Object.entries(breakdown.byTier)
              .sort((a, b) => b[1] - a[1])
              .map(([k, v]) => (
                <span key={k} className={`rounded px-2 py-0.5 ${tierColor(k)}`}>
                  {k}: {v}
                </span>
              ))}
          </div>
        </div>
        <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-3">
          <div className="mb-2 text-xs font-semibold text-zinc-400">Kategori</div>
          <div className="flex flex-wrap gap-2 text-xs">
            {Object.entries(breakdown.byCat)
              .sort((a, b) => b[1] - a[1])
              .map(([k, v]) => (
                <span key={k} className="rounded bg-zinc-800 px-2 py-0.5 text-zinc-300">
                  {k}: {v}
                </span>
              ))}
          </div>
        </div>
      </div>

      {/* Filters */}
      <div className="grid grid-cols-2 gap-2 text-xs md:grid-cols-5">
        <select
          value={filter.exchange}
          onChange={(e) => setFilter({ ...filter, exchange: e.target.value })}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-zinc-200"
        >
          {EXCHANGES.map((x) => (
            <option key={x} value={x}>
              {x || "(tüm borsalar)"}
            </option>
          ))}
        </select>
        <select
          value={filter.tier}
          onChange={(e) => setFilter({ ...filter, tier: e.target.value })}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-zinc-200"
        >
          {TIERS.map((x) => (
            <option key={x} value={x}>
              {x || "(tüm tier)"}
            </option>
          ))}
        </select>
        <select
          value={filter.category}
          onChange={(e) => setFilter({ ...filter, category: e.target.value })}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-zinc-200"
        >
          {CATEGORIES.map((x) => (
            <option key={x} value={x}>
              {x || "(tüm kategori)"}
            </option>
          ))}
        </select>
        <input
          type="text"
          value={filter.q}
          onChange={(e) => setFilter({ ...filter, q: e.target.value })}
          placeholder="sembol ara: BTC"
          className="col-span-2 rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-zinc-200"
        />
      </div>

      {/* Table */}
      <div className="overflow-x-auto rounded-lg border border-zinc-800">
        <table className="w-full text-left text-xs">
          <thead className="bg-zinc-900/60 text-zinc-400">
            <tr>
              <th className="px-2 py-2">Exchange</th>
              <th className="px-2 py-2">Symbol</th>
              <th className="px-2 py-2">Kategori</th>
              <th className="px-2 py-2">Tier</th>
              <th className="px-2 py-2">Sektör</th>
              <th className="px-2 py-2 text-right">MCap</th>
              <th className="px-2 py-2 text-right">30g Vol</th>
              <th className="px-2 py-2 text-right">Fund</th>
              <th className="px-2 py-2">Pin</th>
              <th className="px-2 py-2">Kaynak</th>
              <th className="px-2 py-2">Risk</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((e) => {
              const key = `${e.exchange}/${e.symbol}`;
              const isOpen = expanded === key;
              return (
              <Fragment key={key}>
              <tr className="border-t border-zinc-900 hover:bg-zinc-900/40">
                <td className="px-2 py-1 text-zinc-400">{e.exchange}</td>
                <td className="px-2 py-1 font-mono text-zinc-100">{e.symbol}</td>
                <td className="px-2 py-1">
                  <select
                    value={e.category}
                    onChange={(ev) =>
                      patchMut.mutate({
                        exchange: e.exchange,
                        symbol: e.symbol,
                        patch: { category: ev.target.value, manual_override: true },
                      })
                    }
                    className="rounded border border-zinc-700 bg-zinc-900 px-1 py-0.5 text-zinc-200"
                  >
                    {CATEGORIES.filter((c) => c).map((c) => (
                      <option key={c} value={c}>
                        {c}
                      </option>
                    ))}
                  </select>
                </td>
                <td className="px-2 py-1">
                  <select
                    value={e.risk_tier}
                    onChange={(ev) =>
                      patchMut.mutate({
                        exchange: e.exchange,
                        symbol: e.symbol,
                        patch: { risk_tier: ev.target.value, manual_override: true },
                      })
                    }
                    className={`rounded px-1 py-0.5 ${tierColor(e.risk_tier)}`}
                  >
                    {TIERS.filter((t) => t).map((t) => (
                      <option key={t} value={t}>
                        {t}
                      </option>
                    ))}
                  </select>
                </td>
                <td className="px-2 py-1 text-zinc-400">{e.sector ?? "—"}</td>
                <td className="px-2 py-1 text-right text-zinc-300">{fmtMoney(e.market_cap_usd)}</td>
                <td className="px-2 py-1 text-right text-zinc-300">{fmtMoney(e.avg_daily_vol_usd)}</td>
                <td className="px-2 py-1 text-right text-zinc-300">{e.fundamental_score ?? "—"}</td>
                <td className="px-2 py-1">
                  <input
                    type="checkbox"
                    checked={e.manual_override}
                    onChange={(ev) =>
                      patchMut.mutate({
                        exchange: e.exchange,
                        symbol: e.symbol,
                        patch: { manual_override: ev.target.checked },
                      })
                    }
                  />
                </td>
                <td className="px-2 py-1 text-zinc-500">{e.source ?? "—"}</td>
                <td className="px-2 py-1">
                  <button
                    type="button"
                    onClick={() => setExpanded(isOpen ? null : key)}
                    className="rounded border border-zinc-700 px-2 py-0.5 text-zinc-300 hover:border-emerald-500 hover:text-emerald-300"
                  >
                    {isOpen ? "−" : "Risk"}
                  </button>
                </td>
              </tr>
              {isOpen && (
                <tr className="border-t border-zinc-900 bg-zinc-950/50">
                  <td colSpan={11} className="px-3 py-2">
                    <BudgetPanel exchange={e.exchange} symbol={e.symbol} />
                  </td>
                </tr>
              )}
              </Fragment>
              );
            })}
            {entries.length === 0 && (
              <tr>
                <td colSpan={11} className="px-2 py-6 text-center text-zinc-500">
                  Kayıt yok. `symbol-catalog-refresh` binary'sini çalıştırın.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
