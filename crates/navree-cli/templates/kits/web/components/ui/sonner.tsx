"use client"

import { Toaster as Sonner, type ToasterProps } from "sonner"
import { CircleCheckIcon, InfoIcon, TriangleAlertIcon, OctagonXIcon, Loader2Icon } from "lucide-react"

/** Toasts on an ink-2 card (6px corners) with a line-2 edge; the icon carries the state (green, amber, red). */
const Toaster = ({ ...props }: ToasterProps) => {
  return (
    <Sonner
      theme="dark"
      className="toaster group"
      icons={{
        success: <CircleCheckIcon className="size-4 text-accent" />,
        info: <InfoIcon className="size-4 text-fg-2" />,
        warning: <TriangleAlertIcon className="size-4 text-amber" />,
        error: <OctagonXIcon className="size-4 text-danger" />,
        loading: <Loader2Icon className="size-4 animate-spin text-fg-2" />,
      }}
      style={
        {
          "--normal-bg": "var(--color-ink-2)",
          "--normal-text": "var(--color-fg)",
          "--normal-border": "var(--color-line-2)",
          "--border-radius": "6px",
        } as React.CSSProperties
      }
      toastOptions={{
        classNames: {
          toast: "cn-toast !font-sans !shadow-float",
          title: "!font-medium",
          description: "!text-fg-2",
          closeButton: "!rounded-sm !border-line-2 !bg-ink-3 !text-fg-2 hover:!text-fg",
        },
      }}
      {...props}
    />
  )
}

export { Toaster }
