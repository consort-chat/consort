import { useColumnResize, type Bounds } from "../lib/useColumnResize";
import "./ColumnGrip.css";

/**
 * The edge of a resizable column, as something to take hold of.
 *
 * A separator rather than a button, because that is what it is, and focusable
 * so the arrows work. Placement is the container's: both sidebars want the same
 * grip and each knows where its own edge is.
 */
export function ColumnGrip({
  className,
  label,
  width,
  bounds,
  widens,
  onResize,
}: {
  /** Where to put it. The look is this component's, the position is not. */
  className: string;
  /** What it is for, said out loud. There is nothing in it to read. */
  label: string;
  /** How wide the column is now. */
  width: number;
  /** Asked afresh each gesture, because the maximum follows the window. */
  bounds: () => Bounds;
  /** Which way the pointer goes to widen the column. */
  widens: "left" | "right";
  /** Report a width the grip was dragged or nudged to. Already clamped. */
  onResize: (width: number) => void;
}) {
  const grip = useColumnResize({ width, onResize, widens, bounds });

  return (
    <div
      className={`grip ${className}`}
      role="separator"
      aria-label={label}
      tabIndex={0}
      {...grip}
    >
      {/* The lines, which are all there is to see. Nothing to announce. */}
      <span className="grip__lines" aria-hidden="true" />
    </div>
  );
}
