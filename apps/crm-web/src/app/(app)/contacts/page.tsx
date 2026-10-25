import type { Metadata } from "next";
import { Suspense } from "react";
import { ContactsView } from "./contacts-view";

export const metadata: Metadata = { title: "Contacts" };

export default function ContactsPage() {
  return (
    <Suspense>
      <ContactsView />
    </Suspense>
  );
}
