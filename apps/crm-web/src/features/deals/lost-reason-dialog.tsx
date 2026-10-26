"use client";

import { useState } from "react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Field, FieldError, FieldLabel } from "@/components/ui/field";
import { Spinner } from "@/components/ui/spinner";
import { Textarea } from "@/components/ui/textarea";
import { errorMessage, isApiError } from "@/lib/api/errors";

const SUGGESTIONS = [
  "Went with a competitor",
  "No budget this year",
  "Price too high",
  "No decision made",
  "Timing: revisit next quarter",
];

/**
 * The API requires `lost_reason` when a deal moves to lost (400 otherwise),
 * so dropping a card on Lost asks for it first.
 */
export function LostReasonDialog({
  dealTitle,
  open,
  onCancel,
  onConfirm,
  pending,
  error,
}: {
  dealTitle: string;
  open: boolean;
  onCancel: () => void;
  onConfirm: (reason: string) => void;
  pending?: boolean;
  error?: unknown;
}) {
  const [reason, setReason] = useState("");
  const [touched, setTouched] = useState(false);
  const serverError =
    isApiError(error) && error.isValidation ? (error.fieldErrors.lost_reason?.[0] ?? errorMessage(error)) : undefined;
  const missing = touched && reason.trim() === "";
  const message = missing ? "Say why the deal was lost; it helps the team's reporting." : serverError;

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          setReason("");
          setTouched(false);
          onCancel();
        }
      }}
    >
      <DialogContent className="sm:max-w-md">
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            setTouched(true);
            if (reason.trim()) onConfirm(reason.trim());
          }}
        >
          <DialogHeader>
            <DialogTitle className="text-lg font-semibold tracking-[-0.02em]">Mark “{dealTitle}” as lost</DialogTitle>
            <DialogDescription>
              Lost deals leave the open pipeline. Add the reason so it shows up in reports.
            </DialogDescription>
          </DialogHeader>
          {error && !serverError ? (
            <Alert variant="destructive">
              <AlertDescription>{errorMessage(error)}</AlertDescription>
            </Alert>
          ) : null}
          <Field data-invalid={Boolean(message) || undefined}>
            <FieldLabel htmlFor="lost-reason">Reason</FieldLabel>
            <Textarea
              id="lost-reason"
              autoFocus
              value={reason}
              onChange={(e) => setReason(e.target.value)}
              aria-invalid={Boolean(message) || undefined}
              placeholder="What made them say no?"
              rows={3}
            />
            {message ? <FieldError>{message}</FieldError> : null}
          </Field>
          <div className="flex flex-wrap gap-1.5" aria-label="Common reasons">
            {SUGGESTIONS.map((s) => (
              <Button key={s} type="button" variant="outline" size="xs" onClick={() => setReason(s)}>
                {s}
              </Button>
            ))}
          </div>
          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => {
                setReason("");
                setTouched(false);
                onCancel();
              }}
            >
              Keep it open
            </Button>
            <Button type="submit" variant="destructive" disabled={pending}>
              {pending ? <Spinner data-icon="inline-start" /> : null}
              Mark as lost
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
