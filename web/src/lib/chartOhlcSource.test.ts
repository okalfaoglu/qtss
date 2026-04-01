import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { chartUsesBinanceRestForOhlc, persistChartOhlcMode, readChartOhlcMode } from "./chartOhlcSource";

describe("chartUsesBinanceRestForOhlc", () => {
  it("exchange mode always uses REST", () => {
    expect(chartUsesBinanceRestForOhlc("exchange", "x", "kraken", "spot")).toBe(true);
  });

  it("database mode never uses REST for chart source", () => {
    expect(chartUsesBinanceRestForOhlc("database", "x", "binance", "spot")).toBe(false);
  });

  it("auto without token uses REST", () => {
    expect(chartUsesBinanceRestForOhlc("auto", null, "binance", "spot")).toBe(true);
    expect(chartUsesBinanceRestForOhlc("auto", "   ", "binance", "spot")).toBe(true);
  });

  it("auto with token uses REST for binance spot and USDT-M futures", () => {
    expect(chartUsesBinanceRestForOhlc("auto", "jwt", "binance", "spot")).toBe(true);
    expect(chartUsesBinanceRestForOhlc("auto", "jwt", "Binance", "SPOT")).toBe(true);
    expect(chartUsesBinanceRestForOhlc("auto", "jwt", "binance", "futures")).toBe(true);
    expect(chartUsesBinanceRestForOhlc("auto", "jwt", "binance", "usdt_futures")).toBe(true);
  });

  it("auto with token uses DB path for non-Binance or unsupported segment", () => {
    expect(chartUsesBinanceRestForOhlc("auto", "jwt", "okx", "spot")).toBe(false);
    expect(chartUsesBinanceRestForOhlc("auto", "jwt", "binance", "margin")).toBe(false);
  });
});

describe("readChartOhlcMode / persistChartOhlcMode", () => {
  const mem: Record<string, string> = {};
  const ls: Storage = {
    get length() {
      return Object.keys(mem).length;
    },
    clear() {
      for (const k of Object.keys(mem)) delete mem[k];
    },
    getItem(key: string) {
      return mem[key] ?? null;
    },
    key(i: number) {
      return Object.keys(mem)[i] ?? null;
    },
    removeItem(key: string) {
      delete mem[key];
    },
    setItem(key: string, value: string) {
      mem[key] = value;
    },
  };

  beforeEach(() => {
    for (const k of Object.keys(mem)) delete mem[k];
    vi.stubGlobal("localStorage", ls);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("round-trips mode via localStorage", () => {
    persistChartOhlcMode("database");
    expect(readChartOhlcMode()).toBe("database");
    persistChartOhlcMode("exchange");
    expect(readChartOhlcMode()).toBe("exchange");
  });
});
