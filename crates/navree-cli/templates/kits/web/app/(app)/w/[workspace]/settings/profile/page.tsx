"use client";

import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { useForm, useWatch } from "react-hook-form";
import { toast } from "sonner";
import { z } from "zod";
import { FormAlert, TextField } from "@/components/forms";
import { PageHeader } from "@/components/page-header";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { FieldGroup } from "@/components/ui/field";
import { Spinner } from "@/components/ui/spinner";
import { api } from "@/lib/api/client";
import { applyFieldErrors, errorMessage } from "@/lib/api/errors";
import type { User } from "@/lib/api/types";
import { keys, useMe } from "@/lib/queries";

const schema = z.object({
  name: z.string().trim().min(1, "Enter your name.").max(100),
  email: z.email("Enter your email address."),
  current_password: z.string(),
});
type Values = z.infer<typeof schema>;

/** The signed-in person's name and email address (a new address is verified again). */
export default function ProfilePage() {
  const me = useMe();
  const queryClient = useQueryClient();
  const form = useForm<Values>({
    resolver: zodResolver(schema),
    defaultValues: { name: "", email: "", current_password: "" },
  });
  const { reset } = form;
  useEffect(() => {
    if (me.data) reset({ name: me.data.name, email: me.data.email, current_password: "" });
  }, [me.data, reset]);
  const email = useWatch({ control: form.control, name: "email" });
  const emailChanged = Boolean(me.data && email !== me.data.email);

  const save = useMutation({
    mutationFn: (values: Values) =>
      api.patch<User>(
        "/api/auth/me",
        {
          name: values.name,
          // Changing the address needs the password.
          ...(emailChanged ? { email: values.email, current_password: values.current_password } : {}),
        },
        { workspace: null },
      ),
    onSuccess: async (user) => {
      queryClient.setQueryData(keys.me, user);
      toast.success(emailChanged ? "Saved. Check your inbox to verify the new address." : "Saved.");
    },
    onError: (error) => {
      if (!applyFieldErrors(error, form.setError)) {
        form.setError("root.server", { type: "server", message: errorMessage(error) });
      }
    },
    meta: { silent: true },
  });

  return (
    <>
      <PageHeader title="Profile" description="How your teammates see you." />
      <Card>
        <CardContent>
          <form onSubmit={form.handleSubmit((values) => save.mutate(values))} noValidate className="flex flex-col gap-6">
            <FormAlert message={form.formState.errors.root?.server?.message} />
            <FieldGroup>
              <TextField control={form.control} name="name" label="Name" autoComplete="name" />
              <TextField control={form.control} name="email" label="Email" type="email" autoComplete="email" />
              {emailChanged ? (
                <TextField
                  control={form.control}
                  name="current_password"
                  label="Current password"
                  type="password"
                  autoComplete="current-password"
                  description="Needed to change your email address."
                />
              ) : null}
            </FieldGroup>
            <Button type="submit" className="self-start" disabled={save.isPending || !form.formState.isDirty}>
              {save.isPending ? <Spinner data-icon="inline-start" /> : null}
              Save changes
            </Button>
          </form>
        </CardContent>
      </Card>
    </>
  );
}
