import type { Metadata } from "next";
import { UsersSettings } from "./users-settings";

export const metadata: Metadata = { title: "Users and roles" };

export default function UsersSettingsPage() {
  return <UsersSettings />;
}
