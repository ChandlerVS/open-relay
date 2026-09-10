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
  useDeleteStorageConfig,
  useStorageConfig,
  useStorageKinds,
  useTestStorageConfig,
  useUpsertStorageConfig,
} from "../../../lib/storage/useStorage";

const schema = z.object({
  name: z.string().min(1, "Required."),
  bucket: z.string().min(1, "Required."),
  region: z.string().min(1, "Required."),
  endpoint: z.string().optional(),
  access_key_id: z.string().min(1, "Required."),
  // Empty means "keep the existing secret" — the server never sends it back,
  // so the field starts blank on every load.
  secret_access_key: z.string().optional(),
  force_path_style: z.boolean(),
  visibility: z.enum(["public", "presigned"]),
  presigned_ttl_days: z.string().optional(),
  public_base_url: z.string().optional(),
  key_prefix: z.string().optional(),
});

type FormValues = z.infer<typeof schema>;

const defaults: FormValues = {
  name: "File uploads",
  bucket: "",
  region: "us-east-1",
  endpoint: "",
  access_key_id: "",
  secret_access_key: "",
  force_path_style: false,
  visibility: "public",
  presigned_ttl_days: "7",
  public_base_url: "",
  key_prefix: "",
};

/** Read a string off the redacted `config` blob without an `any`. */
function str(config: unknown, key: string): string {
  const v = (config as Record<string, unknown> | null)?.[key];
  return typeof v === "string" ? v : "";
}
function bool(config: unknown, key: string): boolean {
  return (config as Record<string, unknown> | null)?.[key] === true;
}

export function StorageSettingsPage() {
  const cfg = useStorageConfig();
  const kinds = useStorageKinds();
  const upsert = useUpsertStorageConfig();
  const remove = useDeleteStorageConfig();
  const probe = useTestStorageConfig();
  const [topError, setTopError] = useState<string | null>(null);
  const [topSuccess, setTopSuccess] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: defaults,
  });

  useEffect(() => {
    if (!cfg.data) return;
    const c = cfg.data.config;
    form.reset({
      name: cfg.data.name,
      bucket: str(c, "bucket"),
      region: str(c, "region"),
      endpoint: str(c, "endpoint"),
      access_key_id: str(c, "access_key_id"),
      // Never seeded — the server can't send it, and a placeholder here would
      // be submitted back as a literal new secret.
      secret_access_key: "",
      force_path_style: bool(c, "force_path_style"),
      visibility: str(c, "visibility") === "presigned" ? "presigned" : "public",
      presigned_ttl_days: String(
        (c as Record<string, unknown> | null)?.["presigned_ttl_days"] ?? 7,
      ),
      public_base_url: str(c, "public_base_url"),
      key_prefix: str(c, "key_prefix"),
    });
  }, [cfg.data, form]);

  const isExisting = !!cfg.data;
  const hasSecret = cfg.data?.secret_fields?.["secret_access_key"] === true;
  const visibility = form.watch("visibility");

  const onSubmit = form.handleSubmit(async (values) => {
    setTopError(null);
    setTopSuccess(null);
    const secret = values.secret_access_key?.trim();
    try {
      await upsert.mutateAsync({
        kind: kinds.data?.[0]?.kind ?? "s3",
        name: values.name.trim(),
        config: {
          bucket: values.bucket.trim(),
          region: values.region.trim(),
          endpoint: values.endpoint?.trim() || undefined,
          access_key_id: values.access_key_id.trim(),
          // Omitted when blank, which the server reads as "keep the current
          // one". Sending "" would be indistinguishable and is handled the
          // same way, but omitting says what we mean.
          ...(secret ? { secret_access_key: secret } : {}),
          force_path_style: values.force_path_style,
          visibility: values.visibility,
          presigned_ttl_days: Number(values.presigned_ttl_days) || 7,
          public_base_url: values.public_base_url?.trim() || undefined,
          key_prefix: values.key_prefix?.trim() || undefined,
        },
      });
      form.setValue("secret_access_key", "");
      setTopSuccess("Saved.");
    } catch (err) {
      setTopError((err as Error).message);
    }
  });

  const onTest = async () => {
    setTopError(null);
    setTopSuccess(null);
    try {
      // A rejecting bucket comes back as a 200 with `ok: false` — the message
      // is the answer, not an exception.
      const result = await probe.mutateAsync();
      if (result.ok) {
        setTopSuccess("Uploaded and removed a test file successfully.");
      } else {
        setTopError(result.error ?? "The storage provider rejected the test.");
      }
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
      setTopSuccess("Storage provider removed.");
    } catch (err) {
      setTopError((err as Error).message);
      setConfirmDelete(false);
    }
  };

  if (cfg.isPending) {
    return <Skeleton className="h-64 w-full" />;
  }

  const origin = window.location.origin;

  return (
    <div className="space-y-6 max-w-3xl">
      <div className="space-y-1">
        <h1 className="text-2xl font-semibold tracking-tight">File storage</h1>
        <p className="text-sm text-muted-foreground">
          Where files submitted through a form's <strong>File upload</strong>{" "}
          field are kept. Works with Amazon S3 and any S3-compatible store
          (MinIO, Cloudflare R2, DigitalOcean Spaces, Backblaze B2). Files go
          straight from the visitor's browser to your bucket — they never pass
          through OpenRelay — and the submission records the object's URL.
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
          <AlertTitle>Success</AlertTitle>
          <AlertDescription>{topSuccess}</AlertDescription>
        </Alert>
      )}

      <Card>
        <CardHeader>
          <CardTitle>
            {isExisting ? `Active: ${cfg.data?.name}` : "Not configured"}
          </CardTitle>
          <CardDescription>
            {isExisting
              ? "Edit the active provider. Leave the secret access key blank to keep the current one."
              : "Fill in your bucket and credentials, then use Test connection before saving relies on it."}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form className="space-y-4" onSubmit={onSubmit}>
            <FormField
              id="name"
              label="Label"
              hint="Shown in the admin only."
              error={form.formState.errors.name?.message}
            >
              <Input {...form.register("name")} placeholder="File uploads" />
            </FormField>

            <FormField
              id="bucket"
              label="Bucket"
              error={form.formState.errors.bucket?.message}
            >
              <Input {...form.register("bucket")} placeholder="my-form-uploads" />
            </FormField>

            <FormField
              id="region"
              label="Region"
              hint="For non-AWS stores that don't have regions, any value works (e.g. us-east-1)."
              error={form.formState.errors.region?.message}
            >
              <Input {...form.register("region")} placeholder="us-east-1" />
            </FormField>

            <FormField
              id="access_key_id"
              label="Access key ID"
              error={form.formState.errors.access_key_id?.message}
            >
              <Input {...form.register("access_key_id")} placeholder="AKIA…" />
            </FormField>

            <FormField
              id="secret_access_key"
              label={
                isExisting
                  ? "Secret access key (leave blank to keep)"
                  : "Secret access key"
              }
              hint={
                isExisting
                  ? hasSecret
                    ? "A secret is on record."
                    : "No secret is on record — uploads will fail until you set one."
                  : undefined
              }
              error={form.formState.errors.secret_access_key?.message}
            >
              <Input
                type="password"
                autoComplete="new-password"
                {...form.register("secret_access_key")}
              />
            </FormField>

            <FormField
              id="visibility"
              label="File links"
              hint={
                visibility === "public"
                  ? "Stores a permanent URL, so the link keeps working from whatever CRM the submission is delivered to. Requires the bucket policy below."
                  : "Keeps the bucket private and stores an expiring link. Safest, but the link stops working in delivered records once it expires."
              }
            >
              <select
                id="visibility"
                className="w-full h-9 rounded border border-border bg-background px-2 text-sm"
                {...form.register("visibility")}
              >
                <option value="public">Public — permanent link</option>
                <option value="presigned">Private — expiring link</option>
              </select>
            </FormField>

            {visibility === "presigned" && (
              <FormField
                id="presigned_ttl_days"
                label="Link lifetime (days)"
                hint="1–7. Seven days is the maximum AWS signatures allow."
              >
                <Input
                  type="number"
                  min={1}
                  max={7}
                  {...form.register("presigned_ttl_days")}
                />
              </FormField>
            )}

            {visibility === "public" && (
              <FormField
                id="public_base_url"
                label="Public base URL"
                hint="Optional. A CDN or custom domain serving the bucket; used instead of the bucket's own host when building links."
              >
                <Input
                  {...form.register("public_base_url")}
                  placeholder="https://cdn.example.com"
                />
              </FormField>
            )}

            <details className="rounded border border-border p-3">
              <summary className="cursor-pointer text-sm font-medium">
                Advanced
              </summary>
              <div className="mt-3 space-y-3">
                <FormField
                  id="endpoint"
                  label="Endpoint"
                  hint="Leave blank for Amazon S3. Set for MinIO, R2, Spaces, or B2 — e.g. http://localhost:9000."
                >
                  <Input {...form.register("endpoint")} placeholder="https://…" />
                </FormField>
                <label className="flex items-center gap-2 text-sm">
                  <input type="checkbox" {...form.register("force_path_style")} />
                  Use path-style addressing (required by MinIO)
                </label>
                <FormField
                  id="key_prefix"
                  label="Key prefix"
                  hint="Optional folder to keep uploads under, e.g. openrelay/. Useful as the target for the lifecycle rule below."
                >
                  <Input {...form.register("key_prefix")} placeholder="openrelay/" />
                </FormField>
              </div>
            </details>

            <div className="flex items-center gap-2 pt-2">
              <Button type="submit" disabled={upsert.isPending}>
                {upsert.isPending ? "Saving…" : "Save"}
              </Button>
              <Button
                type="button"
                variant="outline"
                onClick={onTest}
                disabled={probe.isPending || !isExisting}
                title={!isExisting ? "Save the settings first" : undefined}
              >
                {probe.isPending ? "Testing…" : "Test connection"}
              </Button>
              {isExisting && (
                <Button
                  type="button"
                  variant="destructive"
                  className="ml-auto"
                  onClick={() => setConfirmDelete(true)}
                >
                  Remove provider
                </Button>
              )}
            </div>
          </form>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Bucket setup</CardTitle>
          <CardDescription>
            Two things have to be set on the bucket itself. OpenRelay can't do
            them for you — they're changes to your cloud account.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4 text-sm">
          <div className="space-y-1">
            <p className="font-medium">1. CORS — required</p>
            <p className="text-muted-foreground">
              Visitors' browsers upload directly to the bucket, so it has to
              allow cross-origin <code className="font-mono">PUT</code>.
              Embedded forms run on whatever page they're pasted into, which
              is why the origin list is open. Each upload URL is issued only
              for a file field that exists on one of your forms, expires after
              five minutes, and is signed for one exact file size and content
              type — so it can't be used to write anything else to the bucket.
            </p>
            <pre className="rounded bg-muted p-3 text-xs overflow-x-auto">
{`[
  {
    "AllowedOrigins": ["*"],
    "AllowedMethods": ["PUT"],
    "AllowedHeaders": ["content-type"],
    "MaxAgeSeconds": 3000
  }
]`}
            </pre>
          </div>

          {visibility === "public" && (
            <div className="space-y-1">
              <p className="font-medium">
                2. Public read access — required for permanent links
              </p>
              <p className="text-muted-foreground">
                With "Public — permanent link" selected, the stored URL is
                unauthenticated, so the objects have to be readable. Use a
                bucket policy rather than per-object ACLs — new buckets reject
                ACLs outright. Anyone with the URL can read the file; the keys
                contain a random UUID so they aren't guessable, but they are
                not secret.
              </p>
              <pre className="rounded bg-muted p-3 text-xs overflow-x-auto">
{`{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Principal": "*",
    "Action": "s3:GetObject",
    "Resource": "arn:aws:s3:::YOUR-BUCKET/*"
  }]
}`}
              </pre>
            </div>
          )}

          <div className="space-y-1">
            <p className="font-medium">3. A lifecycle rule — recommended</p>
            <p className="text-muted-foreground">
              A visitor can attach a file and then abandon the form. OpenRelay
              doesn't track those, so set an object lifecycle rule to expire
              anything under the upload prefix after however long you keep
              submissions. Without one, abandoned uploads accumulate.
            </p>
          </div>

          <p className="text-xs text-muted-foreground">
            The credentials above need <code className="font-mono">s3:PutObject</code>{" "}
            (and <code className="font-mono">s3:DeleteObject</code> for the
            connection test's cleanup) on this bucket. This OpenRelay instance
            is at <code className="font-mono">{origin}</code>.
          </p>
        </CardContent>
      </Card>

      <ConfirmDialog
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        title="Remove storage provider?"
        description="Forms with a file field will stop accepting uploads. Files already in the bucket are left alone, and links already recorded on past submissions keep working."
        confirmLabel="Remove"
        confirmVariant="destructive"
        pending={remove.isPending}
        onConfirm={onDelete}
      />
    </div>
  );
}
