import { useEffect, useRef, useState } from "react";
import { Search, X } from "lucide-react";
import { Button, Input, NativeSelect } from "@open-relay/ui";
import { useDebouncedValue } from "../../../lib/useDebouncedValue";
import {
  type DuplicateFilter,
  type SortOrder,
  type StatusGroup,
  type SubmissionFilters,
  CLEARED_FILTERS,
  STATUS_GROUPS,
  hasActiveFilters,
} from "../../../lib/submissions/filters";

export interface FilterChange {
  patch: Partial<SubmissionFilters>;
  /** Replace the history entry instead of pushing one (search keystrokes). */
  replace?: boolean;
}

interface Props {
  filters: SubmissionFilters;
  onChange: (change: FilterChange) => void;
  /** `undefined` when the reader lacks `forms:read`. */
  forms: { id: number; label: string }[] | undefined;
  /** `undefined` when the reader lacks `reps:read`. */
  reps: { id: number; name: string }[] | undefined;
}

export function SubmissionsToolbar({ filters, onChange, forms, reps }: Props) {
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <SearchBox value={filters.q} onChange={onChange} />
        <NativeSelect
          aria-label="Sort"
          className="w-auto"
          value={filters.sort}
          onChange={(e) => onChange({ patch: { sort: e.target.value as SortOrder } })}
        >
          <option value="newest">Newest first</option>
          <option value="oldest">Oldest first</option>
        </NativeSelect>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        {forms ? (
          <NativeSelect
            aria-label="Form"
            className="w-auto max-w-[14rem]"
            value={filters.formId ?? ""}
            onChange={(e) =>
              onChange({ patch: { formId: e.target.value ? Number(e.target.value) : null } })
            }
          >
            <option value="">All forms</option>
            {forms.map((f) => (
              <option key={f.id} value={f.id}>
                {f.label}
              </option>
            ))}
            {/* A linked form the list doesn't know (deleted, or not yet loaded). */}
            {filters.formId != null && !forms.some((f) => f.id === filters.formId) && (
              <option value={filters.formId}>Form #{filters.formId}</option>
            )}
          </NativeSelect>
        ) : (
          filters.formId != null && (
            <FilterChip
              label={`Form #${filters.formId}`}
              onClear={() => onChange({ patch: { formId: null } })}
            />
          )
        )}

        <NativeSelect
          aria-label="Delivery status"
          className="w-auto"
          value={filters.status ?? ""}
          onChange={(e) =>
            onChange({ patch: { status: (e.target.value || null) as StatusGroup | null } })
          }
        >
          <option value="">Any delivery status</option>
          {(Object.keys(STATUS_GROUPS) as StatusGroup[]).map((key) => (
            <option key={key} value={key}>
              {STATUS_GROUPS[key].label}
            </option>
          ))}
        </NativeSelect>

        {reps && (reps.length > 0 || filters.repId != null) ? (
          <NativeSelect
            aria-label="Sales rep"
            className="w-auto max-w-[12rem]"
            value={filters.repId ?? ""}
            onChange={(e) =>
              onChange({ patch: { repId: e.target.value ? Number(e.target.value) : null } })
            }
          >
            <option value="">Any rep</option>
            {reps.map((r) => (
              <option key={r.id} value={r.id}>
                {r.name}
              </option>
            ))}
            {filters.repId != null && !reps.some((r) => r.id === filters.repId) && (
              <option value={filters.repId}>Rep #{filters.repId}</option>
            )}
          </NativeSelect>
        ) : (
          filters.repId != null && (
            <FilterChip
              label={`Rep #${filters.repId}`}
              onClear={() => onChange({ patch: { repId: null } })}
            />
          )
        )}

        <NativeSelect
          aria-label="Duplicates"
          className="w-auto"
          value={filters.duplicate ?? ""}
          onChange={(e) =>
            onChange({
              patch: { duplicate: (e.target.value || null) as DuplicateFilter | null },
            })
          }
        >
          <option value="">Include duplicates</option>
          <option value="exclude">Hide duplicates</option>
          <option value="only">Only duplicates</option>
        </NativeSelect>

        <div className="flex items-center gap-1.5 text-sm text-muted-foreground">
          <Input
            type="date"
            aria-label="Received from"
            className="w-auto"
            value={filters.from ?? ""}
            max={filters.to ?? undefined}
            onChange={(e) => onChange({ patch: { from: e.target.value || null } })}
          />
          <span aria-hidden>–</span>
          <Input
            type="date"
            aria-label="Received to"
            className="w-auto"
            value={filters.to ?? ""}
            min={filters.from ?? undefined}
            onChange={(e) => onChange({ patch: { to: e.target.value || null } })}
          />
        </div>

        {hasActiveFilters(filters) && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => onChange({ patch: CLEARED_FILTERS })}
          >
            <X className="h-4 w-4" />
            Clear filters
          </Button>
        )}
      </div>
    </div>
  );
}

/**
 * The search input keeps its own text and pushes a debounced value to the
 * URL. `pushed` is the last value either side agreed on, which is what stops
 * the two effects from echoing each other: a keystroke only reaches the URL
 * once it differs from `pushed`, and a URL change (Clear filters, the back
 * button) only overwrites the text once *it* differs.
 */
function SearchBox({
  value,
  onChange,
}: {
  value: string;
  onChange: (change: FilterChange) => void;
}) {
  const [text, setText] = useState(value);
  const debounced = useDebouncedValue(text);
  const pushed = useRef(value);

  useEffect(() => {
    if (debounced.trim() !== pushed.current.trim()) {
      pushed.current = debounced;
      onChange({ patch: { q: debounced }, replace: true });
    }
  }, [debounced, onChange]);

  useEffect(() => {
    if (value.trim() !== pushed.current.trim()) {
      pushed.current = value;
      setText(value);
    }
  }, [value]);

  return (
    <div className="relative min-w-[16rem] flex-1">
      <Search
        aria-hidden
        className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
      />
      <Input
        type="search"
        aria-label="Search submissions"
        placeholder="Search name, email, company, any field, or #id…"
        className="pl-8 pr-8 [&::-webkit-search-cancel-button]:hidden"
        value={text}
        maxLength={200}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape" && text) {
            e.preventDefault();
            setText("");
          }
        }}
      />
      {text && (
        <button
          type="button"
          aria-label="Clear search"
          className="absolute right-2 top-1/2 -translate-y-1/2 rounded-sm p-0.5 text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          onClick={() => setText("")}
        >
          <X className="h-4 w-4" />
        </button>
      )}
    </div>
  );
}

function FilterChip({ label, onClear }: { label: string; onClear: () => void }) {
  return (
    <span className="inline-flex h-9 items-center gap-1 rounded-md border border-input bg-muted/40 pl-3 pr-1.5 text-sm">
      {label}
      <button
        type="button"
        aria-label={`Remove ${label} filter`}
        className="rounded-sm p-0.5 text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        onClick={onClear}
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </span>
  );
}
