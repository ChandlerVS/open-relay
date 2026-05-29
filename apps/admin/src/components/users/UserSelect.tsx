import { Check, ChevronDown, Loader2 } from "lucide-react";
import {
  Button,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  cn,
} from "@open-relay/ui";
import { useUserSelectList } from "../../lib/users/useUsers";

export interface UserSelectProps {
  value: number | null;
  onChange: (id: number | null) => void;
  placeholder?: string;
  disabled?: boolean;
  /** When true, the dropdown offers a "no user" option that clears the value. */
  allowClear?: boolean;
  className?: string;
}

/**
 * Controlled dropdown that loads its options from `GET /users/select-list`.
 * Pulls the label out of the cached list so the trigger stays correct even
 * when the consumer only stores the id.
 */
export function UserSelect({
  value,
  onChange,
  placeholder = "Select a user…",
  disabled,
  allowClear = false,
  className,
}: UserSelectProps) {
  const { data, isLoading, isError } = useUserSelectList();
  const selected = value != null ? data?.find((o) => o.id === value) : undefined;

  const triggerLabel = (() => {
    if (isLoading) return "Loading users…";
    if (isError) return "Couldn't load users";
    if (selected) return selected.label;
    if (value != null && !selected) return `User #${value}`;
    return placeholder;
  })();

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="outline"
          disabled={disabled || isLoading || isError}
          className={cn(
            "justify-between font-normal",
            !selected && "text-muted-foreground",
            className,
          )}
        >
          <span className="truncate text-left">{triggerLabel}</span>
          {isLoading ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <ChevronDown className="h-4 w-4 opacity-60" />
          )}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="start"
        className="min-w-[var(--radix-dropdown-menu-trigger-width)] max-h-72 overflow-y-auto"
      >
        {allowClear && (
          <DropdownMenuItem onSelect={() => onChange(null)}>
            <Check
              className={cn("h-4 w-4", value == null ? "opacity-100" : "opacity-0")}
            />
            <span className="text-muted-foreground">No user</span>
          </DropdownMenuItem>
        )}
        {data?.length === 0 && (
          <div className="px-2 py-1.5 text-sm text-muted-foreground">No users.</div>
        )}
        {data?.map((opt) => (
          <DropdownMenuItem key={opt.id} onSelect={() => onChange(opt.id)}>
            <Check
              className={cn("h-4 w-4", value === opt.id ? "opacity-100" : "opacity-0")}
            />
            <span className="truncate">{opt.label}</span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
