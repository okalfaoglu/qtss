import type { ChannelSixResponse, PatternMatchPayloadJson } from "../api/client";
import { isLiveRobotSignal, outcomePivotBarRange } from "../lib/channelSixLiveSignal";
import { formationLevelsForMatchRow } from "../lib/formationTradeLevelChart";

function matchRows(res: ChannelSixResponse): PatternMatchPayloadJson[] {
  if (res.pattern_matches?.length) return res.pattern_matches;
  if (res.outcome) {
    return [
      {
        outcome: res.outcome,
        pattern_name: res.pattern_name,
        pattern_drawing_batch: res.pattern_drawing_batch,
        formation_trade_levels: res.formation_trade_levels,
      },
    ];
  }
  return [];
}

function formatLevelPrice(n: number): string {
  const a = Math.abs(n);
  if (a >= 10_000) return n.toFixed(2);
  if (a >= 1) return n.toFixed(4);
  return n.toFixed(6);
}

/**
 * Geçmiş + canlı formasyon özeti: kod doğrulaması ve robot için `pivot_tail_skip === 0` satırını seçme.
 */
export function ChannelScanMatchesTable({ res }: { res: ChannelSixResponse }) {
  if (!res.matched) return null;
  const rows = matchRows(res);
  if (rows.length === 0) return null;

  return (
    <div style={{ marginTop: "0.75rem" }}>
      <p className="muted" style={{ fontSize: "0.8rem", marginBottom: "0.35rem" }}>
        Tespit edilen formasyonlar (geçmiş pencereler + en güncel).{" "}
        <strong>Robot / canlı sinyal</strong>: API <code>live_robot_match_index</code> (veya tabloda{" "}
        <strong>canlı</strong>) — <code>pivot_tail_skip=0</code>, <code>zigzag_level=0</code>.
      </p>
      {typeof res.live_robot_match_index === "number" ? (
        <p className="muted" style={{ fontSize: "0.75rem", marginBottom: "0.35rem" }}>
          <code>live_robot_match_index</code> = {res.live_robot_match_index} (0 tabanlı; yoksa canlı pencere eşleşmedi).
        </p>
      ) : null}
      <div style={{ overflowX: "auto" }}>
        <table className="mono" style={{ width: "100%", fontSize: "0.75rem", borderCollapse: "collapse" }}>
          <thead>
            <tr style={{ textAlign: "left", borderBottom: "1px solid var(--tv-border, #333)" }}>
              <th style={{ padding: "0.2rem 0.4rem" }}>#</th>
              <th style={{ padding: "0.2rem 0.4rem" }}>Desen</th>
              <th style={{ padding: "0.2rem 0.4rem" }}>id</th>
              <th style={{ padding: "0.2rem 0.4rem" }} title="0 = en güncel 6/5 pivot penceresi">skip</th>
              <th style={{ padding: "0.2rem 0.4rem" }}>zz lvl</th>
              <th style={{ padding: "0.2rem 0.4rem" }}>bar aralığı</th>
              <th style={{ padding: "0.2rem 0.4rem" }}>Enter</th>
              <th style={{ padding: "0.2rem 0.4rem" }}>SL</th>
              <th style={{ padding: "0.2rem 0.4rem" }}>TP</th>
              <th style={{ padding: "0.2rem 0.4rem" }}>robot</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((m, i) => {
              const o = m.outcome;
              const name = m.pattern_name ?? `id ${o.scan.pattern_type_id}`;
              const br = outcomePivotBarRange(o);
              const live = isLiveRobotSignal(o);
              const apiPick = res.live_robot_match_index;
              const robotRow = typeof apiPick === "number" && apiPick === i;
              const levels = formationLevelsForMatchRow(res, m, i);
              const tpCell =
                levels && levels.take_profits.length
                  ? levels.take_profits.map((tp) => `${tp.id}: ${formatLevelPrice(tp.price)}`).join("; ")
                  : "—";
              return (
                <tr
                  key={i}
                  style={{
                    borderBottom: "1px solid var(--tv-border, #222)",
                    background: robotRow ? "rgba(76, 175, 80, 0.12)" : undefined,
                  }}
                >
                  <td style={{ padding: "0.25rem 0.4rem" }}>{i + 1}</td>
                  <td style={{ padding: "0.25rem 0.4rem" }}>{name}</td>
                  <td style={{ padding: "0.25rem 0.4rem" }}>{o.scan.pattern_type_id}</td>
                  <td style={{ padding: "0.25rem 0.4rem" }}>{o.pivot_tail_skip ?? 0}</td>
                  <td style={{ padding: "0.25rem 0.4rem" }}>{o.zigzag_level ?? 0}</td>
                  <td style={{ padding: "0.25rem 0.4rem" }}>
                    {br ? `${br.min} … ${br.max}` : "—"}
                  </td>
                  <td style={{ padding: "0.25rem 0.4rem" }}>
                    {levels ? formatLevelPrice(levels.entry) : "—"}
                  </td>
                  <td style={{ padding: "0.25rem 0.4rem" }}>
                    {levels ? formatLevelPrice(levels.stop_loss) : "—"}
                  </td>
                  <td style={{ padding: "0.25rem 0.4rem", maxWidth: "14rem", wordBreak: "break-word" }}>
                    {tpCell}
                  </td>
                  <td style={{ padding: "0.25rem 0.4rem" }}>{live ? "canlı" : "—"}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      {rows.length === 1 && (rows[0].outcome.pivot_tail_skip ?? 0) > 0 ? (
        <p className="muted" style={{ fontSize: "0.75rem", marginTop: "0.35rem" }}>
          Tek eşleşme daha eski bir pencerede; en güncel pencere (skip=0) şu an geçerli formasyon üretmiyor olabilir.
        </p>
      ) : null}
    </div>
  );
}
