"use client";

import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { useForm } from "react-hook-form";
import { toast } from "sonner";
import { z } from "zod";
import { FormAlert, TextField } from "@/components/forms";
import { PageHeader } from "@/components/page-header";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { FieldGroup } from "@/components/ui/field";
import { Spinner } from "@/components/ui/spinner";
import { api } from "@/lib/api/client";
import { applyFieldErrors, errorMessage } from "@/lib/api/errors";
import type { DataList, RecoveryCodes, Session, TwoFactorSetup } from "@/lib/api/types";
import { keys, useMe } from "@/lib/queries";

const passwordSchema = z.object({
  current_password: z.string().min(1, "Enter your current password."),
  password: z.string().min(8, "Use at least 8 characters."),
});
type PasswordValues = z.infer<typeof passwordSchema>;

/** Changing the password signs every other session out; this one gets fresh tokens. */
function PasswordCard() {
  const form = useForm<PasswordValues>({
    resolver: zodResolver(passwordSchema),
    defaultValues: { current_password: "", password: "" },
  });
  const change = useMutation({
    mutationFn: (values: PasswordValues) => api.patch("/api/auth/me/password", values, { workspace: null }),
    onSuccess: () => {
      form.reset();
      toast.success("Password changed. Your other sessions were signed out.");
    },
    onError: (error) => {
      if (!applyFieldErrors(error, form.setError)) {
        form.setError("root.server", { type: "server", message: errorMessage(error) });
      }
    },
    meta: { silent: true },
  });

  return (
    <Card>
      <CardHeader>
        <CardTitle>Password</CardTitle>
      </CardHeader>
      <CardContent>
        <form onSubmit={form.handleSubmit((values) => change.mutate(values))} noValidate className="flex flex-col gap-6">
          <FormAlert message={form.formState.errors.root?.server?.message} />
          <FieldGroup>
            <TextField
              control={form.control}
              name="current_password"
              label="Current password"
              type="password"
              autoComplete="current-password"
            />
            <TextField
              control={form.control}
              name="password"
              label="New password"
              type="password"
              autoComplete="new-password"
              description="At least 8 characters."
            />
          </FieldGroup>
          <Button type="submit" className="self-start" disabled={change.isPending}>
            {change.isPending ? <Spinner data-icon="inline-start" /> : null}
            Change password
          </Button>
        </form>
      </CardContent>
    </Card>
  );
}

/** Recovery codes, shown once: each signs in once when the phone is lost. */
function RecoveryCodeList({ codes }: { codes: string[] }) {
  return (
    <div className="flex flex-col gap-2">
      <p className="text-sm text-fg-2">Save these recovery codes somewhere safe. Each one works once.</p>
      <ul className="grid grid-cols-2 gap-1 rounded-md border bg-ink-0 p-3 font-mono text-sm text-fg-1">
        {codes.map((code) => (
          <li key={code}>{code}</li>
        ))}
      </ul>
    </div>
  );
}

/** Two-factor authentication with an authenticator app (TOTP). */
function TwoFactorCard({ enabled }: { enabled: boolean }) {
  const queryClient = useQueryClient();
  const [setup, setSetup] = useState<TwoFactorSetup>();
  const [codes, setCodes] = useState<string[]>();
  const [code, setCode] = useState("");
  const [password, setPassword] = useState("");
  const [failure, setFailure] = useState<string>();
  const done = () => queryClient.invalidateQueries({ queryKey: keys.me });

  const enable = useMutation({
    mutationFn: () => api.post<TwoFactorSetup>("/api/auth/2fa/enable", undefined, { workspace: null }),
    onSuccess: (result) => {
      setFailure(undefined);
      setSetup(result);
    },
    onError: (error) => setFailure(errorMessage(error)),
    meta: { silent: true },
  });
  const confirm = useMutation({
    mutationFn: () =>
      api.post<RecoveryCodes>("/api/auth/2fa/confirm", { code: code.replace(/\s+/g, "") }, { workspace: null }),
    onSuccess: async (result) => {
      setSetup(undefined);
      setCode("");
      setCodes(result.recovery_codes);
      await done();
    },
    onError: (error) => setFailure(errorMessage(error)),
    meta: { silent: true },
  });
  const disable = useMutation({
    mutationFn: () => api.post("/api/auth/2fa/disable", { password }, { workspace: null }),
    onSuccess: async () => {
      setPassword("");
      setCodes(undefined);
      await done();
    },
    onError: (error) => setFailure(errorMessage(error)),
    meta: { silent: true },
  });
  const regenerate = useMutation({
    mutationFn: () => api.post<RecoveryCodes>("/api/auth/2fa/recovery-codes", undefined, { workspace: null }),
    onSuccess: (result) => setCodes(result.recovery_codes),
    onError: (error) => setFailure(errorMessage(error)),
    meta: { silent: true },
  });

  return (
    <Card>
      <CardHeader>
        <CardTitle>Two-step verification</CardTitle>
        <CardDescription>
          {enabled
            ? "On: signing in asks for a code from your authenticator app."
            : "Ask for a code from an authenticator app when you sign in."}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <FormAlert message={failure} />
        {codes ? <RecoveryCodeList codes={codes} /> : null}
        {!enabled && !setup ? (
          <Button className="self-start" onClick={() => enable.mutate()} disabled={enable.isPending}>
            Turn on
          </Button>
        ) : null}
        {setup ? (
          <div className="flex flex-col gap-4">
            <p className="text-sm text-fg-2">Scan this code with your authenticator app, then enter the code it shows.</p>
            {/* The API renders the QR code as SVG markup. */}
            <div
              className="w-44 rounded-md bg-white p-2"
              aria-label="QR code for your authenticator app"
              // The API renders the QR code as SVG markup from the one-time setup URI, not from user text.
              // nosemgrep: navree-raw-html
              dangerouslySetInnerHTML={{ __html: setup.qr_svg }}
            />
            <p className="text-xs text-fg-3">
              Or enter this key: <code className="text-fg-1">{setup.secret}</code>
            </p>
            <form
              className="flex flex-wrap items-end gap-2"
              onSubmit={(event) => {
                event.preventDefault();
                confirm.mutate();
              }}
            >
              <input
                className="h-8 w-40 rounded-md border border-line-2 bg-ink-1 px-2 text-sm text-fg"
                inputMode="numeric"
                autoComplete="one-time-code"
                aria-label="Code from the app"
                value={code}
                onChange={(event) => setCode(event.target.value)}
              />
              <Button type="submit" disabled={confirm.isPending || !code}>
                Confirm
              </Button>
            </form>
          </div>
        ) : null}
        {enabled ? (
          <div className="flex flex-wrap items-end gap-2">
            <Button variant="outline" onClick={() => regenerate.mutate()} disabled={regenerate.isPending}>
              New recovery codes
            </Button>
            <form
              className="flex flex-wrap items-end gap-2"
              onSubmit={(event) => {
                event.preventDefault();
                disable.mutate();
              }}
            >
              <input
                className="h-8 w-48 rounded-md border border-line-2 bg-ink-1 px-2 text-sm text-fg"
                type="password"
                autoComplete="current-password"
                placeholder="Your password"
                aria-label="Your password, to turn it off"
                value={password}
                onChange={(event) => setPassword(event.target.value)}
              />
              <Button type="submit" variant="destructive" disabled={disable.isPending || !password}>
                Turn off
              </Button>
            </form>
          </div>
        ) : null}
      </CardContent>
    </Card>
  );
}

/** Where the person is signed in; ending a session stops it from refreshing its tokens. */
function SessionsCard() {
  const router = useRouter();
  const queryClient = useQueryClient();
  const sessions = useQuery({
    queryKey: keys.sessions,
    queryFn: async () => (await api.get<DataList<Session>>("/api/v1/account/sessions", { workspace: null })).data,
  });
  const end = useMutation({
    mutationFn: (id: string) => api.delete(`/api/v1/account/sessions/${id}`, { workspace: null }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: keys.sessions }),
  });
  const everywhere = useMutation({
    mutationFn: () => api.post("/api/auth/logout", { all: true }, { workspace: null }),
    onSuccess: () => {
      queryClient.clear();
      router.replace("/sign-in");
    },
  });

  return (
    <Card>
      <CardHeader>
        <CardTitle>Sessions</CardTitle>
        <CardDescription>The browsers and devices signed in to your account.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <ul className="flex flex-col divide-y">
          {(sessions.data ?? []).map((session) => (
            <li key={session.id} className="flex items-center justify-between gap-4 py-2">
              <div className="flex min-w-0 flex-col">
                <span className="truncate text-sm text-fg">{session.user_agent ?? "Unknown device"}</span>
                <span className="text-xs text-fg-3">
                  {session.ip ?? "Unknown address"}, active {new Date(session.last_active_at).toLocaleString()}
                </span>
              </div>
              <Button variant="ghost" size="sm" onClick={() => end.mutate(session.id)} disabled={end.isPending}>
                Sign out
              </Button>
            </li>
          ))}
        </ul>
        <Button variant="outline" className="self-start" onClick={() => everywhere.mutate()}>
          Sign out everywhere
        </Button>
      </CardContent>
    </Card>
  );
}

/** Password, two-step verification and sessions. */
export default function SecurityPage() {
  const me = useMe();
  return (
    <>
      <PageHeader title="Password and security" />
      <PasswordCard />
      <TwoFactorCard enabled={Boolean(me.data?.two_factor_enabled)} />
      <SessionsCard />
    </>
  );
}
