"use client";

import { Can } from "@casl/react";
import { DownloadIcon, PlusIcon, UsersIcon, XIcon } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { toast } from "sonner";
import { ListEmpty, PastEndNotice, SkeletonRows, TablePanel } from "@/components/data-table";
import { OptionFilter, OwnerFilter, SearchFilter } from "@/components/filters";
import { PageHeader } from "@/components/page-header";
import { PaginationBar } from "@/components/pagination-bar";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { OwnerCell } from "@/components/user-avatar";
import { useCurrentUser } from "@/features/auth/queries";
import { useCompany } from "@/features/companies/queries";
import { ContactFormDialog, STATUS_LABEL } from "@/features/contacts/contact-form";
import { exportContactsCsv, useContacts, type ContactListParams } from "@/features/contacts/queries";
import { ContactStatusBadge } from "@/features/contacts/status-badge";
import { useUrlState } from "@/hooks/use-url-state";
import { errorMessage } from "@/lib/api/errors";
import type { ContactStatus } from "@/lib/api/types";
import { CONTACT_STATUSES } from "@/lib/api/types";
import { formatNumber } from "@/lib/format";

function NewContactButton() {
  return (
    <Can I="create" a="Contact">
      <ContactFormDialog
        trigger={
          <Button>
            <PlusIcon data-icon="inline-start" />
            New contact
          </Button>
        }
      />
    </Can>
  );
}

/** CSV export: only rendered for people the API lets export (admin, manager). */
function ExportButton({ filters }: { filters: ContactListParams }) {
  const [pending, setPending] = useState(false);
  return (
    <Can I="export" a="Contact">
      <Button
        variant="outline"
        disabled={pending}
        onClick={async () => {
          setPending(true);
          try {
            const { search, company_id, status, owner_id } = filters;
            await exportContactsCsv({ search, company_id, status, owner_id });
            toast.success("Contacts exported");
          } catch (error) {
            toast.error(errorMessage(error, "The export failed. Try again."));
          } finally {
            setPending(false);
          }
        }}
      >
        {pending ? <Spinner data-icon="inline-start" /> : <DownloadIcon data-icon="inline-start" />}
        Export CSV
      </Button>
    </Can>
  );
}

function CompanyChip({ id, onClear }: { id: string; onClear: () => void }) {
  const company = useCompany(id);
  return (
    <span className="inline-flex h-8 items-center gap-1 rounded-lg border bg-card pr-1 pl-2.5 text-sm">
      <span className="text-muted-foreground">Company:</span>
      <span className="font-medium">{company.data?.name ?? "…"}</span>
      <Button variant="ghost" size="icon-xs" aria-label="Clear company filter" onClick={onClear}>
        <XIcon />
      </Button>
    </span>
  );
}

export function ContactsView() {
  const router = useRouter();
  const url = useUrlState();
  const me = useCurrentUser();
  const status = url.get("status") as ContactStatus | undefined;
  const params: ContactListParams = {
    search: url.get("search"),
    status: status && CONTACT_STATUSES.includes(status) ? status : undefined,
    owner_id: url.get("owner"),
    company_id: url.get("company"),
    page: url.page,
    per_page: 25,
  };
  const contacts = useContacts(params);
  const filtered = Boolean(params.search || params.status || params.owner_id || params.company_id);
  const total = contacts.data?.meta.total;

  return (
    <div className="flex flex-col">
      <PageHeader
        title="Contacts"
        description={
          total === undefined
            ? "People at the companies you sell to"
            : `${formatNumber(total)} ${total === 1 ? "contact" : "contacts"}${filtered ? " match" : ""}`
        }
        actions={
          <>
            <ExportButton filters={params} />
            <NewContactButton />
          </>
        }
      />
      <TablePanel>
        <div className="flex flex-wrap items-center gap-2 border-b p-3">
          <SearchFilter
            value={params.search}
            onChange={(search) => url.set({ search })}
            placeholder="Search by name, email or title"
          />
          <OptionFilter
            value={params.status}
            onChange={(s) => url.set({ status: s })}
            options={CONTACT_STATUSES.map((s) => ({ value: s, label: STATUS_LABEL[s] }))}
            anyLabel="Any status"
            ariaLabel="Filter by status"
          />
          <OwnerFilter value={params.owner_id} onChange={(owner) => url.set({ owner })} currentUserId={me.data?.id} />
          {params.company_id ? (
            <CompanyChip id={params.company_id} onClear={() => url.set({ company: undefined })} />
          ) : null}
        </div>
        <Table className="text-[13px]">
          <TableHeader>
            <TableRow className="hover:bg-transparent [&>th]:h-9 [&>th]:text-xs [&>th]:font-medium [&>th]:text-muted-foreground">
              <TableHead className="pl-4">Name</TableHead>
              <TableHead>Company</TableHead>
              <TableHead>Email</TableHead>
              <TableHead className="hidden xl:table-cell">Phone</TableHead>
              <TableHead>Status</TableHead>
              <TableHead className="pr-4">Owner</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {contacts.isPending ? (
              <SkeletonRows columns={6} />
            ) : contacts.data?.data.length ? (
              contacts.data.data.map((p) => (
                <TableRow
                  key={p.id}
                  className="cursor-pointer"
                  onClick={(e) => {
                    if (!(e.target as HTMLElement).closest("a,button")) router.push(`/contacts/${p.id}`);
                  }}
                >
                  <TableCell className="py-2.5 pl-4">
                    <Link href={`/contacts/${p.id}`} className="font-medium hover:underline">
                      {p.full_name}
                    </Link>
                    {p.title ? <div className="text-xs text-muted-foreground">{p.title}</div> : null}
                  </TableCell>
                  <TableCell>
                    {p.company ? (
                      <Link href={`/companies/${p.company.id}`} className="hover:underline">
                        {p.company.name}
                      </Link>
                    ) : (
                      <span className="text-muted-foreground">No company</span>
                    )}
                  </TableCell>
                  <TableCell className="max-w-56 truncate">
                    {p.email ? (
                      <a href={`mailto:${p.email}`} className="hover:underline">
                        {p.email}
                      </a>
                    ) : (
                      <span className="text-muted-foreground">Not set</span>
                    )}
                  </TableCell>
                  <TableCell className="hidden whitespace-nowrap figures xl:table-cell">
                    {p.phone ?? <span className="text-muted-foreground">Not set</span>}
                  </TableCell>
                  <TableCell>
                    <ContactStatusBadge status={p.status} />
                  </TableCell>
                  <TableCell className="pr-4">
                    <OwnerCell owner={p.owner} />
                  </TableCell>
                </TableRow>
              ))
            ) : (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={6}>
                  {contacts.data && contacts.data.meta.total > 0 ? (
                    <PastEndNotice
                      noun="contacts"
                      total={contacts.data.meta.total}
                      onFirstPage={() => url.set({ page: undefined })}
                    />
                  ) : (
                    <ListEmpty
                      filtered={filtered}
                      noun="contacts"
                      icon={<UsersIcon />}
                      onClear={() =>
                        url.set({ search: undefined, status: undefined, owner: undefined, company: undefined })
                      }
                      action={<NewContactButton />}
                    />
                  )}
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
        <PaginationBar meta={contacts.data?.meta} onPageChange={(page) => url.set({ page })} noun="contacts" />
      </TablePanel>
    </div>
  );
}
