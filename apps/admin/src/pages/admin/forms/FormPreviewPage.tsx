import { useRef, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { ArrowLeft, Check, Copy, Download, TriangleAlert } from "lucide-react";
import { QRCodeCanvas } from "qrcode.react";
import { ShadowForm } from "@open-relay/form-renderer";
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
  FormField,
  Input,
  Skeleton,
} from "@open-relay/ui";
import { api } from "../../../lib/api/client";
import { useForm, useFormEmbed, type FormDto } from "../../../lib/forms/useForms";
import { useRepsList, type RepDto } from "../../../lib/reps/useReps";
import { useTheme } from "../../../lib/theme/useTheme";

type Result =
  | { kind: "submitted"; id: number }
  | { kind: "error"; message: string };

export function FormPreviewPage() {
  const { id } = useParams<{ id: string }>();
  const formId = Number(id);
  const valid = Number.isFinite(formId);

  const { resolved: theme } = useTheme();
  const { data: form, isLoading } = useForm(valid ? formId : null);
  const embed = useFormEmbed(valid ? formId : null);
  const [result, setResult] = useState<Result | null>(null);

  return (
    <div className="space-y-6 max-w-5xl">
      <div>
        <Link
          to="/forms"
          className="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground"
        >
          <ArrowLeft className="h-4 w-4" />
          Back to forms
        </Link>
        <h1 className="mt-2 text-2xl font-semibold tracking-tight">
          {isLoading ? (
            <Skeleton className="h-7 w-48" />
          ) : (
            `Preview${form ? ` · ${form.name}` : ""}`
          )}
        </h1>
        <p className="text-sm text-muted-foreground">
          Rendered with the embed SDK exactly as a host page sees it.
        </p>
      </div>

      <Alert variant="destructive">
        <TriangleAlert className="h-4 w-4" />
        <AlertTitle>Submissions are real</AlertTitle>
        <AlertDescription>
          Submitting saves a submission to the database and delivers it to this
          form's configured backends — including creating real records in
          external systems.
        </AlertDescription>
      </Alert>

      <div className="grid gap-6 md:grid-cols-2">
        <div>
          {valid ? (
            <ShadowForm
              formId={String(formId)}
              apiUrl={api.baseUrl}
              theme={theme}
              onSubmitted={({ id }) => setResult({ kind: "submitted", id })}
              onError={(message) => setResult({ kind: "error", message })}
            />
          ) : (
            <Alert variant="destructive">
              <AlertTitle>Invalid form id</AlertTitle>
              <AlertDescription>"{id}" is not a valid form id.</AlertDescription>
            </Alert>
          )}
        </div>

        <Card>
          <CardHeader>
            <CardTitle>Submission result</CardTitle>
            <CardDescription>
              The outcome of your most recent test submission.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4 text-sm">
            {result === null && (
              <p className="text-muted-foreground">
                No submission yet. Fill out the form and submit to see the
                result here.
              </p>
            )}
            {result?.kind === "submitted" && (
              <>
                <div>
                  <div className="text-muted-foreground">Submission id</div>
                  <code className="rounded bg-muted px-1.5 py-0.5">
                    {result.id}
                  </code>
                </div>
                <div>
                  <div className="text-muted-foreground">
                    Delivering to backends
                  </div>
                  {form && form.backends.length > 0 ? (
                    <ul className="mt-1 space-y-1">
                      {form.backends.map((b, i) => (
                        <li key={`${b.kind}-${b.instance_id ?? "default"}-${i}`}>
                          <code className="rounded bg-muted px-1.5 py-0.5">
                            {b.kind}
                          </code>
                          {b.instance_id != null && (
                            <span className="ml-2 text-muted-foreground">
                              instance #{b.instance_id}
                            </span>
                          )}
                        </li>
                      ))}
                    </ul>
                  ) : (
                    <p className="mt-1 text-muted-foreground">
                      No backends bound — stored only.
                    </p>
                  )}
                </div>
              </>
            )}
            {result?.kind === "error" && (
              <Alert variant="destructive">
                <AlertTitle>Submission failed</AlertTitle>
                <AlertDescription>{result.message}</AlertDescription>
              </Alert>
            )}
          </CardContent>
        </Card>
      </div>

      {form && <RepLinksCard form={form} />}

      <Card>
        <CardHeader>
          <CardTitle>Embed on your site</CardTitle>
          <CardDescription>
            Paste this snippet into your page's HTML where you want the form to
            appear. It loads the OpenRelay SDK and renders this form — no other
            setup required.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {embed.isLoading && <Skeleton className="h-16 w-full" />}
          {embed.isError && (
            <Alert variant="destructive">
              <TriangleAlert className="h-4 w-4" />
              <AlertTitle>Couldn't load embed code</AlertTitle>
              <AlertDescription>{embed.error.message}</AlertDescription>
            </Alert>
          )}
          {embed.data && (
            <>
              <pre className="overflow-x-auto rounded-md border bg-muted p-3 text-xs leading-relaxed">
                <code>{embed.data.snippet}</code>
              </pre>
              <CopyButton value={embed.data.snippet} />
            </>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

/**
 * Per-rep QR + link generator. The form can be embedded on any page, so the
 * admin types that landing URL (minus query params); we append `?rep=<key>`
 * (plus any configured source-param values) and render a scannable QR per rep
 * associated with this form.
 */
function RepLinksCard({ form }: { form: FormDto }) {
  const { data: reps } = useRepsList();
  const [landingUrl, setLandingUrl] = useState("");
  const [paramValues, setParamValues] = useState<Record<string, string>>({});

  const formReps: RepDto[] = (reps?.items ?? []).filter((r) =>
    form.reps.includes(r.id),
  );

  if (form.reps.length === 0) return null;

  const extraParams = (): Record<string, string> => {
    const out: Record<string, string> = {};
    for (const sp of form.source_params) {
      const v = paramValues[sp.param]?.trim();
      if (v) out[sp.param] = v;
    }
    return out;
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Per-rep QR links</CardTitle>
        <CardDescription>
          Generate a QR code per rep for business cards and events. Enter the
          page URL where this form is embedded; each link tags the lead to that
          rep on submit.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="grid gap-4 sm:grid-cols-2">
          <FormField
            id="rep-landing-url"
            label="Landing page URL"
            hint="The page where the form is embedded — without query params."
          >
            <Input
              value={landingUrl}
              placeholder="https://example.com/contact"
              onChange={(e) => setLandingUrl(e.target.value)}
            />
          </FormField>
          {form.source_params.map((sp) => (
            <FormField
              key={sp.param}
              id={`rep-param-${sp.param}`}
              label={sp.param}
              hint="Captured as a tag on every scan of these links."
            >
              <Input
                value={paramValues[sp.param] ?? ""}
                placeholder={sp.param === "event" ? "mjbiz-2026" : sp.param}
                onChange={(e) =>
                  setParamValues((v) => ({ ...v, [sp.param]: e.target.value }))
                }
              />
            </FormField>
          ))}
        </div>

        {formReps.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No reps attached to this form yet. Edit the form to attach reps.
          </p>
        ) : !landingUrl.trim() ? (
          <p className="text-sm text-muted-foreground">
            Enter a landing page URL to generate links.
          </p>
        ) : (
          <div className="grid gap-4 sm:grid-cols-2">
            {formReps.map((rep) => (
              <RepQrTile
                key={rep.id}
                rep={rep}
                url={buildRepUrl(landingUrl, rep.key, extraParams())}
              />
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

/** Append `rep` + extra params to a base URL, tolerating non-absolute input. */
function buildRepUrl(
  base: string,
  repKey: string,
  extra: Record<string, string>,
): string {
  const params = { rep: repKey, ...extra };
  try {
    const u = new URL(base);
    for (const [k, v] of Object.entries(params)) u.searchParams.set(k, v);
    return u.toString();
  } catch {
    const qs = Object.entries(params)
      .map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`)
      .join("&");
    return base + (base.includes("?") ? "&" : "?") + qs;
  }
}

function RepQrTile({ rep, url }: { rep: RepDto; url: string }) {
  const canvasWrap = useRef<HTMLDivElement>(null);

  const download = () => {
    const canvas = canvasWrap.current?.querySelector("canvas");
    if (!canvas) return;
    const link = document.createElement("a");
    link.download = `qr-${rep.key}.png`;
    link.href = canvas.toDataURL("image/png");
    link.click();
  };

  return (
    <div className="flex gap-3 rounded-md border border-border p-3">
      <div ref={canvasWrap} className="shrink-0">
        <QRCodeCanvas value={url} size={96} marginSize={2} />
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div className="text-sm font-medium">{rep.name}</div>
        <code className="block truncate rounded bg-muted px-1.5 py-0.5 text-xs" title={url}>
          {url}
        </code>
        <div className="mt-auto flex gap-2">
          <CopyButton value={url} label="Copy link" />
          <Button type="button" variant="outline" size="sm" onClick={download}>
            <Download className="h-4 w-4" />
            QR
          </Button>
        </div>
      </div>
    </div>
  );
}

/** Copies `value` to the clipboard, flipping its label to "Copied" briefly. */
function CopyButton({ value, label = "Copy snippet" }: { value: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <Button
      type="button"
      variant="outline"
      size="sm"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value);
          setCopied(true);
          window.setTimeout(() => setCopied(false), 2000);
        } catch {
          // Clipboard API unavailable (e.g. a non-secure context) — leave the
          // snippet on screen for manual selection.
        }
      }}
    >
      {copied ? (
        <>
          <Check className="h-4 w-4" />
          Copied
        </>
      ) : (
        <>
          <Copy className="h-4 w-4" />
          {label}
        </>
      )}
    </Button>
  );
}
