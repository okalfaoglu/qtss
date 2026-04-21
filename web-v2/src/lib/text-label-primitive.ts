/**
 * Fixed-visibility text label primitive for lightweight-charts v5.
 *
 * Unlike `createSeriesMarkers`, which auto-hides labels when bar
 * spacing gets tight on zoom-out, this primitive always renders — it's
 * the closest we can get to Pine's `label.new` in this runtime. Pairs
 * one label with one (time, price) anchor.
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

export interface TextLabelOptions {
  time: Time;
  price: number;
  text: string;
  color: string;
  /** "above" offsets up by `offsetPx`, "below" offsets down. */
  position: "above" | "below";
  fontSize?: number;
  offsetPx?: number;
  background?: string;
  paddingPx?: number;
}

class TextLabelRenderer implements IPrimitivePaneRenderer {
  constructor(
    private readonly x: number,
    private readonly y: number,
    private readonly opts: TextLabelOptions
  ) {}

  draw(target: CanvasRenderingTarget2D): void {
    target.useBitmapCoordinateSpace((scope) => {
      const ctx = scope.context;
      const hRatio = scope.horizontalPixelRatio;
      const vRatio = scope.verticalPixelRatio;

      const fontSize = (this.opts.fontSize ?? 10) * vRatio;
      const offset = (this.opts.offsetPx ?? 10) * vRatio;
      const padX = (this.opts.paddingPx ?? 3) * hRatio;
      const padY = (this.opts.paddingPx ?? 2) * vRatio;

      ctx.font = `${fontSize}px sans-serif`;
      const metrics = ctx.measureText(this.opts.text);
      const textW = metrics.width;
      const textH = fontSize;

      const cx = this.x * hRatio;
      const cy = this.y * vRatio;
      const yTop =
        this.opts.position === "above"
          ? cy - offset - textH - padY * 2
          : cy + offset;
      const xLeft = cx - textW / 2 - padX;
      const boxW = textW + padX * 2;
      const boxH = textH + padY * 2;

      if (this.opts.background) {
        ctx.fillStyle = this.opts.background;
        ctx.fillRect(xLeft, yTop, boxW, boxH);
      }
      ctx.fillStyle = this.opts.color;
      ctx.textAlign = "center";
      ctx.textBaseline = "top";
      ctx.fillText(this.opts.text, cx, yTop + padY);
    });
  }
}

class TextLabelPaneView implements IPrimitivePaneView {
  private _renderer: TextLabelRenderer | null = null;

  update(x: number | null, y: number | null, opts: TextLabelOptions): void {
    if (x == null || y == null) {
      this._renderer = null;
      return;
    }
    this._renderer = new TextLabelRenderer(x, y, opts);
  }

  zOrder(): "top" {
    return "top";
  }

  renderer(): IPrimitivePaneRenderer | null {
    return this._renderer;
  }
}

export class TextLabelPrimitive implements ISeriesPrimitive<Time> {
  private readonly opts: TextLabelOptions;
  private readonly view = new TextLabelPaneView();
  private readonly views: IPrimitivePaneView[] = [this.view];
  private attachedParam: SeriesAttachedParameter<Time, SeriesType> | null = null;

  constructor(opts: TextLabelOptions) {
    this.opts = opts;
  }

  attached(param: SeriesAttachedParameter<Time, SeriesType>): void {
    this.attachedParam = param;
  }

  detached(): void {
    this.attachedParam = null;
  }

  updateAllViews(): void {
    if (!this.attachedParam) return;
    const { chart, series } = this.attachedParam;
    const x = chart.timeScale().timeToCoordinate(this.opts.time);
    const y = series.priceToCoordinate(this.opts.price);
    this.view.update(x as number | null, y as number | null, this.opts);
  }

  paneViews(): readonly IPrimitivePaneView[] {
    return this.views;
  }
}
