import { Button } from "@open-relay/ui";
import { api } from "./api";

export function App() {
  return (
    <main className="min-h-screen bg-background text-foreground p-8 flex flex-col gap-4 items-start">
      <h1 className="text-2xl font-semibold">OpenRelay Admin</h1>
      <p className="text-sm opacity-70">API base: {api.baseUrl}</p>
      <Button onClick={() => api.client.GET("/healthz" as never)}>Ping /healthz</Button>
    </main>
  );
}
