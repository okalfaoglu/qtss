import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type { BlotterEntry, BlotterFeed } from "../lib/types";

// Tag pill colors. Keeping this as a small registry instead of an
// inline ternary so adding a new entry kind in the wire format is a
// one-line change here.
const KIND_BADGE: Record<BlotterEntry["kind"], string> = {
  order: "bg-sky-500/15 text-sky-300 border-sky-500/30",
  fill: "bg-emerald-500/15 text-emerald-300 border-emerald-500/30",
};

function fmtNum(v: string | null | undefined): string {
  return v ?? "—";
}

function fmtTime(iso: string): string {
  // Trim sub-second precision so the table stays readable; full
  // timestamp is in the title attribute for hover.
  return iso.replace("T", " ").replace(/\.\d+Z$/, "Z");
}

function Row({ entry }: { entry: BlotterEntry }) {
  const isOrder = entry.kind === "order";
  return (
    <tr className="border-t border-zinc-800/60">
      <td className="px-3 py-2">
        <span
          className={`inline-block rounded border px-2 py-0.5 text-xs uppercase ${KIND_BADGE[entry.kind]}`}
        >
          {entry.kind}
        </span>
      </td>
      <td className="px-3 py-2 font-mono text-xs text-zinc-400" title={entry.at}>
        {fmtTime(entry.at)}
      </td>
      <td className="px-3 py-2 text-zinc-200">
        {entry.venue}
        <span className="text-zinc-500"> / {entry.segment}</span>
      </td>
      <td className="px-3 py-2 font-mono text-zinc-100">{entry.symbol}</td>
      <td className="px-3 py-2 text-zinc-300">
        {isOrder ? entry.side : <span className="text-zinc-500">—</span>}
      </td>
      <td className="px-3 py-2 text-zinc-300">
        {isOrder ? entry.order_type : <span className="text-zinc-500">—</span>}
      </td>
      <td className="px-3 py-2 text-right font-mono text-zinc-100">
        {fmtNum(entry.quantity)}
      </td>
      <td className="px-3 py-2 text-right font-mono text-zinc-100">
        {fmtNum(entry.price)}
      </td>
      <td className="px-3 py-2 text-zinc-300">
        {isOrder ? entry.status : entry.fee ? `fee ${entry.fee} ${entry.fee_asset ?? ""}` : "—"}
      </td>
    </tr>
  );
}

export function Blotter() {
  const query = useQuery({
    queryKey: ["v2", "blotter"],
    queryFn: () => apiFetch<BlotterFeed>("/v2/blotter"),
    refetchInterval: 5_000,
  });

  if (query.isLoading) {
    return <div className="text-sm text-zinc-400">Loading blotter…</div>;
  }
  if (query.isError) {
    return (
      <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
        Failed to load blotter: {(query.error as Error).message}
      </div>
    );
  }
  const feed = query.data!;

  return (
    <div className="space-y-4">
      <div className="flex items-baseline justify-between">
        <div className="text-xs uppercase tracking-wide text-zinc-500">
          Order blotter — {feed.entries.length} entries
        </div>
        <div className="text-xs text-zinc-500">Generated at {feed.generated_at}</div>
      </div>

      <div className="overflow-hidden rounded-lg border border-zinc-800 bg-zinc-900/60">
        {feed.entries.length === 0 ? (
          <div className="px-4 py-6 text-sm text-zinc-500">No orders or fills yet.</div>
        ) : (
          <table className="w-full text-sm">
            <thead className="bg-zinc-900/80 text-xs uppercase text-zinc-500">
              <tr>
                <th className="px-3 py-2 text-left">Kind</th>
                <th className="px-3 py-2 text-left">When</th>
                <th className="px-3 py-2 text-left">Venue</th>
                <th className="px-3 py-2 text-left">Symbol</th>
                <th className="px-3 py-2 text-left">Side</th>
                <th className="px-3 py-2 text-left">Type</th>
                <th className="px-3 py-2 text-right">Qty</th>
                <th className="px-3 py-2 text-right">Price</th>
                <th className="px-3 py-2 text-left">Status / Fee</th>
              </tr>
            </thead>
            <tbody>
              {feed.entries.map((e, i) => (
                <Row key={i} entry={e} />
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
