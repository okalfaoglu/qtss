import type { TFunction } from "i18next";
import type { ChannelSixRejectJson } from "../api/client";

export function channelSixRejectMessage(t: TFunction, reject: ChannelSixRejectJson | undefined): string {
  if (!reject) return t("app.channelReject.missing");
  switch (reject.code) {
    case "insufficient_pivots":
      return t("app.channelReject.insufficientPivots", {
        have: String(reject.have_pivots ?? "?"),
        need: String(reject.need_pivots ?? 6),
      });
    case "pivot_alternation":
      return t("app.channelReject.pivotAlternation");
    case "bar_ratio_upper":
      return t("app.channelReject.barRatioUpper");
    case "bar_ratio_lower":
      return t("app.channelReject.barRatioLower");
    case "inspect_upper":
      return t("app.channelReject.inspectUpper");
    case "inspect_lower":
      return t("app.channelReject.inspectLower");
    case "pattern_not_allowed":
      return t("app.channelReject.patternNotAllowed");
    case "overlap_ignored":
      return t("app.channelReject.overlapIgnored");
    case "duplicate_pivot_window":
      return t("app.channelReject.duplicatePivotWindow");
    case "last_pivot_direction":
      return t("app.channelReject.lastPivotDirection");
    case "size_filter":
      return t("app.channelReject.sizeFilter");
    case "entry_not_in_channel":
      return t("app.channelReject.entryNotInChannel");
    default:
      return reject.code;
  }
}
