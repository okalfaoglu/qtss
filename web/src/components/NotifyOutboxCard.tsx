import { useCallback, useEffect, useMemo, useState } from "react";
import { fetchNotifyOutbox, type NotifyOutboxRowApi } from "../api/client";

type Props = {
  accessToken: string;
  /** Increment (e.g. after enqueue from Template tab) to re-fetch the table. */
  refreshSignal?: number;
};

function asIso(s: string | null | undefined): string {
  if (!s) return "—";
  const d = new Date(s);
  if (!Number.isFinite(d.getTime())) return s;
  return d.toISOString().replace("T", " ").replace("Z", "Z");
}

export function NotifyOutboxCard({ accessToken, refreshSignal = 0 }: Props) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [limit, setLimit] = useState(50);
  const [status, setStatus] = useState<"all" | "pending" | "sending" | "sent" | "failed">("all");
  const [eventKey, setEventKey] = useState("");
  const [exchange, setExchange] = useState("");
  const [segment, setSegment] = useState("");
  const [symbol, setSymbol] = useState("");
  const [q, setQ] = useState("");
  const [rows, setRows] = useState<NotifyOutboxRowApi[]>([]);

  const refresh = useCallback(async () => {
    setBusy(true);
    setErr("");
    try {
      const list = await fetchNotifyOutbox(accessToken, {
        limit: Math.min(200, Math.max(1, limit)),
        status: status === "all" ? "" : status,
        event_key: eventKey,
        exchange,
        segment,
        symbol,
        q,
      });
      setRows(list);
    } catch (e: any) {
      setErr(String(e?.message ?? e));
    } finally {
      setBusy(false);
    }
  }, [accessToken, limit, status, eventKey, exchange, segment, symbol, q]);

  useEffect(() => {
    void refresh();
  }, [refresh, refreshSignal]);

  const filtered = useMemo(() => {
    const term = q.trim().toLowerCase();
    let list = rows.slice();
    if (term) {
      list = list.filter((r) => {
        const ch = (r.channels ?? []).join(",").toLowerCase();
        return (
          (r.title ?? "").toLowerCase().includes(term) ||
          (r.body ?? "").toLowerCase().includes(term) ||
          ch.includes(term) ||
          (r.last_error ?? "").toLowerCase().includes(term)
        );
      });
    }
    return list;
  }, [rows, q]);

  return (
    <div className="card">
      <p className="tv-drawer__section-head">Bildirim kuyruğu (notify_outbox)</p>
      <p className="muted" style={{ marginTop: 0, fontSize: "0.82rem" }}>
        <code className="mono">GET /api/v1/notify/outbox</code> — worker gönderimi{" "}
        <code className="mono">notify_outbox_loop</code> ile yapılır.
      </p>

      <div style={{ display: "flex", gap: "0.45rem", flexWrap: "wrap", alignItems: "end" }}>
        <label className="muted">
          <span>Limit</span>
          <input
            className="tv-topstrip__input"
            value={String(limit)}
            onChange={(e) => setLimit(parseInt(e.target.value || "50", 10) || 50)}
            style={{ width: "6rem" }}
          />
        </label>
        <label className="muted">
          <span>Status</span>
          <select className="tv-topstrip__select" value={status} onChange={(e) => setStatus(e.target.value as any)}>
            <option value="all">All</option>
            <option value="pending">pending</option>
            <option value="sending">sending</option>
            <option value="sent">sent</option>
            <option value="failed">failed</option>
          </select>
        </label>
        <label className="muted">
          <span>event_key</span>
          <input className="tv-topstrip__input" value={eventKey} onChange={(e) => setEventKey(e.target.value)} style={{ width: "10rem" }} />
        </label>
        <label className="muted">
          <span>exchange</span>
          <input className="tv-topstrip__input" value={exchange} onChange={(e) => setExchange(e.target.value)} style={{ width: "7rem" }} />
        </label>
        <label className="muted">
          <span>segment</span>
          <input className="tv-topstrip__input" value={segment} onChange={(e) => setSegment(e.target.value)} style={{ width: "7rem" }} />
        </label>
        <label className="muted">
          <span>symbol</span>
          <input className="tv-topstrip__input" value={symbol} onChange={(e) => setSymbol(e.target.value)} style={{ width: "7rem" }} />
        </label>
        <label className="muted" style={{ flex: 1, minWidth: "14rem" }}>
          <span>Ara</span>
          <input
            className="tv-topstrip__input"
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder="title/body/channel/error"
          />
        </label>
        <button type="button" className="theme-toggle" onClick={() => void refresh()} disabled={busy}>
          {busy ? "Yükleniyor…" : "Refresh"}
        </button>
      </div>

      {err ? <p className="err">{err}</p> : null}

      <div style={{ marginTop: "0.75rem", overflowX: "auto" }}>
        <table className="mono" style={{ width: "100%", fontSize: "0.72rem", borderCollapse: "collapse" }}>
          <thead>
            <tr>
              <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>created</th>
              <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>status</th>
              <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>severity</th>
              <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>event_key</th>
              <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>exchange</th>
              <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>segment</th>
              <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>symbol</th>
              <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>channels</th>
              <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>title</th>
              <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>body</th>
              <th style={{ textAlign: "left", padding: "0.15rem 0.2rem" }}>last_error</th>
            </tr>
          </thead>
          <tbody>
            {filtered.map((r) => (
              <tr key={r.id}>
                <td style={{ padding: "0.12rem 0.2rem", whiteSpace: "nowrap" }}>{asIso(r.created_at)}</td>
                <td style={{ padding: "0.12rem 0.2rem", whiteSpace: "nowrap" }}>{r.status}</td>
                <td style={{ padding: "0.12rem 0.2rem", whiteSpace: "nowrap" }}>{r.severity}</td>
                <td style={{ padding: "0.12rem 0.2rem", whiteSpace: "nowrap" }}>{r.event_key ?? ""}</td>
                <td style={{ padding: "0.12rem 0.2rem", whiteSpace: "nowrap" }}>{r.exchange ?? ""}</td>
                <td style={{ padding: "0.12rem 0.2rem", whiteSpace: "nowrap" }}>{r.segment ?? ""}</td>
                <td style={{ padding: "0.12rem 0.2rem", whiteSpace: "nowrap" }}>{r.symbol ?? ""}</td>
                <td style={{ padding: "0.12rem 0.2rem", whiteSpace: "nowrap" }}>{(r.channels ?? []).join(", ")}</td>
                <td style={{ padding: "0.12rem 0.2rem" }}>{r.title}</td>
                <td style={{ padding: "0.12rem 0.2rem" }}>{r.body}</td>
                <td style={{ padding: "0.12rem 0.2rem" }}>{r.last_error ?? ""}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

