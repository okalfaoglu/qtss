/**
 * Polygon Drawing Primitive for lightweight-charts v5.
 *
 * Fills an arbitrary polygon (N vertices in chart time/price space) and
 * optionally strokes the border. Used for harmonic XABCD shapes where
 * two triangles (XAB + BCD) are filled to visualise the pattern body
 * the way classical charting software draws it.
 *
 * Mirrors RectanglePrimitive: attaches to a series, converts each
 * vertex to pixel coordinates on every frame, renders at the "bottom"
 * zOrder so candles stay on top.
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

export interface PolygonVertex {
  time: Time;
  price: number;
}

export interface PolygonOptions {
  vertices: PolygonVertex[];
  fillColor: string;
  borderColor: string;
  borderWidth?: number;
}

class PolygonRenderer implements IPrimitivePaneRenderer {
  private _xs: number[];
  private _ys: number[];
  private _opts: PolygonOptions;

  constructor(xs: number[], ys: number[], opts: PolygonOptions) {
    this._xs = xs;
    this._ys = ys;
    this._opts = opts;
  }

  draw(target: CanvasRenderingTarget2D): void {
    if (this._xs.length < 3) return;
    target.useBitmapCoordinateSpace((scope) => {
      const ctx = scope.context;
      const hr = scope.horizontalPixelRatio;
      const vr = scope.verticalPixelRatio;

      ctx.beginPath();
      ctx.moveTo(this._xs[0] * hr, this._ys[0] * vr);
      for (let i = 1; i < this._xs.length; i++) {
        ctx.lineTo(this._xs[i] * hr, this._ys[i] * vr);
      }
      ctx.closePath();

      ctx.fillStyle = this._opts.fillColor;
      ctx.fill();

      const bw = this._opts.borderWidth ?? 0;
      if (bw > 0) {
        ctx.strokeStyle = this._opts.borderColor;
        ctx.lineWidth = bw * hr;
        ctx.stroke();
      }
    });
  }
}

class PolygonPaneView implements IPrimitivePaneView {
  private _renderer: PolygonRenderer | null = null;

  update(xs: (number | null)[], ys: (number | null)[], opts: PolygonOptions): void {
    if (xs.some((v) => v == null) || ys.some((v) => v == null)) {
      this._renderer = null;
      return;
    }
    this._renderer = new PolygonRenderer(
      xs as number[],
      ys as number[],
      opts,
    );
  }

  zOrder(): "bottom" {
    return "bottom";
  }

  renderer(): IPrimitivePaneRenderer | null {
    return this._renderer;
  }
}

export class PolygonPrimitive implements ISeriesPrimitive<Time> {
  private _opts: PolygonOptions;
  private _paneView = new PolygonPaneView();
  private _paneViews: IPrimitivePaneView[] = [this._paneView];
  private _attached: SeriesAttachedParameter<Time, SeriesType> | null = null;

  constructor(opts: PolygonOptions) {
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

    const xs = this._opts.vertices.map(
      (v) => timeScale.timeToCoordinate(v.time) as number | null,
    );
    const ys = this._opts.vertices.map(
      (v) => series.priceToCoordinate(v.price) as number | null,
    );

    this._paneView.update(xs, ys, this._opts);
  }

  paneViews(): readonly IPrimitivePaneView[] {
    return this._paneViews;
  }
}
