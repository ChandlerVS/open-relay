import { useState } from "react";
import { Link, useParams } from "react-router-dom";
import { ArrowLeft, TriangleAlert } from "lucide-react";
import { ShadowForm } from "@open-relay/form-renderer";
import {
  Alert,
  AlertDescription,
  AlertTitle,
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  Skeleton,
} from "@open-relay/ui";
import { api } from "../../../lib/api/client";
import { useForm } from "../../../lib/forms/useForms";
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
    </div>
  );
}
