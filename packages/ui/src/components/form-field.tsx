import * as React from "react";
import { cn } from "../lib/cn";
import { Label } from "./label";

export interface FormFieldProps {
  id: string;
  label: React.ReactNode;
  error?: string;
  hint?: React.ReactNode;
  className?: string;
  children: React.ReactElement<{ id?: string; "aria-invalid"?: boolean; "aria-describedby"?: string }>;
}

/**
 * Layout helper: Label + control + inline error/hint. Wires `id`,
 * `aria-invalid`, and `aria-describedby` onto the child so the
 * caller doesn't have to repeat them.
 */
export function FormField({ id, label, error, hint, className, children }: FormFieldProps) {
  const messageId = error || hint ? `${id}-message` : undefined;
  const control = React.cloneElement(children, {
    id,
    "aria-invalid": Boolean(error),
    "aria-describedby": messageId,
  });
  return (
    <div className={cn("flex flex-col gap-2", className)}>
      <Label htmlFor={id}>{label}</Label>
      {control}
      {(error || hint) && (
        <p
          id={messageId}
          className={cn(
            "text-xs",
            error ? "text-destructive" : "text-muted-foreground",
          )}
        >
          {error ?? hint}
        </p>
      )}
    </div>
  );
}
