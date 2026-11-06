import * as React from "react"
import { format } from "date-fns"
import { CalendarIcon } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Calendar } from "@/components/ui/calendar"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"

// Today is fixed for the comparison: the calendar opens on November 2026.
const OPENS_ON = new Date(2026, 10, 3)

export function DatesScreen() {
  const [date, setDate] = React.useState<Date | undefined>()
  const [open, setOpen] = React.useState(false)

  return (
    <main className="mx-auto w-full max-w-md space-y-6 p-6">
      <h1 className="text-2xl font-semibold">Choose a date</h1>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            id="cmp-date-trigger"
            variant="outline"
            className="w-[240px] justify-start text-left font-normal"
          >
            <CalendarIcon />
            {date ? format(date, "MM/dd/yyyy") : "Pick a date"}
          </Button>
        </PopoverTrigger>
        <PopoverContent className="w-auto p-0" align="start">
          <Calendar
            mode="single"
            selected={date}
            defaultMonth={OPENS_ON}
            onSelect={(chosen) => {
              setDate(chosen)
              setOpen(false)
            }}
          />
        </PopoverContent>
      </Popover>
    </main>
  )
}
