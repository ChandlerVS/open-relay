import { useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { useMutation } from "@tanstack/react-query";
import { useLocation, useNavigate } from "react-router-dom";
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
import { useOAuthPublicConfig } from "../lib/oauth/useOAuth";

const schema = z.object({
  email: z.string().email("Enter a valid email address."),
  password: z.string().min(1, "Password is required."),
});

type FormValues = z.infer<typeof schema>;

interface LocationState {
  from?: { pathname: string };
}

export function LoginPage() {
  const navigate = useNavigate();
  const location = useLocation();
  const { signIn } = useAuth();
  const oauth = useOAuthPublicConfig();
  const [formError, setFormError] = useState<string | null>(null);

  const {
    register,
    handleSubmit,
    formState: { errors },
  } = useForm<FormValues>({ resolver: zodResolver(schema) });

  const mutation = useMutation({
    mutationFn: async (values: FormValues) => {
      const { data, error, response } = await api.client.POST("/auth/login", {
        body: values,
      });
      if (data) return data;
      // 401 is the only expected failure; the backend body says "unauthorized"
      // which is unhelpful for end users.
      if (response.status === 401) throw new Error("Invalid email or password.");
      throw new Error(extractApiErrorMessage(error, "Sign-in failed."));
    },
    onSuccess: (data) => {
      signIn(data.token, data.refresh_token, data.user);
      const from = (location.state as LocationState | null)?.from?.pathname ?? "/";
      navigate(from, { replace: true });
    },
    onError: (err: Error) => setFormError(err.message),
  });

  return (
    <main className="min-h-screen grid place-items-center bg-background text-foreground p-6">
      <Card className="w-full max-w-md">
        <CardHeader>
          <CardTitle>Sign in</CardTitle>
          <CardDescription>Access the OpenRelay admin panel.</CardDescription>
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
                <AlertTitle>Sign-in failed</AlertTitle>
                <AlertDescription>{formError}</AlertDescription>
              </Alert>
            )}
            <FormField id="email" label="Email" error={errors.email?.message}>
              <Input
                type="email"
                autoComplete="email"
                placeholder="you@example.com"
                {...register("email")}
              />
            </FormField>
            <FormField id="password" label="Password" error={errors.password?.message}>
              <Input type="password" autoComplete="current-password" {...register("password")} />
            </FormField>
          </CardContent>
          <CardFooter className="flex flex-col gap-3">
            <Button type="submit" disabled={mutation.isPending} className="w-full">
              {mutation.isPending ? "Signing in…" : "Sign in"}
            </Button>
            {oauth.data?.enabled && oauth.data.display_name && (
              <>
                <div className="relative w-full">
                  <div className="absolute inset-0 flex items-center">
                    <span className="w-full border-t border-border" />
                  </div>
                  <div className="relative flex justify-center text-xs uppercase tracking-wider">
                    <span className="bg-card px-2 text-muted-foreground">or</span>
                  </div>
                </div>
                <Button
                  type="button"
                  variant="outline"
                  className="w-full"
                  onClick={() => {
                    window.location.href = `${api.baseUrl}/auth/oauth/start`;
                  }}
                >
                  Sign in with {oauth.data.display_name}
                </Button>
              </>
            )}
          </CardFooter>
        </form>
      </Card>
    </main>
  );
}
