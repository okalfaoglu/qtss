import { describe, expect, it } from "vitest";
import { normalizeSymbolForBinanceKlinesApi } from "./binanceKlinesSymbol";

describe("normalizeSymbolForBinanceKlinesApi", () => {
  it("strips TradingView perpetual suffix", () => {
    expect(normalizeSymbolForBinanceKlinesApi("TRADOORUSDT.P")).toBe("TRADOORUSDT");
    expect(normalizeSymbolForBinanceKlinesApi("btcusdt.p")).toBe("BTCUSDT");
  });

  it("strips exchange prefix", () => {
    expect(normalizeSymbolForBinanceKlinesApi("BINANCE:ETHUSDT")).toBe("ETHUSDT");
  });

  it("strips .PERP suffix", () => {
    expect(normalizeSymbolForBinanceKlinesApi("FOO.PERP")).toBe("FOO");
  });
});
