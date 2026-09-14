import * as React from "react";
import { cn } from "../lib/cn";

export interface CheckboxProps
  extends Omit<React.InputHTMLAttributes<HTMLInputElement>, "type"> {
  /** Draws the "some selected" dash. DOM-only state, so it's set via a ref. */
  indeterminate?: boolean;
}

/** A native checkbox, themed via `accent-color`. */
export const Checkbox = React.forwardRef<HTMLInputElement, CheckboxProps>(
  ({ className, indeterminate = false, ...props }, ref) => {
    const inner = React.useRef<HTMLInputElement>(null);
    React.useImperativeHandle(ref, () => inner.current as HTMLInputElement);
    React.useEffect(() => {
      if (inner.current) inner.current.indeterminate = indeterminate;
    }, [indeterminate]);
    return (
      <input
        ref={inner}
        type="checkbox"
        className={cn(
          "h-4 w-4 cursor-pointer rounded border-input align-middle accent-primary",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
          "disabled:cursor-not-allowed disabled:opacity-50",
          className,
        )}
        {...props}
      />
    );
  },
);
Checkbox.displayName = "Checkbox";
