import { LogOut, User as UserIcon } from "lucide-react";
import { useNavigate } from "react-router-dom";
import {
  Button,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@open-relay/ui";
import { useAuth } from "../../lib/auth/useAuth";
import { ThemeToggle } from "../../lib/theme/ThemeToggle";

export function Topbar() {
  const { user, signOut } = useAuth();
  const navigate = useNavigate();

  const onSignOut = () => {
    signOut();
    navigate("/login", { replace: true });
  };

  const displayLabel = user?.display_name ?? user?.email ?? "Account";

  return (
    <header className="h-14 flex items-center justify-between gap-4 border-b border-border px-4 bg-background">
      <div className="text-sm font-medium text-muted-foreground">Admin</div>
      <div className="flex items-center gap-2">
        <ThemeToggle />
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="sm" className="gap-2 h-9">
              <UserIcon className="h-4 w-4" />
              <span className="max-w-[160px] truncate">{displayLabel}</span>
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="min-w-[200px]">
            <DropdownMenuLabel>
              Signed in as
              <div className="text-foreground font-normal truncate normal-case">{user?.email}</div>
            </DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={() => navigate("/profile")}>
              <UserIcon className="h-4 w-4" />
              <span>Profile</span>
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={onSignOut}>
              <LogOut className="h-4 w-4" />
              <span>Sign out</span>
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </header>
  );
}
