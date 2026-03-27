import { useEffect, useRef, useState } from "react";
import { fetchMultiTimeframeLiveCells, type TimeframeLiveCell } from "../api/fetchMultiTimeframeLive";
import { CHART_INTERVALS } from "../lib/chartIntervals";

/** 0 = çoklu TF canlı şerit kapalı. Ana grafik `VITE_LIVE_POLL_MS` ile ayrı kalır. */
function readMtfPollMs(): number {
  const raw = import.meta.env.VITE_MTF_LIVE_POLL_MS;
  if (raw === "0" || raw === "false") return 0;
  const n = parseInt(String(raw ?? "3000"), 10);
  return Number.isFinite(n) && n >= 0 ? n : 3000;
}

function formatPct(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return "—";
  const sign = n > 0 ? "+" : "";
  return `${sign}${n.toFixed(2)}%`;
}

function formatPrice(n: number): string {
  if (!Number.isFinite(n)) return "—";
  const abs = Math.abs(n);
  if (abs >= 1000) return n.toLocaleString(undefined, { maximumFractionDigits: 2 });
  if (abs >= 1) return n.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 4 });
  return n.toLocaleString(undefined, { maximumFractionDigits: 6 });
}

type Props = {
  symbol: string;
  activeInterval: string;
  accessToken?: string | null;
  exchange?: string;
  segment?: string;
  /** Ana grafik OHLC kaynağı ile hizalı (JWT varken de Binance REST). */
  ohlcFromBinanceRest?: boolean;
};

export function MultiTimeframeLiveStrip({
  symbol,
  activeInterval,
  accessToken,
  exchange,
  segment,
  ohlcFromBinanceRest,
}: Props) {
  const [cells, setCells] = useState<TimeframeLiveCell[] | null>(null);
  const seqRef = useRef(0);

  useEffect(() => {
    const pollMs = readMtfPollMs();
    if (pollMs === 0) {
      setCells(null);
      return undefined;
    }
    const sym = symbol.trim();
    if (!sym) {
      setCells(null);
      return undefined;
    }

    const run = async () => {
      if (typeof document !== "undefined" && document.hidden) return;
      const seq = ++seqRef.current;
      const next = await fetchMultiTimeframeLiveCells({
        symbol: sym,
        intervals: CHART_INTERVALS,
        accessToken: accessToken ?? undefined,
        exchange,
        segment,
        ohlcFromBinanceRest,
      });
      if (seq !== seqRef.current) return;
      setCells(next);
    };

    void run();
    const id = window.setInterval(() => void run(), pollMs);
    return () => {
      seqRef.current++;
      window.clearInterval(id);
    };
  }, [symbol, accessToken, exchange, segment, ohlcFromBinanceRest]);

  const pollMs = readMtfPollMs();
  if (pollMs === 0) return null;

  const ivActive = activeInterval.trim();

  return (
    <div
      className="tv-mtf-strip"
      role="region"
      aria-label="Tüm zaman dilimlerinde son mum anlık değişimi"
    >
      <span className="tv-mtf-strip__label muted" title="Son mum: açılışa ve önceki kapanışa göre yüzde (periyodik REST)">
        TF canlı
      </span>
      <div className="tv-mtf-strip__scroll">
        {(cells ?? CHART_INTERVALS.map((interval) => ({ interval, stats: null }))).map((cell) => {
          const active = cell.interval === ivActive;
          const pO = cell.stats?.pctFromOpen;
          const pP = cell.stats?.pctFromPrevClose;
          const up = pO != null && pO > 0;
          const down = pO != null && pO < 0;
          const cls = [
            "tv-mtf-cell",
            "mono",
            active ? "tv-mtf-cell--active" : "",
            up ? "tv-mtf-cell--up" : "",
            down ? "tv-mtf-cell--down" : "",
            !up && !down && cell.stats ? "tv-mtf-cell--flat" : "",
          ]
            .filter(Boolean)
            .join(" ");

          const titleParts = [
            `${cell.interval} · kapanış ${cell.stats ? formatPrice(cell.stats.close) : "—"}`,
            cell.stats
              ? `H ${formatPrice(cell.stats.high)} · L ${formatPrice(cell.stats.low)} · A ${formatPrice(cell.stats.open)}`
              : "",
            `Mum içi: ${formatPct(pO)}`,
            pP != null && Number.isFinite(pP) ? `Önceki kapanışa göre: ${formatPct(pP)}` : "",
            cell.error ? `Hata: ${cell.error.slice(0, 120)}` : "",
          ].filter(Boolean);

          return (
            <span key={cell.interval} className={cls} title={titleParts.join("\n")}>
              <span className="tv-mtf-cell__iv">{cell.interval}</span>
              <span className="tv-mtf-cell__pct">{formatPct(pO)}</span>
              {pP != null && Number.isFinite(pP) ? (
                <span className="tv-mtf-cell__prev">{formatPct(pP)}</span>
              ) : (
                <span className="tv-mtf-cell__prev tv-mtf-cell__prev--empty" aria-hidden />
              )}
            </span>
          );
        })}
      </div>
    </div>
  );
}
