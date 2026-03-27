import { useMemo, useState } from "react";

type Side = "long" | "short";

type Props = {
  open: boolean;
  onClose: () => void;
  /** Son tıklanan fiyatı forma yaz */
  lastPrice: number | null;
};

export function ProfitCalculator({ open, onClose, lastPrice }: Props) {
  const [entry, setEntry] = useState("");
  const [exit, setExit] = useState("");
  const [qty, setQty] = useState("1");
  const [side, setSide] = useState<Side>("long");
  const [feePct, setFeePct] = useState("0.1");

  const parsed = useMemo(() => {
    const e = parseFloat(entry.replace(",", "."));
    const x = parseFloat(exit.replace(",", "."));
    const q = parseFloat(qty.replace(",", "."));
    const f = parseFloat(feePct.replace(",", "."));
    return {
      ok: Number.isFinite(e) && Number.isFinite(x) && Number.isFinite(q) && q > 0,
      entry: e,
      exit: x,
      qty: q,
      feePct: Number.isFinite(f) ? f : 0,
    };
  }, [entry, exit, qty, feePct]);

  const pnl = useMemo(() => {
    if (!parsed.ok) return null;
    const { entry: en, exit: ex, qty: q, feePct: fp } = parsed;
    const dir = side === "long" ? 1 : -1;
    const gross = (ex - en) * q * dir;
    const fee = (Math.abs(en * q) + Math.abs(ex * q)) * (fp / 100);
    return { gross, fee, net: gross - fee };
  }, [parsed, side]);

  if (!open) return null;

  return (
    <div className="tv-profit-calc" role="dialog" aria-label="Kar zarar hesabı">
      <div className="tv-profit-calc__head">
        <span>Kar / zarar</span>
        <button type="button" className="tv-icon-btn" onClick={onClose} aria-label="Kapat">
          ×
        </button>
      </div>
      <div className="tv-profit-calc__grid">
        <label>
          Giriş
          <input
            className="mono"
            value={entry}
            onChange={(e) => setEntry(e.target.value)}
            placeholder="0"
          />
        </label>
        <label>
          Çıkış
          <input
            className="mono"
            value={exit}
            onChange={(e) => setExit(e.target.value)}
            placeholder="0"
          />
        </label>
        <label>
          Miktar
          <input
            className="mono"
            value={qty}
            onChange={(e) => setQty(e.target.value)}
            placeholder="1"
          />
        </label>
        <label>
          Taraf
          <select value={side} onChange={(e) => setSide(e.target.value as Side)}>
            <option value="long">Long</option>
            <option value="short">Short</option>
          </select>
        </label>
        <label>
          Ücret % (iki yön)
          <input
            className="mono"
            value={feePct}
            onChange={(e) => setFeePct(e.target.value)}
            placeholder="0.1"
          />
        </label>
      </div>
      {lastPrice != null && Number.isFinite(lastPrice) ? (
        <p className="tv-profit-calc__hint muted">
          Grafik fiyatı: <span className="mono">{lastPrice.toFixed(2)}</span>{" "}
          <button type="button" className="tv-link-btn" onClick={() => setEntry(String(lastPrice))}>
            giriş
          </button>{" "}
          <button type="button" className="tv-link-btn" onClick={() => setExit(String(lastPrice))}>
            çıkış
          </button>
        </p>
      ) : null}
      {pnl ? (
        <div className="tv-profit-calc__result mono">
          <div>Brüt: {pnl.gross.toFixed(4)}</div>
          <div>Ücret: −{pnl.fee.toFixed(4)}</div>
          <div className={pnl.net >= 0 ? "tv-pnl-pos" : "tv-pnl-neg"}>Net: {pnl.net.toFixed(4)}</div>
        </div>
      ) : (
        <p className="muted">Geçerli giriş / çıkış / miktar girin.</p>
      )}
    </div>
  );
}
