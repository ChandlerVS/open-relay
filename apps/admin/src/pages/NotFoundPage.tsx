import { Link } from "react-router-dom";
import { Button } from "@open-relay/ui";

export function NotFoundPage() {
  return (
    <main className="min-h-screen grid place-items-center bg-background text-foreground p-6">
      <div className="text-center space-y-3">
        <h1 className="text-3xl font-semibold tracking-tight">Not found</h1>
        <p className="text-sm text-muted-foreground">
          The page you're looking for doesn't exist.
        </p>
        <Button asChild>
          <Link to="/">Back to dashboard</Link>
        </Button>
      </div>
    </main>
  );
}
