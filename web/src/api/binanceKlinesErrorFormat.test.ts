import { describe, expect, it } from "vitest";
import {
  describeBinanceKlinesHttpFailureCore,
  isLikelyHtmlErrorPayload,
} from "./binanceKlinesErrorFormat";

describe("isLikelyHtmlErrorPayload", () => {
  it("detects html 404", () => {
    expect(isLikelyHtmlErrorPayload("<html><head><title>404 Not Found</title>")).toBe(true);
    expect(isLikelyHtmlErrorPayload("<!DOCTYPE html><html>")).toBe(true);
  });

  it("rejects json", () => {
    expect(isLikelyHtmlErrorPayload('{"code":-1121,"msg":"x"}')).toBe(false);
  });
});

describe("describeBinanceKlinesHttpFailureCore", () => {
  it("replaces html with guidance", () => {
    const msg = describeBinanceKlinesHttpFailureCore(
      404,
      "<html><title>404 Not Found</title></html>",
      { requestUrl: "/__binance_fapi/fapi/v1/klines?symbol=X", segment: "futures" },
      false,
    );
    expect(msg).toContain("JSON değil");
    expect(msg).toContain("VITE_BINANCE_FAPI_API_BASE");
    expect(msg).not.toContain("<html>");
  });

  it("includes request url when debug", () => {
    const msg = describeBinanceKlinesHttpFailureCore(
      404,
      "<html></html>",
      { requestUrl: "https://example.com/x" },
      true,
    );
    expect(msg).toContain("https://example.com/x");
  });

  it("formats binance json -1121", () => {
    const msg = describeBinanceKlinesHttpFailureCore(
      400,
      '{"code":-1121,"msg":"Invalid symbol."}',
      { requestUrl: "https://fapi.binance.com/fapi/v1/klines" },
      false,
    );
    expect(msg).toContain("-1121");
    expect(msg).toContain("USDT-M");
  });
});
