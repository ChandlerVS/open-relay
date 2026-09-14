import type { operations } from "@open-relay/api-client";

/**
 * The submissions page keeps every piece of view state in the URL, so a
 * filtered view survives a reload and can be pasted to a colleague. This
 * module is the only place that knows the param names.
 *
 * `form_id` predates the rest and is linked to from the dashboard — keep it.
 */

export type ListApiQuery = NonNullable<
  operations["list_submissions"]["parameters"]["query"]
>;
export type ExportApiQuery = NonNullable<
  operations["export_submissions"]["parameters"]["query"]
>;

export type StatusGroup = "failed" | "queued" | "delivered";
export type DuplicateFilter = "only" | "exclude";
export type SortOrder = "newest" | "oldest";

/**
 * Admin-facing groupings of the five delivery statuses. The server filter is
 * "has *any* delivery in the set", so a submission that failed on one backend
 * and succeeded on another shows under both Failed and Delivered.
 */
export const STATUS_GROUPS: Record<StatusGroup, { label: string; statuses: string[] }> = {
  failed: { label: "Failed", statuses: ["permanent_failure", "exhausted"] },
  queued: { label: "Queued", statuses: ["pending", "in_progress"] },
  delivered: { label: "Delivered", statuses: ["succeeded"] },
};

export const PAGE_SIZES = [25, 50, 100] as const;
const DEFAULT_PAGE_SIZE = 25;

export interface SubmissionFilters {
  q: string;
  formId: number | null;
  status: StatusGroup | null;
  repId: number | null;
  duplicate: DuplicateFilter | null;
  /** Local calendar dates, `YYYY-MM-DD`, both inclusive as the admin sees them. */
  from: string | null;
  to: string | null;
  sort: SortOrder;
  /** 1-based. */
  page: number;
  perPage: number;
}

const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;

function positiveInt(raw: string | null): number | null {
  if (raw == null || !/^\d+$/.test(raw)) return null;
  const n = Number(raw);
  return Number.isSafeInteger(n) && n > 0 ? n : null;
}

function oneOf<T extends string>(raw: string | null, allowed: readonly T[]): T | null {
  return raw != null && (allowed as readonly string[]).includes(raw) ? (raw as T) : null;
}

export function readFilters(params: URLSearchParams): SubmissionFilters {
  const perPage = positiveInt(params.get("per_page"));
  const from = params.get("from");
  const to = params.get("to");
  return {
    q: params.get("q") ?? "",
    formId: positiveInt(params.get("form_id")),
    status: oneOf(params.get("status"), ["failed", "queued", "delivered"] as const),
    repId: positiveInt(params.get("rep_id")),
    duplicate: oneOf(params.get("duplicate"), ["only", "exclude"] as const),
    from: from && DATE_RE.test(from) ? from : null,
    to: to && DATE_RE.test(to) ? to : null,
    sort: oneOf(params.get("sort"), ["newest", "oldest"] as const) ?? "newest",
    page: positiveInt(params.get("page")) ?? 1,
    perPage:
      perPage != null && (PAGE_SIZES as readonly number[]).includes(perPage)
        ? perPage
        : DEFAULT_PAGE_SIZE,
  };
}

/**
 * Apply `patch` to the filters in `params`, returning a new `URLSearchParams`.
 * Changing anything but `page` resets to page 1 — page 4 of the old result
 * set means nothing in the new one. Params this module doesn't own (e.g. the
 * open `submission`) are carried over untouched.
 */
export function writeFilters(
  params: URLSearchParams,
  patch: Partial<SubmissionFilters>,
): URLSearchParams {
  const next = { ...readFilters(params), ...patch };
  const onlyPage = Object.keys(patch).every((k) => k === "page");
  if (!onlyPage) next.page = patch.page ?? 1;

  const out = new URLSearchParams(params);
  const set = (key: string, value: string | number | null, fallback?: string | number) => {
    if (value == null || value === "" || value === fallback) out.delete(key);
    else out.set(key, String(value));
  };
  set("q", next.q.trim());
  set("form_id", next.formId);
  set("status", next.status);
  set("rep_id", next.repId);
  set("duplicate", next.duplicate);
  set("from", next.from);
  set("to", next.to);
  set("sort", next.sort, "newest");
  set("page", next.page, 1);
  set("per_page", next.perPage, DEFAULT_PAGE_SIZE);
  return out;
}

/** The filters that narrow the result set — paging and sort don't count. */
export function hasActiveFilters(f: SubmissionFilters): boolean {
  return Boolean(
    f.q.trim() || f.formId || f.status || f.repId || f.duplicate || f.from || f.to,
  );
}

export const CLEARED_FILTERS: Partial<SubmissionFilters> = {
  q: "",
  formId: null,
  status: null,
  repId: null,
  duplicate: null,
  from: null,
  to: null,
};

/** Midnight at the start of a local calendar day, `days` later. */
function localMidnight(ymd: string, days = 0): Date {
  const [y, m, d] = ymd.split("-").map(Number) as [number, number, number];
  // The Date constructor normalises day overflow and DST, which adding
  // 86_400_000 ms would not.
  return new Date(y, m - 1, d + days);
}

/**
 * Lower the URL filters into the export API's query. Dates are resolved in
 * the *browser's* timezone: the server takes instants, `from` inclusive and
 * `to` exclusive, so "to 2026-09-13" becomes midnight starting the 14th.
 */
export function toExportQuery(f: SubmissionFilters): ExportApiQuery {
  const query: ExportApiQuery = {};
  const q = f.q.trim();
  if (q) query.q = q;
  if (f.formId != null) query.form_id = f.formId;
  if (f.status) query.status = STATUS_GROUPS[f.status].statuses.join(",");
  if (f.repId != null) query.sales_rep_id = f.repId;
  if (f.duplicate) query.duplicate = f.duplicate === "only";
  if (f.from) query.from = localMidnight(f.from).toISOString();
  if (f.to) query.to = localMidnight(f.to, 1).toISOString();
  if (f.sort !== "newest") query.sort = f.sort;
  return query;
}

export function toListQuery(f: SubmissionFilters): ListApiQuery {
  return {
    ...toExportQuery(f),
    limit: f.perPage,
    offset: (f.page - 1) * f.perPage,
  };
}
