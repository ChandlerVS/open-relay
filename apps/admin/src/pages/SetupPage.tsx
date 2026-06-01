import { useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { useMutation } from "@tanstack/react-query";
import { Link, useNavigate } from "react-router-dom";
import {
  Alert,
  AlertDescription,
  AlertTitle,
  Button,
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
  FormField,
  Input,
} from "@open-relay/ui";
import { api } from "../lib/api/client";
import { extractApiErrorMessage } from "../lib/api/errors";
import { useAuth } from "../lib/auth/useAuth";

const schema = z
  .object({
    display_name: z
      .string()
      .trim()
      .max(255, "255 characters max.")
      .optional()
      .or(z.literal("")),
    email: z.string().email("Enter a valid email address."),
    password: z.string().min(12, "Minimum 12 characters."),
    confirm: z.string(),
  })
  .refine((d) => d.password === d.confirm, {
    path: ["confirm"],
    message: "Passwords don't match.",
  });

type FormValues = z.infer<typeof schema>;

export function SetupPage() {
  const navigate = useNavigate();
  const { signIn } = useAuth();
  const [formError, setFormError] = useState<string | null>(null);

  const {
    register,
    handleSubmit,
    formState: { errors },
  } = useForm<FormValues>({ resolver: zodResolver(schema) });

  const mutation = useMutation({
    mutationFn: async (values: FormValues) => {
      const { data, error, response } = await api.client.POST("/setup/initialize", {
        body: {
          email: values.email,
          password: values.password,
          display_name: values.display_name?.trim() ? values.display_name.trim() : null,
        },
      });
      if (data) return data;
      if (response.status === 409) {
        throw new Error("An admin account already exists. Sign in instead.");
      }
      // 400 carries a useful validation message; everything else gets a generic fallback.
      throw new Error(extractApiErrorMessage(error, "Couldn't create the admin account."));
    },
    onSuccess: (data) => {
      signIn(data.token, data.refresh_token, data.user);
      navigate("/", { replace: true });
    },
    onError: (err: Error) => setFormError(err.message),
  });

  return (
    <main className="min-h-screen grid place-items-center bg-background text-foreground p-6">
      <Card className="w-full max-w-md">
        <CardHeader>
          <div className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
            First-time setup
          </div>
          <CardTitle>Create your admin account</CardTitle>
          <CardDescription>
            This will be the initial OpenRelay administrator. You'll be able to add more users
            later.
          </CardDescription>
        </CardHeader>
        <form
          onSubmit={handleSubmit((values) => {
            setFormError(null);
            mutation.mutate(values);
          })}
          noValidate
        >
          <CardContent className="space-y-4">
            {formError && (
              <Alert variant="destructive">
                <AlertTitle>Setup failed</AlertTitle>
                <AlertDescription>
                  {formError}{" "}
                  {formError.toLowerCase().includes("already") && (
                    <Link to="/login" className="underline font-medium">
                      Go to sign in
                    </Link>
                  )}
                </AlertDescription>
              </Alert>
            )}
            <FormField id="display_name" label="Display name (optional)" error={errors.display_name?.message}>
              <Input autoComplete="name" placeholder="Ada Lovelace" {...register("display_name")} />
            </FormField>
            <FormField id="email" label="Email" error={errors.email?.message}>
              <Input
                type="email"
                autoComplete="email"
                placeholder="admin@example.com"
                {...register("email")}
              />
            </FormField>
            <FormField
              id="password"
              label="Password"
              hint="At least 12 characters."
              error={errors.password?.message}
            >
              <Input type="password" autoComplete="new-password" {...register("password")} />
            </FormField>
            <FormField id="confirm" label="Confirm password" error={errors.confirm?.message}>
              <Input type="password" autoComplete="new-password" {...register("confirm")} />
            </FormField>
          </CardContent>
          <CardFooter className="flex flex-col items-stretch gap-2">
            <Button type="submit" disabled={mutation.isPending}>
              {mutation.isPending ? "Creating…" : "Create account"}
            </Button>
          </CardFooter>
        </form>
      </Card>
    </main>
  );
}
