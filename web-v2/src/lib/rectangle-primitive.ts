/**
 * Rectangle Drawing Primitive for lightweight-charts v5.
 *
 * Draws a filled rectangle between two time points and two price levels.
 * Moves with zoom/scroll like native chart elements.
 */
import type {
  Time,
  ISeriesPrimitive,
  SeriesAttachedParameter,
  SeriesType,
  IPrimitivePaneView,
  IPrimitivePaneRenderer,
} from "lightweight-charts";
import type { CanvasRenderingTarget2D } from "fancy-canvas";

export interface RectangleOptions {
  /** Start time (left edge) */
  time1: Time;
  /** End time (right edge) */
  time2: Time;
  /** Top price */
  priceTop: number;
  /** Bottom price */
  priceBottom: number;
  /** Fill color with alpha, e.g. "#3b82f630" */
  fillColor: string;
  /** Border color, e.g. "#3b82f6" */
  borderColor: string;
  /** Border width (0 = no border) */
  borderWidth?: number;
  /** Optional label text */
  label?: string;
  /** Label color */
  labelColor?: string;
  /** Label font size */
  labelSize?: number;
}

class RectangleRenderer implements IPrimitivePaneRenderer {
  private _x1: number;
  private _x2: number;
  private _y1: number;
  private _y2: number;
  private _opts: RectangleOptions;

  constructor(
    x1: number,
    x2: number,
    y1: number,
    y2: number,
    opts: RectangleOptions
  ) {
    this._x1 = x1;
    this._x2 = x2;
    this._y1 = y1;
    this._y2 = y2;
    this._opts = opts;
  }

  draw(target: CanvasRenderingTarget2D): void {
    target.useBitmapCoordinateSpace((scope) => {
      const ctx = scope.context;
      const ratio = scope.horizontalPixelRatio;
      const vRatio = scope.verticalPixelRatio;

      const x = Math.min(this._x1, this._x2) * ratio;
      const w = Math.abs(this._x2 - this._x1) * ratio;
      const y = Math.min(this._y1, this._y2) * vRatio;
      const h = Math.abs(this._y2 - this._y1) * vRatio;

      // Fill
      ctx.fillStyle = this._opts.fillColor;
      ctx.fillRect(x, y, w, h);

      // Border
      const bw = this._opts.borderWidth ?? 1;
      if (bw > 0) {
        ctx.strokeStyle = this._opts.borderColor;
        ctx.lineWidth = bw * ratio;
        ctx.strokeRect(x, y, w, h);
      }

      // Label
      if (this._opts.label) {
        const fontSize = (this._opts.labelSize ?? 11) * ratio;
        ctx.font = `${fontSize}px sans-serif`;
        ctx.fillStyle = this._opts.labelColor ?? this._opts.borderColor;
        ctx.textAlign = "left";
        ctx.textBaseline = "top";
        ctx.fillText(this._opts.label, x + 4 * ratio, y + 3 * vRatio);
      }
    });
  }
}

class RectanglePaneView implements IPrimitivePaneView {
  private _renderer: RectangleRenderer | null = null;

  update(
    x1: number | null,
    x2: number | null,
    y1: number | null,
    y2: number | null,
    opts: RectangleOptions
  ): void {
    if (x1 == null || x2 == null || y1 == null || y2 == null) {
      this._renderer = null;
      return;
    }
    this._renderer = new RectangleRenderer(x1, x2, y1, y2, opts);
  }

  zOrder(): "bottom" {
    return "bottom";
  }

  renderer(): IPrimitivePaneRenderer | null {
    return this._renderer;
  }
}

export class RectanglePrimitive
  implements ISeriesPrimitive<Time>
{
  private _opts: RectangleOptions;
  private _paneView = new RectanglePaneView();
  private _paneViews: IPrimitivePaneView[] = [this._paneView];
  private _attached: SeriesAttachedParameter<Time, SeriesType> | null = null;

  constructor(opts: RectangleOptions) {
    this._opts = opts;
  }

  attached(param: SeriesAttachedParameter<Time, SeriesType>): void {
    this._attached = param;
  }

  detached(): void {
    this._attached = null;
  }

  updateAllViews(): void {
    if (!this._attached) return;
    const series = this._attached.series;
    const chart = this._attached.chart;
    const timeScale = chart.timeScale();

    const x1 = timeScale.timeToCoordinate(this._opts.time1);
    const x2 = timeScale.timeToCoordinate(this._opts.time2);
    const y1 = series.priceToCoordinate(this._opts.priceTop);
    const y2 = series.priceToCoordinate(this._opts.priceBottom);

    this._paneView.update(
      x1 as number | null,
      x2 as number | null,
      y1 as number | null,
      y2 as number | null,
      this._opts
    );
  }

  paneViews(): readonly IPrimitivePaneView[] {
    return this._paneViews;
  }
}
