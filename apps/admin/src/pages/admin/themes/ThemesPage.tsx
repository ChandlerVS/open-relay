import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { MoreHorizontal, Plus } from "lucide-react";
import {
  Alert,
  AlertDescription,
  Button,
  ConfirmDialog,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  Skeleton,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@open-relay/ui";
import { QueryErrorAlert } from "../../../lib/api/QueryErrorAlert";
import { RequirePermission } from "../../../lib/auth/RequirePermission";
import { usePermissions } from "../../../lib/auth/usePermissions";
import {
  useThemesList,
  type ThemeDto,
  type ThemeSettings,
} from "../../../lib/formThemes/useThemes";
import {
  useDeleteTheme,
  useUpdateTheme,
} from "../../../lib/formThemes/useThemeMutations";

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

export function ThemesPage() {
  const { has } = usePermissions();
  const navigate = useNavigate();
  const { data, isLoading, isError, error, refetch } = useThemesList();
  const update = useUpdateTheme();
  const deleteMutation = useDeleteTheme();

  const [deleting, setDeleting] = useState<ThemeDto | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  const canWrite = has("themes:write");
  const canDelete = has("themes:delete");
  const defaultTheme = data?.items.find((t) => t.is_default);

  return (
    <div className="space-y-6 max-w-5xl">
      <div className="flex items-center justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Themes</h1>
          <p className="text-sm text-muted-foreground">
            Reusable looks for embedded forms: colours, corner radius, font and
            spacing. Forms without a theme of their own use the default
            {defaultTheme ? (
              <>
                {" "}
                (<span className="font-medium text-foreground">{defaultTheme.name}</span>)
              </>
            ) : (
              ", or the built-in look while none is set"
            )}
            .
          </p>
        </div>
        <RequirePermission perm="themes:write">
          <Button onClick={() => navigate("/themes/new")}>
            <Plus className="h-4 w-4" />
            New theme
          </Button>
        </RequirePermission>
      </div>

      <QueryErrorAlert
        error={isError ? error : null}
        title="Couldn't load themes"
        onRetry={() => refetch()}
      />
      {actionError && (
        <Alert variant="destructive">
          <AlertDescription>{actionError}</AlertDescription>
        </Alert>
      )}

      <div className="border border-border rounded-lg bg-background">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Name</TableHead>
              <TableHead>Colours</TableHead>
              <TableHead>Used by</TableHead>
              <TableHead>Updated</TableHead>
              <TableHead className="w-10 text-right pr-3">
                <span className="sr-only">Actions</span>
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {isLoading ? (
              Array.from({ length: 3 }).map((_, i) => (
                <TableRow key={`s-${i}`}>
                  <TableCell>
                    <Skeleton className="h-4 w-40" />
                  </TableCell>
                  <TableCell>
                    <Skeleton className="h-4 w-24" />
                  </TableCell>
                  <TableCell>
                    <Skeleton className="h-4 w-16" />
                  </TableCell>
                  <TableCell>
                    <Skeleton className="h-4 w-24" />
                  </TableCell>
                  <TableCell />
                </TableRow>
              ))
            ) : data && data.items.length > 0 ? (
              data.items.map((t) => (
                <TableRow key={t.id}>
                  <TableCell>
                    <div className="flex items-center gap-2">
                      <Link to={`/themes/${t.id}`} className="font-medium hover:underline">
                        {t.name}
                      </Link>
                      {t.is_default && (
                        <span className="rounded-full bg-muted px-2 py-0.5 text-xs font-medium text-muted-foreground">
                          Default
                        </span>
                      )}
                    </div>
                  </TableCell>
                  <TableCell>
                    <Swatches settings={t.settings} />
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {t.form_count === 0 ? "—" : plural(t.form_count, "form")}
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {new Date(t.updated_at).toLocaleDateString()}
                  </TableCell>
                  <TableCell className="text-right pr-2">
                    <RowMenu
                      canWrite={canWrite}
                      canDelete={canDelete}
                      isDefault={t.is_default}
                      onEdit={() => navigate(`/themes/${t.id}`)}
                      onMakeDefault={() => {
                        setActionError(null);
                        update.mutate(
                          { id: t.id, input: { is_default: true } },
                          { onError: (err) => setActionError(err.message) },
                        );
                      }}
                      onDelete={() => {
                        setDeleteError(null);
                        setDeleting(t);
                      }}
                    />
                  </TableCell>
                </TableRow>
              ))
            ) : (
              <TableRow>
                <TableCell
                  colSpan={5}
                  className="text-center py-10 text-sm text-muted-foreground"
                >
                  No themes yet, so every form uses the built-in look. Click "New
                  theme" to create one.
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>

      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(o) => {
          if (!o) {
            setDeleting(null);
            setDeleteError(null);
          }
        }}
        title="Delete theme?"
        description={
          <span>
            This permanently removes{" "}
            <span className="font-medium text-foreground">{deleting?.name}</span>.
            {deleting && deleting.form_count > 0 && (
              <span className="mt-2 block">
                {plural(deleting.form_count, "form")} using it will switch to the
                default theme.
              </span>
            )}
            {deleting?.is_default && (
              <span className="mt-2 block">
                It is the default theme, so forms without a theme of their own
                will go back to the built-in look.
              </span>
            )}
            {deleteError && (
              <span className="mt-2 block text-destructive">{deleteError}</span>
            )}
          </span>
        }
        confirmLabel="Delete"
        pending={deleteMutation.isPending}
        onConfirm={() => {
          if (!deleting) return;
          deleteMutation.mutate(
            { id: deleting.id },
            {
              onSuccess: () => {
                setDeleting(null);
                setDeleteError(null);
              },
              onError: (err) => setDeleteError(err.message),
            },
          );
        }}
      />
    </div>
  );
}

const SWATCH_KEYS = ["accent", "background", "text", "input_border"] as const;

/** The colours a theme sets, at a glance. Values are server-validated hex. */
function Swatches({ settings }: { settings: ThemeSettings }) {
  const set = SWATCH_KEYS.flatMap((key) => {
    const value = settings.colors?.[key];
    return value ? [{ key, value }] : [];
  });
  if (set.length === 0) {
    return <span className="text-xs text-muted-foreground">Built-in</span>;
  }
  return (
    <div className="flex items-center gap-1">
      {set.map(({ key, value }) => (
        <span
          key={key}
          title={`${key.replace("_", " ")}: ${value}`}
          className="h-4 w-4 rounded-full border border-border"
          style={{ background: value }}
        />
      ))}
    </div>
  );
}

interface RowMenuProps {
  canWrite: boolean;
  canDelete: boolean;
  isDefault: boolean;
  onEdit: () => void;
  onMakeDefault: () => void;
  onDelete: () => void;
}

function RowMenu({
  canWrite,
  canDelete,
  isDefault,
  onEdit,
  onMakeDefault,
  onDelete,
}: RowMenuProps) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="sm" aria-label="Row actions">
          <MoreHorizontal className="h-4 w-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        {/* Opening the editor is fine read-only; it just won't save. */}
        <DropdownMenuItem onSelect={onEdit}>{canWrite ? "Edit" : "View"}</DropdownMenuItem>
        {canWrite && !isDefault && (
          <DropdownMenuItem onSelect={onMakeDefault}>Make default</DropdownMenuItem>
        )}
        {canDelete && <DropdownMenuSeparator />}
        {canDelete && (
          <DropdownMenuItem
            onSelect={onDelete}
            className="text-destructive focus:text-destructive"
          >
            Delete
          </DropdownMenuItem>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
