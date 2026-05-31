import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import {
  Alert,
  AlertDescription,
  AlertTitle,
  Button,
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  ConfirmDialog,
  FormField,
  Input,
  Skeleton,
} from "@open-relay/ui";
import {
  useDeleteOAuthConfig,
  useOAuthAdminConfig,
  useOAuthDiscover,
  useUpsertOAuthConfig,
  type DiscoveryPrefill,
} from "../../../lib/oauth/useOAuth";
import { useRoleSelectList } from "../../../lib/roles/useRoles";

const schema = z.object({
  display_name: z.string().min(1, "Required."),
  discovery_url: z.string().optional(),
  client_id: z.string().min(1, "Required."),
  client_secret: z.string().optional(),
  authorize_url: z.string().min(1, "Required.").url("Must be a URL."),
  token_url: z.string().min(1, "Required.").url("Must be a URL."),
  userinfo_url: z
    .string()
    .url("Must be a URL.")
    .or(z.literal(""))
    .optional(),
  jwks_url: z
    .string()
    .url("Must be a URL.")
    .or(z.literal(""))
    .optional(),
  issuer: z.string().optional(),
  scopes: z.string().min(1, "Required."),
  default_role_id: z.string().optional(),
  email_claim: z.string().optional(),
  subject_claim: z.string().optional(),
});

type FormValues = z.infer<typeof schema>;

const defaults: FormValues = {
  display_name: "",
  discovery_url: "",
  client_id: "",
  client_secret: "",
  authorize_url: "",
  token_url: "",
  userinfo_url: "",
  jwks_url: "",
  issuer: "",
  scopes: "openid email profile",
  default_role_id: "",
  email_claim: "email",
  subject_claim: "sub",
};

export function AuthSettingsPage() {
  const cfg = useOAuthAdminConfig();
  const roles = useRoleSelectList();
  const upsert = useUpsertOAuthConfig();
  const remove = useDeleteOAuthConfig();
  const discover = useOAuthDiscover();
  const [topError, setTopError] = useState<string | null>(null);
  const [topSuccess, setTopSuccess] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: defaults,
  });

  useEffect(() => {
    if (cfg.data) {
      form.reset({
        display_name: cfg.data.display_name,
        discovery_url: cfg.data.discovery_url ?? "",
        client_id: cfg.data.client_id,
        client_secret: "",
        authorize_url: cfg.data.authorize_url,
        token_url: cfg.data.token_url,
        userinfo_url: cfg.data.userinfo_url ?? "",
        jwks_url: cfg.data.jwks_url ?? "",
        issuer: cfg.data.issuer ?? "",
        scopes: cfg.data.scopes,
        default_role_id: cfg.data.default_role_id?.toString() ?? "",
        email_claim: cfg.data.email_claim,
        subject_claim: cfg.data.subject_claim,
      });
    }
  }, [cfg.data, form]);

  const isExisting = !!cfg.data;

  const onSubmit = form.handleSubmit(async (values) => {
    setTopError(null);
    setTopSuccess(null);
    const secret = values.client_secret?.trim();
    try {
      await upsert.mutateAsync({
        display_name: values.display_name.trim(),
        discovery_url: values.discovery_url?.trim() || undefined,
        client_id: values.client_id.trim(),
        client_secret: secret ? secret : undefined,
        authorize_url: values.authorize_url.trim(),
        token_url: values.token_url.trim(),
        userinfo_url: values.userinfo_url?.trim() || undefined,
        jwks_url: values.jwks_url?.trim() || undefined,
        issuer: values.issuer?.trim() || undefined,
        scopes: values.scopes.trim(),
        default_role_id: values.default_role_id
          ? Number(values.default_role_id)
          : undefined,
        email_claim: values.email_claim?.trim() || undefined,
        subject_claim: values.subject_claim?.trim() || undefined,
      });
      form.setValue("client_secret", "");
      setTopSuccess("Saved.");
    } catch (err) {
      setTopError((err as Error).message);
    }
  });

  const onDiscover = async () => {
    setTopError(null);
    setTopSuccess(null);
    const url = form.getValues("discovery_url")?.trim();
    if (!url) {
      setTopError("Enter a discovery URL first.");
      return;
    }
    try {
      const prefill: DiscoveryPrefill = await discover.mutateAsync({
        discovery_url: url,
      });
      form.setValue("authorize_url", prefill.authorize_url, {
        shouldDirty: true,
      });
      form.setValue("token_url", prefill.token_url, { shouldDirty: true });
      form.setValue("userinfo_url", prefill.userinfo_url ?? "", {
        shouldDirty: true,
      });
      form.setValue("jwks_url", prefill.jwks_url ?? "", { shouldDirty: true });
      form.setValue("issuer", prefill.issuer ?? "", { shouldDirty: true });
      if (prefill.scopes_supported && prefill.scopes_supported.length > 0) {
        // Only override if our current scopes are still the default.
        const current = form.getValues("scopes")?.trim();
        if (!current || current === defaults.scopes) {
          // Prefer common OIDC scopes the IdP supports.
          const wanted = ["openid", "email", "profile"].filter((s) =>
            prefill.scopes_supported?.includes(s),
          );
          if (wanted.length > 0) {
            form.setValue("scopes", wanted.join(" "), { shouldDirty: true });
          }
        }
      }
      setTopSuccess("Endpoints filled in from discovery doc.");
    } catch (err) {
      setTopError((err as Error).message);
    }
  };

  const onDelete = async () => {
    setTopError(null);
    setTopSuccess(null);
    try {
      await remove.mutateAsync();
      form.reset(defaults);
      setConfirmDelete(false);
      setTopSuccess("OAuth provider removed.");
    } catch (err) {
      setTopError((err as Error).message);
      setConfirmDelete(false);
    }
  };

  if (cfg.isPending) {
    return <Skeleton className="h-64 w-full" />;
  }

  return (
    <div className="space-y-6 max-w-3xl">
      <div className="space-y-1">
        <h1 className="text-2xl font-semibold tracking-tight">Authentication</h1>
        <p className="text-sm text-muted-foreground">
          Configure an OIDC-compliant OAuth provider. When set, the login page
          shows a "Sign in with {`{name}`}" button alongside the password form.
          Users can also link their existing accounts.
        </p>
      </div>

      {topError && (
        <Alert variant="destructive">
          <AlertTitle>Error</AlertTitle>
          <AlertDescription>{topError}</AlertDescription>
        </Alert>
      )}
      {topSuccess && (
        <Alert>
          <AlertTitle>Saved</AlertTitle>
          <AlertDescription>{topSuccess}</AlertDescription>
        </Alert>
      )}

      <Card>
        <CardHeader>
          <CardTitle>
            {isExisting ? `Active: ${cfg.data?.display_name}` : "Not configured"}
          </CardTitle>
          <CardDescription>
            {isExisting
              ? "Edit the active OAuth provider. Leave the client secret empty to keep the existing value."
              : "Fill out the fields below and save. Or paste a discovery URL and click Discover to prefill the endpoints."}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form className="space-y-4" onSubmit={onSubmit}>
            <FormField
              id="display_name"
              label='Button label (e.g. "Microsoft")'
              error={form.formState.errors.display_name?.message}
            >
              <Input {...form.register("display_name")} placeholder="Microsoft" />
            </FormField>

            <div className="space-y-2">
              <FormField
                id="discovery_url"
                label="OIDC discovery URL"
                hint="Paste the well-known URL; we'll fetch the endpoints below."
                error={form.formState.errors.discovery_url?.message}
              >
                <Input
                  {...form.register("discovery_url")}
                  placeholder="https://login.microsoftonline.com/{tenant}/v2.0/.well-known/openid-configuration"
                />
              </FormField>
              <Button
                type="button"
                variant="outline"
                onClick={onDiscover}
                disabled={discover.isPending}
              >
                {discover.isPending ? "Fetching..." : "Discover"}
              </Button>
            </div>

            <FormField
              id="authorize_url"
              label="Authorize endpoint"
              error={form.formState.errors.authorize_url?.message}
            >
              <Input {...form.register("authorize_url")} />
            </FormField>

            <FormField
              id="token_url"
              label="Token endpoint"
              error={form.formState.errors.token_url?.message}
            >
              <Input {...form.register("token_url")} />
            </FormField>

            <FormField
              id="userinfo_url"
              label="Userinfo endpoint"
              hint="Optional, but recommended. We GET this with the access token to read email/sub."
              error={form.formState.errors.userinfo_url?.message}
            >
              <Input {...form.register("userinfo_url")} />
            </FormField>

            <FormField
              id="client_id"
              label="Client ID"
              error={form.formState.errors.client_id?.message}
            >
              <Input {...form.register("client_id")} />
            </FormField>

            <FormField
              id="client_secret"
              label={isExisting ? "Client secret (leave blank to keep)" : "Client secret"}
              error={form.formState.errors.client_secret?.message}
            >
              <Input
                type="password"
                autoComplete="new-password"
                {...form.register("client_secret")}
              />
            </FormField>

            <FormField
              id="scopes"
              label="Scopes"
              hint="Space-separated. OIDC requires `openid`."
              error={form.formState.errors.scopes?.message}
            >
              <Input {...form.register("scopes")} />
            </FormField>

            <FormField
              id="default_role_id"
              label="Default role for new OAuth users"
              hint="Assigned when an OAuth sign-in arrives for an email with no existing user."
              error={form.formState.errors.default_role_id?.message}
            >
              <select
                id="default_role_id"
                className="w-full h-9 rounded border border-border bg-background px-2 text-sm"
                {...form.register("default_role_id")}
              >
                <option value="">— No default role —</option>
                {(roles.data ?? []).map((r) => (
                  <option key={r.id} value={r.id}>
                    {r.name}
                  </option>
                ))}
              </select>
            </FormField>

            <details className="rounded border border-border p-3">
              <summary className="cursor-pointer text-sm font-medium">
                Advanced
              </summary>
              <div className="mt-3 space-y-3">
                <FormField
                  id="jwks_url"
                  label="JWKS URL"
                  hint="Required. Public keys used to verify the ID-token signature."
                  error={form.formState.errors.jwks_url?.message}
                >
                  <Input {...form.register("jwks_url")} />
                </FormField>
                <FormField
                  id="issuer"
                  label="Issuer"
                  hint="Required. Must match the id_token `iss` claim. Microsoft Entra multi-tenant may use the `{tenantid}` placeholder."
                  error={form.formState.errors.issuer?.message}
                >
                  <Input {...form.register("issuer")} />
                </FormField>
                <FormField id="email_claim" label="Email claim">
                  <Input {...form.register("email_claim")} />
                </FormField>
                <FormField id="subject_claim" label="Subject claim">
                  <Input {...form.register("subject_claim")} />
                </FormField>
                <p className="text-xs text-muted-foreground">
                  Redirect URI to register with the IdP:{" "}
                  <code className="font-mono">
                    {new URL("/auth/oauth/callback", window.location.origin.replace(":5173", ":8080")).toString()}
                  </code>
                </p>
              </div>
            </details>

            <div className="flex items-center justify-between pt-2">
              <Button type="submit" disabled={upsert.isPending}>
                {upsert.isPending ? "Saving..." : "Save"}
              </Button>
              {isExisting && (
                <Button
                  type="button"
                  variant="destructive"
                  onClick={() => setConfirmDelete(true)}
                >
                  Remove provider
                </Button>
              )}
            </div>
          </form>
        </CardContent>
      </Card>

      <ConfirmDialog
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        title="Remove OAuth provider?"
        description="Users with no local password may lose access. They can be given a password from the Users page before removal."
        confirmLabel="Remove"
        confirmVariant="destructive"
        pending={remove.isPending}
        onConfirm={onDelete}
      />
    </div>
  );
}
