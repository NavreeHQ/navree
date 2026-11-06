import * as React from "react"

import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"

const ACTIONS = ["Edit", "Duplicate", "Archive"]

export function TabsScreen() {
  const [chosen, setChosen] = React.useState("")

  return (
    <main className="mx-auto w-full max-w-md space-y-6 p-6">
      <h1 className="text-2xl font-semibold">Deal sections</h1>
      <Tabs defaultValue="overview">
        <TabsList>
          <TabsTrigger value="overview">Overview</TabsTrigger>
          <TabsTrigger value="activity">Activity</TabsTrigger>
          <TabsTrigger value="settings">Settings</TabsTrigger>
        </TabsList>
        <TabsContent value="overview">The overview panel.</TabsContent>
        <TabsContent value="activity">The activity panel.</TabsContent>
        <TabsContent value="settings">The settings panel.</TabsContent>
      </Tabs>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button id="cmp-menu-trigger" variant="outline">
            Actions
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          {ACTIONS.map((action) => (
            <DropdownMenuItem
              key={action}
              onSelect={() => setChosen(`Chosen: ${action}`)}
            >
              {action}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
      <p id="cmp-chosen" role="status" className="text-muted-foreground text-sm">
        {chosen || "Nothing chosen yet."}
      </p>
    </main>
  )
}
