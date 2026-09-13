import { Outlet } from "react-router-dom";
import { Sidebar } from "./Sidebar";
import { Topbar } from "./Topbar";

export function AdminShell() {
  return (
    <div className="min-h-screen flex bg-background text-foreground">
      <Sidebar />
      <div className="flex-1 flex flex-col min-w-0">
        <Topbar />
        {/* Deliberately not a scroll container: the document scrolls, which
            is what lets the form builder pin its side panels with
            `position: sticky`. An `overflow-auto` here would become the
            nearest scrollport — one that never actually scrolls, since this
            shell is `min-h-screen` rather than `h-screen` — and silently
            neuter them. */}
        <main className="flex-1 p-6">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
