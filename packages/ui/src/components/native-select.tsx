import * as React from "react";
import { cn } from "../lib/cn";

export type NativeSelectProps = React.SelectHTMLAttributes<HTMLSelectElement>;

/**
 * A native `<select>` styled to match `Input`. There is no Radix Select here,
 * and for short option lists the native control is the more accessible one.
 */
export const NativeSelect = React.forwardRef<HTMLSelectElement, NativeSelectProps>(
  ({ className, ...props }, ref) => (
    <select
      ref={ref}
      className={cn(
        "flex h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm shadow-sm transition-colors",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-0",
        "disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    />
  ),
);
NativeSelect.displayName = "NativeSelect";
