import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { ArrowLeft, RotateCcw } from "lucide-react";
import {
  MAX_RADIUS,
  ShadowForm,
  isHexColor,
  isSafeFontFamily,
  type PublicFormDto,
} from "@open-relay/form-renderer";
import {
  Alert,
  AlertDescription,
  AlertTitle,
  Button,
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  FormField,
  Input,
  Skeleton,
  cn,
} from "@open-relay/ui";
import { api } from "../../../lib/api/client";
import { QueryErrorAlert } from "../../../lib/api/QueryErrorAlert";
import { PermissionNotice } from "../../../lib/auth/PermissionNotice";
import { usePermissions } from "../../../lib/auth/usePermissions";
import {
  useFormTheme,
  type ThemeDto,
  type ThemeSettings,
} from "../../../lib/formThemes/useThemes";
import {
  useCreateTheme,
  useUpdateTheme,
} from "../../../lib/formThemes/useThemeMutations";
import { SAMPLE_SCHEMA } from "./sampleSchema";
import {
  COLOR_GROUPS,
  DEFAULT_RADIUS,
  DENSITIES,
  FONT_PRESETS,
  FONT_SIZES,
  canonicalSettings,
  toPickerValue,
  withColor,
  withSetting,
} from "./settings";

const RADIO_ROW =
  "flex items-start gap-3 px-3 py-2 cursor-pointer hover:bg-accent/40 border-b border-border last:border-b-0";

export function ThemeEditorPage() {
  const { id } = useParams<{ id: string }>();
  const isNew = id === undefined;
  const themeId = isNew ? null : Number(id);
  const validId = themeId !== null && Number.isFinite(themeId);
  const { data, isLoading, isError, error, refetch } = useFormTheme(
    validId ? themeId : null,
  );

  if (!isNew && !validId) {
    return <p className="text-sm text-destructive">Invalid theme id.</p>;
  }
  if (!isNew && isLoading) {
    return (
      <div className="space-y-4 max-w-6xl">
        <Skeleton className="h-8 w-48" />
        <Skeleton className="h-96 w-full" />
      </div>
    );
  }
  if (!isNew && !data) {
    return (
      <div className="space-y-4 max-w-6xl">
        <BackLink />
        <QueryErrorAlert
          error={isError ? error : null}
          title="Couldn't load theme"
          onRetry={() => refetch()}
        />
      </div>
    );
  }
  // Keyed so navigating from /themes/new to the created theme's URL starts
  // from the saved row rather than carrying the draft state across.
  return <ThemeEditor key={data?.id ?? "new"} existing={data ?? null} />;
}

function BackLink() {
  return (
    <Link
      to="/themes"
      className="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground"
    >
      <ArrowLeft className="h-4 w-4" />
      Themes
    </Link>
  );
}

function ThemeEditor({ existing }: { existing: ThemeDto | null }) {
  const navigate = useNavigate();
  const canWrite = usePermissions().has("themes:write");
  const create = useCreateTheme();
  const update = useUpdateTheme();

  const [name, setName] = useState(existing?.name ?? "");
  const [isDefault, setIsDefault] = useState(existing?.is_default ?? false);
  const [settings, setSettings] = useState<ThemeSettings>(existing?.settings ?? {});
  // The embed's own light/dark switch. A theme is one palette, but any colour
  // it leaves unset still follows this, so it's worth seeing both.
  const [hostMode, setHostMode] = useState<"light" | "dark">("light");
  const [saveError, setSaveError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);

  const clean = useMemo(() => canonicalSettings(settings), [settings]);
  const settingsChanged =
    !existing ||
    JSON.stringify(clean) !== JSON.stringify(canonicalSettings(existing.settings));
  const nameChanged = name.trim() !== (existing?.name ?? "");
  const defaultChanged = isDefault !== (existing?.is_default ?? false);
  const dirty = !existing || settingsChanged || nameChanged || defaultChanged;
  const fontInvalid = clean.font_family != null && !isSafeFontFamily(clean.font_family);
  const pending = create.isPending || update.isPending;

  const previewSchema: PublicFormDto = useMemo(
    () => ({ ...SAMPLE_SCHEMA, theme: clean }),
    [clean],
  );

  const save = () => {
    if (!name.trim()) {
      setSaveError("Name is required.");
      return;
    }
    if (fontInvalid) {
      setSaveError("The font family has characters a CSS font list can't contain.");
      return;
    }
    setSaveError(null);
    if (!existing) {
      create.mutate(
        { name: name.trim(), is_default: isDefault, settings: clean },
        {
          onSuccess: (t) => navigate(`/themes/${t.id}`, { replace: true }),
          onError: (err) => setSaveError(err.message),
        },
      );
      return;
    }
    update.mutate(
      {
        id: existing.id,
        input: {
          name: nameChanged ? name.trim() : undefined,
          is_default: defaultChanged ? isDefault : undefined,
          settings: settingsChanged ? clean : undefined,
        },
      },
      {
        onSuccess: () => setSavedAt(Date.now()),
        onError: (err) => setSaveError(err.message),
      },
    );
  };

  const set = (next: (s: ThemeSettings) => ThemeSettings) => {
    setSavedAt(null);
    setSettings(next);
  };

  return (
    <div className="space-y-6 max-w-6xl">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div className="space-y-1">
          <BackLink />
          <h1 className="text-2xl font-semibold tracking-tight">
            {existing ? existing.name : "New theme"}
          </h1>
        </div>
        {canWrite && (
          <div className="flex items-center gap-3">
            {savedAt && !dirty && (
              <span className="text-sm text-muted-foreground">Saved.</span>
            )}
            <Button onClick={save} disabled={pending || !dirty}>
              {pending ? "Saving…" : existing ? "Save theme" : "Create theme"}
            </Button>
          </div>
        )}
      </div>

      {!canWrite && <PermissionNotice perm="themes:write" action="edit this theme" />}
      {saveError && (
        <Alert variant="destructive">
          <AlertTitle>Couldn't save theme</AlertTitle>
          <AlertDescription>{saveError}</AlertDescription>
        </Alert>
      )}

      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
        {/* A disabled fieldset makes every control inside read-only at once. */}
        <fieldset disabled={!canWrite} className="min-w-0 space-y-6">
          <Card>
            <CardHeader>
              <CardTitle className="text-base">Basics</CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              <FormField id="theme-name" label="Name">
                <Input
                  value={name}
                  maxLength={200}
                  onChange={(e) => {
                    setSavedAt(null);
                    setName(e.target.value);
                  }}
                />
              </FormField>
              <label className="flex items-start gap-2 text-sm">
                <input
                  type="checkbox"
                  className="mt-0.5 h-4 w-4 accent-primary"
                  checked={isDefault}
                  onChange={(e) => {
                    setSavedAt(null);
                    setIsDefault(e.target.checked);
                  }}
                />
                <span>
                  <span className="font-medium">Default theme</span>
                  <span className="block text-xs text-muted-foreground">
                    Forms without a theme of their own use this one. There is only
                    ever one default, so checking this replaces the current one.
                  </span>
                </span>
              </label>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="text-base">Colours</CardTitle>
              <p className="text-xs text-muted-foreground">
                A colour left unset keeps the built-in value, which follows the
                embed's light or dark mode.
              </p>
            </CardHeader>
            <CardContent className="space-y-5">
              {COLOR_GROUPS.map((group) => (
                <div key={group.title} className="space-y-2">
                  <h3 className="text-sm font-semibold">{group.title}</h3>
                  <div className="grid gap-3 sm:grid-cols-2">
                    {group.colors.map((c) => (
                      <ColorField
                        key={c.key}
                        id={`theme-color-${c.key}`}
                        label={c.label}
                        builtin={c.builtin}
                        value={settings.colors?.[c.key] ?? null}
                        onChange={(v) => set((s) => withColor(s, c.key, v))}
                      />
                    ))}
                  </div>
                </div>
              ))}
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="text-base">Shape &amp; type</CardTitle>
            </CardHeader>
            <CardContent className="space-y-5">
              <div className="space-y-2">
                <div className="flex items-center justify-between text-sm">
                  <label htmlFor="theme-radius" className="font-medium">
                    Corner radius
                  </label>
                  <span className="tabular-nums text-muted-foreground">
                    {settings.radius ?? DEFAULT_RADIUS}px
                    {settings.radius == null && " (built-in)"}
                  </span>
                </div>
                <div className="flex items-center gap-2">
                  <input
                    id="theme-radius"
                    type="range"
                    min={0}
                    max={MAX_RADIUS}
                    step={1}
                    value={settings.radius ?? DEFAULT_RADIUS}
                    onChange={(e) =>
                      set((s) => withSetting(s, "radius", Number(e.target.value)))
                    }
                    className="flex-1 accent-primary"
                  />
                  <ResetButton
                    label="Reset corner radius"
                    disabled={settings.radius == null}
                    onClick={() => set((s) => withSetting(s, "radius", undefined))}
                  />
                </div>
              </div>

              <div className="space-y-2">
                <FormField
                  id="theme-font"
                  label="Font family"
                  hint="A CSS font list. Only fonts the host page already loads will render; the rest fall through to the next name."
                  error={
                    fontInvalid
                      ? "Use letters, digits, spaces, commas, hyphens, underscores and balanced quotes only."
                      : undefined
                  }
                >
                  <Input
                    value={settings.font_family ?? ""}
                    placeholder="Built-in system font"
                    maxLength={200}
                    onChange={(e) =>
                      set((s) => withSetting(s, "font_family", e.target.value))
                    }
                  />
                </FormField>
                <div className="flex flex-wrap gap-2">
                  {FONT_PRESETS.map((p) => (
                    <Button
                      key={p.label}
                      type="button"
                      size="sm"
                      variant={(clean.font_family ?? "") === p.value ? "default" : "outline"}
                      onClick={() => set((s) => withSetting(s, "font_family", p.value))}
                    >
                      {p.label}
                    </Button>
                  ))}
                </div>
              </div>

              <div className="space-y-2">
                <h3 className="text-sm font-medium">Text size</h3>
                <RadioRows
                  name="theme-font-size"
                  value={settings.font_size ?? "medium"}
                  options={FONT_SIZES}
                  onChange={(v) =>
                    set((s) => withSetting(s, "font_size", v === "medium" ? undefined : v))
                  }
                />
              </div>

              <div className="space-y-2">
                <h3 className="text-sm font-medium">Spacing</h3>
                <RadioRows
                  name="theme-density"
                  value={settings.density ?? "comfortable"}
                  options={DENSITIES}
                  onChange={(v) =>
                    set((s) =>
                      withSetting(s, "density", v === "comfortable" ? undefined : v),
                    )
                  }
                />
              </div>
            </CardContent>
          </Card>
        </fieldset>

        <div className="min-w-0 lg:sticky lg:top-6 lg:self-start">
          <Card>
            <CardHeader className="flex flex-row items-center justify-between gap-2 space-y-0">
              <CardTitle className="text-base">Preview</CardTitle>
              <div className="inline-flex rounded-md border border-border p-0.5 text-xs">
                {(["light", "dark"] as const).map((mode) => (
                  <button
                    key={mode}
                    type="button"
                    onClick={() => setHostMode(mode)}
                    className={cn(
                      "rounded px-2 py-1 capitalize",
                      hostMode === mode
                        ? "bg-accent text-foreground"
                        : "text-muted-foreground hover:text-foreground",
                    )}
                  >
                    {mode} embed
                  </button>
                ))}
              </div>
            </CardHeader>
            <CardContent className="space-y-3">
              <div
                className={cn(
                  "rounded-md p-4",
                  hostMode === "dark" ? "bg-neutral-950" : "bg-white",
                )}
              >
                <ShadowForm
                  formId="theme-preview"
                  apiUrl={api.baseUrl}
                  schema={previewSchema}
                  previewMode
                  theme={hostMode}
                />
              </div>
              <p className="text-xs text-muted-foreground">
                A host page that sets the public <code>--or-color-*</code>,{" "}
                <code>--or-radius</code> or <code>--or-font</code> variables still
                overrides this theme.
              </p>
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  );
}

/**
 * A native picker plus a hex text box. The text box keeps its own draft so a
 * half-typed value (`#4f4`) doesn't get pushed into the theme — only a valid
 * hex, or an empty box (meaning "built-in"), is committed.
 */
function ColorField({
  id,
  label,
  builtin,
  value,
  onChange,
}: {
  id: string;
  label: string;
  builtin: string;
  value: string | null;
  onChange: (next: string | undefined) => void;
}) {
  const [draft, setDraft] = useState(value ?? "");
  // Follow changes made elsewhere — the picker, the reset button.
  useEffect(() => {
    setDraft((d) => (d.trim() === (value ?? "") ? d : (value ?? "")));
  }, [value]);
  const invalid = draft.trim() !== "" && !isHexColor(draft.trim());

  return (
    <div className="space-y-1">
      <label htmlFor={id} className="text-xs font-medium">
        {label}
      </label>
      <div className="flex items-center gap-2">
        <input
          type="color"
          aria-label={`${label} picker`}
          value={toPickerValue(value ?? builtin)}
          onChange={(e) => onChange(e.target.value)}
          className="h-8 w-10 shrink-0 cursor-pointer rounded border border-border bg-background p-0.5 disabled:cursor-not-allowed"
        />
        <Input
          id={id}
          value={draft}
          placeholder={builtin}
          aria-invalid={invalid || undefined}
          className={cn("h-8 font-mono text-xs", invalid && "border-destructive")}
          onChange={(e) => {
            const next = e.target.value;
            setDraft(next);
            const t = next.trim();
            if (t === "") onChange(undefined);
            else if (isHexColor(t)) onChange(t);
          }}
        />
        <ResetButton
          label={`Reset ${label}`}
          disabled={value == null}
          onClick={() => onChange(undefined)}
        />
      </div>
    </div>
  );
}

function ResetButton({
  label,
  disabled,
  onClick,
}: {
  label: string;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      aria-label={label}
      title="Use the built-in value"
      disabled={disabled}
      onClick={onClick}
    >
      <RotateCcw className="h-3.5 w-3.5" />
    </Button>
  );
}

function RadioRows<T extends string>({
  name,
  value,
  options,
  onChange,
}: {
  name: string;
  value: T;
  options: readonly { value: T; title: string; description: string }[];
  onChange: (next: T) => void;
}) {
  return (
    <div className="border border-border rounded-md">
      {options.map((o) => (
        <label key={o.value} className={RADIO_ROW}>
          <input
            type="radio"
            name={name}
            className="mt-1 h-4 w-4"
            checked={value === o.value}
            onChange={() => onChange(o.value)}
          />
          <div className="flex-1 min-w-0">
            <div className="text-sm font-medium">{o.title}</div>
            <div className="text-xs text-muted-foreground">{o.description}</div>
          </div>
        </label>
      ))}
    </div>
  );
}
