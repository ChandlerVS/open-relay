import { NavLink } from "react-router-dom";
import { FileText, Inbox, LayoutDashboard, Plug, Users } from "lucide-react";
import { cn } from "@open-relay/ui";

interface NavItem {
  to: string;
  label: string;
  icon: typeof FileText;
  disabled?: boolean;
}

const ITEMS: NavItem[] = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard },
  { to: "/forms", label: "Forms", icon: FileText, disabled: true },
  { to: "/backends", label: "Backends", icon: Plug, disabled: true },
  { to: "/submissions", label: "Submissions", icon: Inbox, disabled: true },
  { to: "/users", label: "Users", icon: Users },
];

export function Sidebar() {
  return (
    <aside className="hidden md:flex md:w-60 shrink-0 flex-col border-r border-border bg-background">
      <div className="h-14 flex items-center px-4 border-b border-border">
        <span className="font-semibold tracking-tight">OpenRelay</span>
      </div>
      <nav className="flex-1 p-2 space-y-1">
        {ITEMS.map(({ to, label, icon: Icon, disabled }) =>
          disabled ? (
            <span
              key={to}
              className="flex items-center gap-2 rounded-md px-3 py-2 text-sm text-muted-foreground cursor-not-allowed"
              title="Coming soon"
            >
              <Icon className="h-4 w-4" />
              <span className="flex-1">{label}</span>
              <span className="text-[10px] uppercase tracking-wider rounded bg-muted px-1.5 py-0.5">
                soon
              </span>
            </span>
          ) : (
            <NavLink
              key={to}
              to={to}
              end={to === "/"}
              className={({ isActive }) =>
                cn(
                  "flex items-center gap-2 rounded-md px-3 py-2 text-sm transition-colors",
                  isActive
                    ? "bg-accent text-accent-foreground"
                    : "text-foreground hover:bg-accent/60",
                )
              }
            >
              <Icon className="h-4 w-4" />
              <span>{label}</span>
            </NavLink>
          ),
        )}
      </nav>
      <div className="border-t border-border px-4 py-3 text-xs text-muted-foreground">
        skeleton build
      </div>
    </aside>
  );
}
