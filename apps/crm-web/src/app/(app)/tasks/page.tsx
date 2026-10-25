import type { Metadata } from "next";
import { Suspense } from "react";
import { TasksView } from "./tasks-view";

export const metadata: Metadata = { title: "Tasks" };

export default function TasksPage() {
  return (
    <Suspense>
      <TasksView />
    </Suspense>
  );
}
