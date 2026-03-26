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
  remove: () => void;
  resize: (width: number, height: number) => void;
  timeScale: () => { fitContent: () => void };
};

type SeriesHandle = {
  setData: (data: CandleDatum[]) => void;
};

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

        const chart = library.createChart(container, {
          width: Math.max(container.clientWidth, 320),
          height: Math.max(container.clientHeight, 260),
          layout: {
            background: { type: library.ColorType.Solid, color: "transparent" },
            textColor: "#8f9098",
          },
          grid: {
            vertLines: { color: "rgba(38, 39, 43, 0.28)" },
            horzLines: { color: "rgba(38, 39, 43, 0.46)" },
          },
          rightPriceScale: {
            borderColor: "#26272b",
          },
          timeScale: {
            borderColor: "#26272b",
            timeVisible: true,
            secondsVisible: true,
          },
          crosshair: {
            vertLine: {
              color: "rgba(255,255,255,0.08)",
              labelBackgroundColor: "#18181b",
            },
            horzLine: {
              color: "rgba(255,255,255,0.08)",
              labelBackgroundColor: "#18181b",
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
          upColor: "#42cc4e",
          downColor: "#d85b5b",
          borderVisible: true,
          borderUpColor: "#42cc4e",
          borderDownColor: "#d85b5b",
          wickUpColor: "#42cc4e",
          wickDownColor: "#d85b5b",
          priceLineVisible: false,
          lastValueVisible: false,
        });

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
      <div className="grid grid-cols-3 gap-[10px] border-b border-[#26272b] px-[20px] py-[14px] text-[13px] font-medium text-[#a7a7ae]">
        <div className="rounded-[6px] border border-[#26272b] bg-[#111114] px-[12px] py-[10px]">
          <p className="uppercase tracking-[0.18em] text-[10px] text-[#6f6f76]">Last</p>
          <p className="mt-[6px] text-[18px] font-bold leading-none text-white">
            {formatMaybePrice(lastPrice)}
          </p>
        </div>
        <div className="rounded-[6px] border border-[#26272b] bg-[#111114] px-[12px] py-[10px]">
          <p className="uppercase tracking-[0.18em] text-[10px] text-[#6f6f76]">Mid</p>
          <p className="mt-[6px] text-[18px] font-bold leading-none text-white">
            {formatMaybePrice(midPrice)}
          </p>
        </div>
        <div className="rounded-[6px] border border-[#26272b] bg-[#111114] px-[12px] py-[10px]">
          <p className="uppercase tracking-[0.18em] text-[10px] text-[#6f6f76]">Spread</p>
          <p className="mt-[6px] text-[18px] font-bold leading-none text-white">
            {formatMaybePrice(spread)}
          </p>
        </div>
      </div>

      <div className="relative min-h-0 flex-1">
        <div className="h-full w-full px-[10px] py-[12px]">
          <div className="h-full w-full" ref={containerRef} />
        </div>

        {candles.length === 0 ? (
          <div className="absolute inset-0 flex items-center justify-center px-[36px] text-center text-[16px] leading-[1.25] text-[#8a8a92]">
            Waiting for market trades to draw 10s candles for {marketName}.
          </div>
        ) : null}

        {chartError ? (
          <div className="absolute inset-0 flex items-center justify-center bg-[rgba(20,20,22,0.84)] px-[36px] text-center text-[16px] leading-[1.25] text-[#b8b8bc]">
            Candlestick rendering is unavailable in this environment.
          </div>
        ) : null}
      </div>
    </div>
  );
}
