import { Link } from "react-router-dom";
import {
  FileText,
  Inbox,
  Users as UsersIcon,
  Server,
  type LucideIcon,
} from "lucide-react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
  Skeleton,
  Alert,
  AlertTitle,
  AlertDescription,
} from "@open-relay/ui";
import { useAuth } from "../../lib/auth/useAuth";
import { useDashboardOverview } from "../../lib/dashboard/useDashboard";

function formatDate(value: string) {
  return new Date(value).toLocaleString();
}

type Totals = {
  users: number;
  forms: number;
  submissions: number;
  backends: number;
};

const STAT_CARDS: { key: keyof Totals; label: string; icon: LucideIcon }[] = [
  { key: "forms", label: "Forms", icon: FileText },
  { key: "submissions", label: "Submissions", icon: Inbox },
  { key: "users", label: "Users", icon: UsersIcon },
  { key: "backends", label: "Backends", icon: Server },
];

// Friendly labels for the raw delivery-status slugs the worker writes.
const DELIVERY_STATUS_LABELS: Record<string, string> = {
  pending: "Pending",
  in_progress: "In progress",
  succeeded: "Succeeded",
  permanent_failure: "Failed",
  exhausted: "Exhausted",
};

export function DashboardPage() {
  const { user } = useAuth();
  const name = user?.display_name?.trim() || user?.email || "there";
  const { data, isLoading, isError, error } = useDashboardOverview();

  return (
    <div className="space-y-6 max-w-5xl">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">
          Welcome, {name}.
        </h1>
        <p className="text-sm text-muted-foreground">
          An overview of your forms, submissions, and delivery activity.
        </p>
      </div>

      {isError ? (
        <Alert variant="destructive">
          <AlertTitle>Couldn't load the dashboard</AlertTitle>
          <AlertDescription>
            {error instanceof Error ? error.message : "Unexpected error."}
          </AlertDescription>
        </Alert>
      ) : null}

      {/* Stat cards */}
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        {STAT_CARDS.map(({ key, label, icon: Icon }) => (
          <Card key={key}>
            <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
              <CardTitle className="text-sm font-medium text-muted-foreground">
                {label}
              </CardTitle>
              <Icon className="h-4 w-4 text-muted-foreground" />
            </CardHeader>
            <CardContent>
              {isLoading ? (
                <Skeleton className="h-8 w-16" />
              ) : (
                <div className="text-3xl font-semibold tabular-nums">
                  {data?.totals[key] ?? 0}
                </div>
              )}
            </CardContent>
          </Card>
        ))}
      </div>

      <div className="grid gap-4 md:grid-cols-2">
        {/* Delivery status breakdown */}
        <Card>
          <CardHeader>
            <CardTitle>Delivery status</CardTitle>
            <CardDescription>
              Per-backend delivery state across all submissions.
            </CardDescription>
          </CardHeader>
          <CardContent>
            {isLoading ? (
              <div className="space-y-2">
                <Skeleton className="h-6 w-full" />
                <Skeleton className="h-6 w-full" />
              </div>
            ) : data && data.delivery_status.length > 0 ? (
              <ul className="space-y-2">
                {data.delivery_status.map((s) => (
                  <li
                    key={s.status}
                    className="flex items-center justify-between text-sm"
                  >
                    <span>{DELIVERY_STATUS_LABELS[s.status] ?? s.status}</span>
                    <span className="font-medium tabular-nums">{s.count}</span>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="text-sm text-muted-foreground">No deliveries yet.</p>
            )}
          </CardContent>
        </Card>

        {/* Top forms by submission volume */}
        <Card>
          <CardHeader>
            <CardTitle>Top forms</CardTitle>
            <CardDescription>Forms ranked by submission count.</CardDescription>
          </CardHeader>
          <CardContent>
            {isLoading ? (
              <div className="space-y-2">
                <Skeleton className="h-6 w-full" />
                <Skeleton className="h-6 w-full" />
              </div>
            ) : data && data.top_forms.length > 0 ? (
              <ul className="space-y-2">
                {data.top_forms.map((f) => (
                  <li
                    key={f.form_id}
                    className="flex items-center justify-between text-sm"
                  >
                    <Link
                      to={`/submissions?form_id=${f.form_id}`}
                      className="truncate hover:underline"
                    >
                      {f.form_name}
                    </Link>
                    <span className="font-medium tabular-nums">{f.count}</span>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="text-sm text-muted-foreground">No forms yet.</p>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Recent submissions — only present when the caller has submissions:read */}
      {data?.recent_submissions != null ? (
        <Card>
          <CardHeader>
            <CardTitle>Recent submissions</CardTitle>
            <CardDescription>
              The latest activity across all forms.
            </CardDescription>
          </CardHeader>
          <CardContent className="px-0">
            {data.recent_submissions.length === 0 ? (
              <p className="px-6 text-sm text-muted-foreground">
                No submissions yet.
              </p>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Submitted</TableHead>
                    <TableHead>Name</TableHead>
                    <TableHead>Email</TableHead>
                    <TableHead>Form</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {data.recent_submissions.map((s) => (
                    <TableRow key={s.id}>
                      <TableCell>{formatDate(s.created_at)}</TableCell>
                      <TableCell>{s.name ?? "—"}</TableCell>
                      <TableCell>{s.email ?? "—"}</TableCell>
                      <TableCell>
                        <Link
                          to={`/submissions?form_id=${s.form_id}`}
                          className="hover:underline"
                        >
                          {s.form_name ?? `Form #${s.form_id}`}
                        </Link>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </CardContent>
        </Card>
      ) : null}
    </div>
  );
}
