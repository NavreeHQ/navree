"use client";

import { Can } from "@navree/next/abilities";
import { BriefcaseIcon, Building2Icon, MailIcon, PencilIcon, PhoneIcon, Trash2Icon } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { Money } from "@/components/money";
import { DetailRow, Panel } from "@/components/page-header";
import { MetaItem, RecordHeader, RecordMissing, RecordSkeleton } from "@/components/record-header";
import { StageBadge } from "@/components/stage";
import { Button } from "@/components/ui/button";
import { OwnerCell } from "@/components/user-avatar";
import { RecordActivity } from "@/features/activities/components";
import { ContactFormDialog } from "@/features/contacts/contact-form";
import { useContact, useDeleteContact } from "@/features/contacts/queries";
import { ContactStatusBadge } from "@/features/contacts/status-badge";
import { useDeals } from "@/features/deals/queries";
import type { Contact } from "@/lib/api/types";
import { formatDateLong } from "@/lib/format";

const BACK = { href: "/contacts", label: "Contacts" };

/** Deals at the contact's company where this person is the main contact. */
function ContactDeals({ contact }: { contact: Contact }) {
  const deals = useDeals(
    { filter: { company_id: contact.company_id ?? undefined }, per_page: 100 },
    Boolean(contact.company_id),
  );
  const mine = deals.data?.data.filter((d) => d.contact_id === contact.id) ?? [];
  if (!contact.company_id)
    return <p className="text-sm text-muted-foreground">Link this contact to a company to track deals.</p>;
  if (deals.isPending) return <p className="text-sm text-muted-foreground">Loading deals…</p>;
  if (mine.length === 0) return <p className="text-sm text-muted-foreground">Not the main contact on any deal.</p>;
  return (
    <ul className="-my-1 flex flex-col divide-y">
      {mine.map((d) => (
        <li key={d.id} className="flex items-center justify-between gap-3 py-2 text-sm">
          <div className="flex min-w-0 flex-col gap-1">
            <Link href={`/deals/${d.id}`} className="truncate font-medium hover:underline">
              {d.title}
            </Link>
            <StageBadge stage={d.stage} className="w-fit" />
          </div>
          <Money cents={d.amount_cents} currency={d.currency} className="font-medium" />
        </li>
      ))}
    </ul>
  );
}

export function ContactDetail({ id }: { id: string }) {
  const router = useRouter();
  const contact = useContact(id);
  const remove = useDeleteContact();

  if (contact.isPending) return <RecordSkeleton />;
  if (contact.isError) return <RecordMissing error={contact.error} noun="contact" back={BACK} />;
  const p = contact.data;

  return (
    <div className="flex flex-col">
      <RecordHeader
        title={p.full_name}
        eyebrow={<ContactStatusBadge status={p.status} className="w-fit" />}
        meta={
          <>
            {p.title ? <MetaItem icon={BriefcaseIcon}>{p.title}</MetaItem> : null}
            {p.company ? (
              <MetaItem icon={Building2Icon}>
                <Link href={`/companies/${p.company.id}`} className="transition-colors hover:text-fg hover:underline">
                  {p.company.name}
                </Link>
              </MetaItem>
            ) : null}
            {p.email ? (
              <MetaItem icon={MailIcon}>
                <a href={`mailto:${p.email}`} className="transition-colors hover:text-fg hover:underline">
                  {p.email}
                </a>
              </MetaItem>
            ) : null}
            {p.phone ? (
              <MetaItem icon={PhoneIcon}>
                <a href={`tel:${p.phone}`} className="figures hover:text-foreground hover:underline">
                  {p.phone}
                </a>
              </MetaItem>
            ) : null}
          </>
        }
        actions={
          <>
            <Can I="update" this={p}>
              <ContactFormDialog
                contact={p}
                trigger={
                  <Button variant="outline">
                    <PencilIcon data-icon="inline-start" />
                    Edit
                  </Button>
                }
              />
            </Can>
            <Can I="delete" this={p}>
              <ConfirmDialog
                trigger={
                  <Button variant="ghost" size="icon" aria-label="Delete contact">
                    <Trash2Icon />
                  </Button>
                }
                title={`Delete ${p.full_name}?`}
                description="The contact is removed from lists and search. Their activity history is kept."
                confirmLabel="Delete contact"
                pending={remove.isPending}
                onConfirm={() =>
                  remove.mutate(p.id, {
                    onSuccess: () => {
                      toast.success(`${p.full_name} deleted`);
                      router.push("/contacts");
                    },
                  })
                }
              />
            </Can>
          </>
        }
      />
      <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_20rem]">
        <Panel title="Activity" className="min-w-0">
          <RecordActivity subjectType="contact" subjectId={p.id} />
        </Panel>
        <div className="flex flex-col gap-4">
          <Panel title="Details" bodyClassName="py-2">
            <dl>
              <DetailRow label="Owner">
                <OwnerCell owner={p.owner} />
              </DetailRow>
              <DetailRow label="Status">
                <ContactStatusBadge status={p.status} />
              </DetailRow>
              <DetailRow label="Company">
                {p.company ? (
                  <Link href={`/companies/${p.company.id}`} className="record-link">
                    {p.company.name}
                  </Link>
                ) : null}
              </DetailRow>
              <DetailRow label="Job title">{p.title}</DetailRow>
              <DetailRow label="Email">{p.email}</DetailRow>
              <DetailRow label="Phone">{p.phone}</DetailRow>
              <DetailRow label="Added">{formatDateLong(p.created_at)}</DetailRow>
              <DetailRow label="Last updated">{formatDateLong(p.updated_at)}</DetailRow>
            </dl>
          </Panel>
          <Panel title="Deals">
            <ContactDeals contact={p} />
          </Panel>
        </div>
      </div>
    </div>
  );
}
