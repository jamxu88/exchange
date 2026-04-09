"use client";

import { useEffect, useRef, useState } from "react";
import { formatMaybePrice } from "@/components/trade/trade-store";
import type { MarketTrade } from "@/components/trade/trade-types";
import type { CandlestickData, UTCTimestamp } from "lightweight-charts";

const candleFormatter = new Intl.NumberFormat("en-US", {
  style: "currency",
  currency: "USD",
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
});

const CANDLE_BUCKET_MS = 10_000;
const MAX_CANDLES = 120;

type CandleDatum = CandlestickData<UTCTimestamp>;

type TradeCandlestickChartProps = {
  lastPrice: number | null;
  marketName: string;
  midPrice: number | null;
  spread: number | null;
  trades: MarketTrade[];
};

type ChartHandle = {
  applyOptions: (options: Record<string, unknown>) => void;
  remove: () => void;
  resize: (width: number, height: number) => void;
  timeScale: () => { fitContent: () => void };
};

type SeriesHandle = {
  applyOptions: (options: Record<string, unknown>) => void;
  setData: (data: CandleDatum[]) => void;
};

type ChartPalette = {
  gridSoft: string;
  gridStrong: string;
  crosshair: string;
  panel: string;
  border: string;
  textMuted: string;
  textQuiet: string;
  textFaint: string;
  textPrimary: string;
  positive: string;
  negative: string;
  overlay: string;
};

function readThemeValue(name: string, fallback: string) {
  if (typeof document === "undefined") {
    return fallback;
  }

  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value.length > 0 ? value : fallback;
}

function readChartPalette(): ChartPalette {
  return {
    gridSoft: readThemeValue("--trade-chart-grid-soft", "rgba(38, 39, 43, 0.28)"),
    gridStrong: readThemeValue("--trade-chart-grid-strong", "rgba(38, 39, 43, 0.46)"),
    crosshair: readThemeValue("--trade-chart-crosshair", "rgba(255, 255, 255, 0.08)"),
    panel: readThemeValue("--trade-panel-elevated", "#18181b"),
    border: readThemeValue("--trade-border", "#26272b"),
    textMuted: readThemeValue("--trade-text-muted", "#8a8a92"),
    textQuiet: readThemeValue("--trade-text-quiet", "#a7a7ae"),
    textFaint: readThemeValue("--trade-text-faint", "#6f6f76"),
    textPrimary: readThemeValue("--trade-text-primary", "#f5f5f5"),
    positive: readThemeValue("--trade-positive", "#42cc4e"),
    negative: readThemeValue("--trade-negative", "#d85b5b"),
    overlay: readThemeValue("--overlay", "rgba(20, 20, 22, 0.84)"),
  };
}

function applyChartTheme(chart: ChartHandle, series: SeriesHandle) {
  const palette = readChartPalette();
  chart.applyOptions({
    layout: {
      textColor: palette.textMuted,
    },
    grid: {
      vertLines: { color: palette.gridSoft },
      horzLines: { color: palette.gridStrong },
    },
    rightPriceScale: {
      borderColor: palette.border,
    },
    timeScale: {
      borderColor: palette.border,
    },
    crosshair: {
      vertLine: {
        color: palette.crosshair,
        labelBackgroundColor: palette.panel,
      },
      horzLine: {
        color: palette.crosshair,
        labelBackgroundColor: palette.panel,
      },
    },
  });
  series.applyOptions({
    upColor: palette.positive,
    downColor: palette.negative,
    borderUpColor: palette.positive,
    borderDownColor: palette.negative,
    wickUpColor: palette.positive,
    wickDownColor: palette.negative,
  });
}

export function buildCandlestickData(trades: MarketTrade[]) {
  const candles: CandleDatum[] = [];
  const sortedTrades = [...trades].sort(
    (left, right) => Date.parse(left.occurredAt) - Date.parse(right.occurredAt),
  );

  for (const trade of sortedTrades) {
    const timestamp = Date.parse(trade.occurredAt);
    if (!Number.isFinite(timestamp)) {
      continue;
    }

    const bucketStart = Math.floor(timestamp / CANDLE_BUCKET_MS) * CANDLE_BUCKET_MS;
    const time = Math.floor(bucketStart / 1000) as UTCTimestamp;
    const previous = candles[candles.length - 1];

    if (!previous || previous.time !== time) {
      candles.push({
        time,
        open: trade.price,
        high: trade.price,
        low: trade.price,
        close: trade.price,
      });
      continue;
    }

    previous.high = Math.max(previous.high, trade.price);
    previous.low = Math.min(previous.low, trade.price);
    previous.close = trade.price;
  }

  return candles.slice(-MAX_CANDLES);
}

export function TradeCandlestickChart({
  lastPrice,
  marketName,
  midPrice,
  spread,
  trades,
}: TradeCandlestickChartProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<ChartHandle | null>(null);
  const seriesRef = useRef<SeriesHandle | null>(null);
  const candlesRef = useRef<CandleDatum[]>([]);
  const [chartError, setChartError] = useState(false);
  const candles = buildCandlestickData(trades);
  candlesRef.current = candles;

  useEffect(() => {
    let mounted = true;
    let resizeObserver: ResizeObserver | null = null;
    let themeObserver: MutationObserver | null = null;

    async function loadChart() {
      const container = containerRef.current;
      if (!container) {
        return;
      }

      try {
        const library = await import("lightweight-charts");
        if (!mounted || !containerRef.current) {
          return;
        }

        const palette = readChartPalette();

        const chart = library.createChart(container, {
          width: Math.max(container.clientWidth, 320),
          height: Math.max(container.clientHeight, 260),
          layout: {
            background: { type: library.ColorType.Solid, color: "transparent" },
            textColor: palette.textMuted,
          },
          grid: {
            vertLines: { color: palette.gridSoft },
            horzLines: { color: palette.gridStrong },
          },
          rightPriceScale: {
            borderColor: palette.border,
          },
          timeScale: {
            borderColor: palette.border,
            timeVisible: true,
            secondsVisible: true,
          },
          crosshair: {
            vertLine: {
              color: palette.crosshair,
              labelBackgroundColor: palette.panel,
            },
            horzLine: {
              color: palette.crosshair,
              labelBackgroundColor: palette.panel,
            },
          },
          localization: {
            priceFormatter: (value: number) => candleFormatter.format(value),
          },
          handleScroll: {
            mouseWheel: true,
            pressedMouseMove: true,
            horzTouchDrag: true,
            vertTouchDrag: false,
          },
          handleScale: {
            mouseWheel: true,
            pinch: true,
            axisPressedMouseMove: true,
          },
        }) as unknown as ChartHandle & {
          addSeries: (
            definition: unknown,
            options: Record<string, unknown>,
          ) => SeriesHandle;
        };

        const series = chart.addSeries(library.CandlestickSeries, {
          upColor: palette.positive,
          downColor: palette.negative,
          borderVisible: true,
          borderUpColor: palette.positive,
          borderDownColor: palette.negative,
          wickUpColor: palette.positive,
          wickDownColor: palette.negative,
          priceLineVisible: false,
          lastValueVisible: false,
        });

        applyChartTheme(chart, series);
        series.setData(candlesRef.current);
        chart.timeScale().fitContent();

        chartRef.current = chart;
        seriesRef.current = series;
        setChartError(false);

        if (typeof ResizeObserver !== "undefined") {
          resizeObserver = new ResizeObserver((entries) => {
            const entry = entries[0];
            if (!entry || !chartRef.current) {
              return;
            }

            chartRef.current.resize(entry.contentRect.width, entry.contentRect.height);
          });
          resizeObserver.observe(container);
        }

        if (typeof MutationObserver !== "undefined") {
          themeObserver = new MutationObserver(() => {
            if (!chartRef.current || !seriesRef.current) {
              return;
            }

            applyChartTheme(chartRef.current, seriesRef.current);
          });
          themeObserver.observe(document.documentElement, {
            attributes: true,
            attributeFilter: ["data-theme"],
          });
        }
      } catch {
        if (mounted) {
          setChartError(true);
        }
      }
    }

    void loadChart();

    return () => {
      mounted = false;
      resizeObserver?.disconnect();
      themeObserver?.disconnect();
      chartRef.current?.remove();
      chartRef.current = null;
      seriesRef.current = null;
    };
  }, []);

  useEffect(() => {
    if (!seriesRef.current || !chartRef.current) {
      return;
    }

    seriesRef.current.setData(candles);
    chartRef.current.timeScale().fitContent();
  }, [candles]);

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="candlestick-view">
      <div className="grid grid-cols-3 gap-[10px] border-b border-[var(--trade-border)] px-[20px] py-[14px] text-[13px] font-medium text-[var(--trade-text-quiet)]">
        <div className="rounded-[6px] border border-[var(--trade-border)] bg-[var(--trade-panel-soft)] px-[12px] py-[10px]">
          <p className="uppercase tracking-[0.18em] text-[10px] text-[var(--trade-text-faint)]">Last</p>
          <p className="mt-[6px] text-[18px] font-bold leading-none text-[var(--trade-text-primary)]">
            {formatMaybePrice(lastPrice)}
          </p>
        </div>
        <div className="rounded-[6px] border border-[var(--trade-border)] bg-[var(--trade-panel-soft)] px-[12px] py-[10px]">
          <p className="uppercase tracking-[0.18em] text-[10px] text-[var(--trade-text-faint)]">Mid</p>
          <p className="mt-[6px] text-[18px] font-bold leading-none text-[var(--trade-text-primary)]">
            {formatMaybePrice(midPrice)}
          </p>
        </div>
        <div className="rounded-[6px] border border-[var(--trade-border)] bg-[var(--trade-panel-soft)] px-[12px] py-[10px]">
          <p className="uppercase tracking-[0.18em] text-[10px] text-[var(--trade-text-faint)]">Spread</p>
          <p className="mt-[6px] text-[18px] font-bold leading-none text-[var(--trade-text-primary)]">
            {formatMaybePrice(spread)}
          </p>
        </div>
      </div>

      <div className="relative min-h-0 flex-1">
        <div className="h-full w-full px-[10px] py-[12px]">
          <div className="h-full w-full" ref={containerRef} />
        </div>

        {candles.length === 0 ? (
          <div className="absolute inset-0 flex items-center justify-center px-[36px] text-center text-[16px] leading-[1.25] text-[var(--trade-text-muted)]">
            Waiting for market trades to draw 10s candles for {marketName}.
          </div>
        ) : null}

        {chartError ? (
          <div className="absolute inset-0 flex items-center justify-center bg-[var(--overlay)] px-[36px] text-center text-[16px] leading-[1.25] text-[var(--trade-text-faint)]">
            Candlestick rendering is unavailable in this environment.
          </div>
        ) : null}
      </div>
    </div>
  );
}
