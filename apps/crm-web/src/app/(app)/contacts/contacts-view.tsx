"use client";

import { Can } from "@navree/next/abilities";
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

function NewContactButton({ openOnCreateIntent }: { openOnCreateIntent?: boolean }) {
  return (
    <Can I="create" a="Contact">
      <ContactFormDialog
        openOnCreateIntent={openOnCreateIntent}
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
function ExportButton({ query }: { query: ContactListParams }) {
  const [pending, setPending] = useState(false);
  return (
    <Can I="export" a="Contact">
      <Button
        variant="outline"
        disabled={pending}
        onClick={async () => {
          setPending(true);
          try {
            await exportContactsCsv({ filter: query.filter, search: query.search });
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
    <span className="inline-flex h-8 items-center gap-1 rounded-md border border-line-2 bg-ink-2 pr-1 pl-2.5 text-sm">
      <span className="text-fg-3">Company:</span>
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
    filter: {
      status: status && CONTACT_STATUSES.includes(status) ? status : undefined,
      owner_id: url.get("owner"),
      company_id: url.get("company"),
    },
    page: url.page,
    per_page: 25,
  };
  const contacts = useContacts(params);
  const filtered = Boolean(
    params.search || params.filter?.status || params.filter?.owner_id || params.filter?.company_id,
  );
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
            <ExportButton query={params} />
            <NewContactButton openOnCreateIntent />
          </>
        }
      />
      <TablePanel>
        <div className="flex flex-wrap items-center gap-2 border-b p-3" role="search" aria-label="Filter contacts">
          <SearchFilter
            value={params.search}
            onChange={(search) => url.set({ search })}
            placeholder="Search by name, email or title"
          />
          <OptionFilter
            value={params.filter?.status}
            onChange={(s) => url.set({ status: s })}
            options={CONTACT_STATUSES.map((s) => ({ value: s, label: STATUS_LABEL[s] }))}
            anyLabel="Any status"
            ariaLabel="Filter by status"
          />
          <OwnerFilter
            value={params.filter?.owner_id}
            onChange={(owner) => url.set({ owner })}
            currentUserId={me.data?.id}
          />
          {params.filter?.company_id ? (
            <CompanyChip id={params.filter.company_id} onClear={() => url.set({ company: undefined })} />
          ) : null}
        </div>
        <Table className="text-[13px]">
          <TableHeader>
            <TableRow className="bg-ink-1 hover:bg-ink-1 [&>th]:h-9">
              <TableHead className="pl-4">Name</TableHead>
              <TableHead className="hidden lg:table-cell">Title</TableHead>
              <TableHead className="hidden sm:table-cell">Company</TableHead>
              <TableHead className="hidden xl:table-cell">Email</TableHead>
              <TableHead className="hidden 2xl:table-cell">Phone</TableHead>
              <TableHead>Status</TableHead>
              <TableHead className="hidden pr-4 md:table-cell">Owner</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {contacts.isPending ? (
              <SkeletonRows
                columns={[
                  "",
                  "hidden lg:table-cell",
                  "hidden sm:table-cell",
                  "hidden xl:table-cell",
                  "hidden 2xl:table-cell",
                  "",
                  "hidden md:table-cell",
                ]}
              />
            ) : contacts.data?.data.length ? (
              contacts.data.data.map((p) => (
                <TableRow
                  key={p.id}
                  className="cursor-pointer"
                  onClick={(e) => {
                    if (!(e.target as HTMLElement).closest("a,button")) router.push(`/contacts/${p.id}`);
                  }}
                >
                  <TableCell className="max-w-64 truncate pl-4">
                    <Link href={`/contacts/${p.id}`} className="font-medium text-fg hover:underline">
                      {p.full_name}
                    </Link>
                  </TableCell>
                  <TableCell className="hidden max-w-52 truncate text-fg-2 lg:table-cell">
                    {p.title ?? <span className="text-fg-3">Not set</span>}
                  </TableCell>
                  <TableCell className="hidden sm:table-cell">
                    {p.company ? (
                      <Link href={`/companies/${p.company.id}`} className="text-fg-1 hover:text-fg hover:underline">
                        {p.company.name}
                      </Link>
                    ) : (
                      <span className="text-fg-3">No company</span>
                    )}
                  </TableCell>
                  <TableCell className="hidden max-w-56 truncate xl:table-cell">
                    {p.email ? (
                      <a href={`mailto:${p.email}`} className="text-fg-1 hover:text-fg hover:underline">
                        {p.email}
                      </a>
                    ) : (
                      <span className="text-fg-3">Not set</span>
                    )}
                  </TableCell>
                  <TableCell className="hidden whitespace-nowrap text-fg-1 figures 2xl:table-cell">
                    {p.phone ?? <span className="text-fg-3">Not set</span>}
                  </TableCell>
                  <TableCell>
                    <ContactStatusBadge status={p.status} />
                  </TableCell>
                  <TableCell className="hidden max-w-44 pr-4 md:table-cell">
                    <OwnerCell owner={p.owner} />
                  </TableCell>
                </TableRow>
              ))
            ) : (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={7}>
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
