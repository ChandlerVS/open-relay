import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@open-relay/ui";
import { useAuth } from "../../lib/auth/useAuth";

export function DashboardPage() {
  const { user } = useAuth();
  const name = user?.display_name?.trim() || user?.email || "there";

  return (
    <div className="space-y-6 max-w-4xl">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Welcome, {name}.</h1>
        <p className="text-sm text-muted-foreground">
          OpenRelay is in skeleton mode — domain features will land here next.
        </p>
      </div>
      <div className="grid gap-4 md:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>Forms</CardTitle>
            <CardDescription>
              Define the schemas and renderers your hosts will embed.
            </CardDescription>
          </CardHeader>
          <CardContent className="text-sm text-muted-foreground">Coming soon.</CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Backends</CardTitle>
            <CardDescription>
              Wire submissions to GoHighLevel, OpenRelay's own store, and more.
            </CardDescription>
          </CardHeader>
          <CardContent className="text-sm text-muted-foreground">Coming soon.</CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Submissions</CardTitle>
            <CardDescription>Inspect and replay delivery for each submission.</CardDescription>
          </CardHeader>
          <CardContent className="text-sm text-muted-foreground">Coming soon.</CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Users</CardTitle>
            <CardDescription>Invite teammates and manage SSO providers.</CardDescription>
          </CardHeader>
          <CardContent className="text-sm text-muted-foreground">Coming soon.</CardContent>
        </Card>
      </div>
    </div>
  );
}
