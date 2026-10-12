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
  useDeleteEmailConfig,
  useEmailConfig,
  useEmailKinds,
  useSendTestEmail,
  useUpsertEmailConfig,
} from "../../../lib/email/useEmail";

const schema = z.object({
  name: z.string().min(1, "Required."),
  region: z.string().min(1, "Required."),
  access_key_id: z.string().min(1, "Required."),
  // Empty means "keep the existing secret" — the server never sends it back,
  // so the field starts blank on every load.
  secret_access_key: z.string().optional(),
  from_address: z.string().trim().email("Enter a single email address."),
  from_name: z.string().optional(),
  configuration_set: z.string().optional(),
});

type FormValues = z.infer<typeof schema>;

const defaults: FormValues = {
  name: "Notifications",
  region: "us-east-1",
  access_key_id: "",
  secret_access_key: "",
  from_address: "",
  from_name: "",
  configuration_set: "",
};

/** Read a string off the redacted `config` blob without an `any`. */
function str(config: unknown, key: string): string {
  const v = (config as Record<string, unknown> | null)?.[key];
  return typeof v === "string" ? v : "";
}

export function EmailSettingsPage() {
  const cfg = useEmailConfig();
  const kinds = useEmailKinds();
  const upsert = useUpsertEmailConfig();
  const remove = useDeleteEmailConfig();
  const sendTest = useSendTestEmail();
  const [topError, setTopError] = useState<string | null>(null);
  const [topSuccess, setTopSuccess] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [testTo, setTestTo] = useState("");

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: defaults,
  });

  useEffect(() => {
    if (!cfg.data) return;
    const c = cfg.data.config;
    form.reset({
      name: cfg.data.name,
      region: str(c, "region"),
      access_key_id: str(c, "access_key_id"),
      // Never seeded — the server can't send it, and a placeholder here would
      // be submitted back as a literal new secret.
      secret_access_key: "",
      from_address: str(c, "from_address"),
      from_name: str(c, "from_name"),
      configuration_set: str(c, "configuration_set"),
    });
  }, [cfg.data, form]);

  const isExisting = !!cfg.data;
  const hasSecret = cfg.data?.secret_fields?.["secret_access_key"] === true;

  const onSubmit = form.handleSubmit(async (values) => {
    setTopError(null);
    setTopSuccess(null);
    const secret = values.secret_access_key?.trim();
    try {
      await upsert.mutateAsync({
        kind: kinds.data?.[0]?.kind ?? "ses",
        name: values.name.trim(),
        config: {
          region: values.region.trim(),
          access_key_id: values.access_key_id.trim(),
          // Omitted when blank, which the server reads as "keep the current
          // one".
          ...(secret ? { secret_access_key: secret } : {}),
          from_address: values.from_address.trim(),
          from_name: values.from_name?.trim() || undefined,
          configuration_set: values.configuration_set?.trim() || undefined,
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
    const to = testTo.trim();
    try {
      // A rejection comes back as a 200 with `ok: false` — the message is the
      // answer, not an exception.
      const result = await sendTest.mutateAsync(to);
      if (result.ok) {
        setTopSuccess(`Sent a test email to ${to}. Check that inbox (and its spam folder).`);
      } else {
        setTopError(result.error ?? "The email provider rejected the test.");
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
      setTopSuccess("Email provider removed.");
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
        <h1 className="text-2xl font-semibold tracking-tight">Email</h1>
        <p className="text-sm text-muted-foreground">
          The provider that sends submission notifications. Each form chooses
          its own recipient under <strong>Notification email</strong> in the
          form's settings; this page sets who the mail comes from and how it's
          sent. Amazon SES is supported today.
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
              : "Until a provider is saved, forms with a notification email record a failed delivery for every submission."}
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
              <Input {...form.register("name")} placeholder="Notifications" />
            </FormField>

            <FormField
              id="region"
              label="SES region"
              hint="The AWS region your sender identity is verified in."
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
                    : "No secret is on record — notifications will fail until you set one."
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
              id="from_address"
              label="From address"
              hint="Must be a verified identity in SES — the address itself, or any address on a verified domain."
              error={form.formState.errors.from_address?.message}
            >
              <Input
                type="email"
                {...form.register("from_address")}
                placeholder="forms@example.com"
              />
            </FormField>

            <FormField
              id="from_name"
              label="From name"
              hint="Optional. Shown beside the address, e.g. OpenRelay."
              error={form.formState.errors.from_name?.message}
            >
              <Input {...form.register("from_name")} placeholder="OpenRelay" />
            </FormField>

            <details className="rounded border border-border p-3">
              <summary className="cursor-pointer text-sm font-medium">
                Advanced
              </summary>
              <div className="mt-3 space-y-3">
                <FormField
                  id="configuration_set"
                  label="Configuration set"
                  hint="Optional SES configuration set, for event publishing or a dedicated IP pool."
                  error={form.formState.errors.configuration_set?.message}
                >
                  <Input {...form.register("configuration_set")} />
                </FormField>
              </div>
            </details>

            <div className="flex items-center gap-2 pt-2">
              <Button type="submit" disabled={upsert.isPending}>
                {upsert.isPending ? "Saving…" : "Save"}
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
          <CardTitle>Send a test email</CardTitle>
          <CardDescription>
            Sends a real message through the saved settings, so a rejection
            shows up here rather than on a form's first submission.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex items-end gap-2">
            <FormField id="test_to" label="Send to" className="flex-1">
              <Input
                type="email"
                value={testTo}
                onChange={(e) => setTestTo(e.target.value)}
                placeholder="you@example.com"
              />
            </FormField>
            <Button
              type="button"
              variant="outline"
              onClick={onTest}
              disabled={sendTest.isPending || !isExisting || !testTo.trim()}
              title={!isExisting ? "Save the settings first" : undefined}
            >
              {sendTest.isPending ? "Sending…" : "Send test"}
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>SES setup</CardTitle>
          <CardDescription>
            These are settings in your AWS account; OpenRelay can't change
            them for you.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4 text-sm">
          <div className="space-y-1">
            <p className="font-medium">1. Verify the sender</p>
            <p className="text-muted-foreground">
              SES only sends from a verified identity. Verify the from address,
              or its whole domain, in the SES console for the region above.
            </p>
          </div>
          <div className="space-y-1">
            <p className="font-medium">2. Leave the sandbox, or verify recipients</p>
            <p className="text-muted-foreground">
              New SES accounts start in the <em>sandbox</em>, where mail can
              only go <strong>to</strong> verified addresses as well. Until you
              request production access, verify every notification recipient
              in SES — otherwise SES rejects the message and the delivery shows
              as failed.
            </p>
          </div>
          <p className="text-xs text-muted-foreground">
            The credentials above need{" "}
            <code className="font-mono">ses:SendEmail</code> for the sender
            identity. Notifications are plain text and are sent at least once:
            in rare cases (a crash mid-send) one can arrive twice.
          </p>
        </CardContent>
      </Card>

      <ConfirmDialog
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        title="Remove email provider?"
        description="Forms with a notification email will record a failed delivery for every new submission until a provider is configured again."
        confirmLabel="Remove"
        confirmVariant="destructive"
        pending={remove.isPending}
        onConfirm={onDelete}
      />
    </div>
  );
}
