"use client";

import { Can } from "@casl/react";
import { Building2Icon, PlusIcon } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { ListEmpty, PastEndNotice, SkeletonRows, SortableHead, TablePanel } from "@/components/data-table";
import { OptionFilter, OwnerFilter, SearchFilter } from "@/components/filters";
import { PageHeader } from "@/components/page-header";
import { PaginationBar } from "@/components/pagination-bar";
import { Button } from "@/components/ui/button";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { CompanyMonogram, OwnerCell } from "@/components/user-avatar";
import { useCurrentUser } from "@/features/auth/queries";
import { CompanyFormDialog, INDUSTRIES } from "@/features/companies/company-form";
import { useCompanies, type CompanyListParams } from "@/features/companies/queries";
import { useUrlState } from "@/hooks/use-url-state";
import type { CompanySort } from "@/lib/api/types";
import { formatDate, formatNumber } from "@/lib/format";

const SORTS: CompanySort[] = ["name", "-name", "created_at", "-created_at"];

function NewCompanyButton({ openOnCreateIntent }: { openOnCreateIntent?: boolean }) {
  return (
    <Can I="create" a="Company">
      <CompanyFormDialog
        openOnCreateIntent={openOnCreateIntent}
        trigger={
          <Button>
            <PlusIcon data-icon="inline-start" />
            New company
          </Button>
        }
      />
    </Can>
  );
}

export function CompaniesView() {
  const router = useRouter();
  const url = useUrlState();
  const me = useCurrentUser();
  const sortParam = url.get("sort") as CompanySort | undefined;
  const params: CompanyListParams = {
    search: url.get("search"),
    owner_id: url.get("owner"),
    industry: url.get("industry"),
    sort: sortParam && SORTS.includes(sortParam) ? sortParam : "name",
    page: url.page,
    per_page: 25,
  };
  const companies = useCompanies(params);
  const filtered = Boolean(params.search || params.owner_id || params.industry);
  const total = companies.data?.meta.total;

  return (
    <div className="flex flex-col">
      <PageHeader
        title="Companies"
        description={
          total === undefined
            ? "Accounts your team sells to"
            : `${formatNumber(total)} ${total === 1 ? "company" : "companies"}${filtered ? " match" : ""}`
        }
        actions={<NewCompanyButton openOnCreateIntent />}
      />
      <TablePanel>
        <div className="flex flex-wrap items-center gap-2 border-b p-3" role="search" aria-label="Filter companies">
          <SearchFilter
            value={params.search}
            onChange={(search) => url.set({ search })}
            placeholder="Search by name or domain"
          />
          <OwnerFilter value={params.owner_id} onChange={(owner) => url.set({ owner })} currentUserId={me.data?.id} />
          <OptionFilter
            value={params.industry}
            onChange={(industry) => url.set({ industry })}
            options={INDUSTRIES.map((i) => ({ value: i, label: i }))}
            anyLabel="Any industry"
            ariaLabel="Filter by industry"
          />
        </div>
        <Table className="text-[13px]">
          <TableHeader>
            <TableRow className="bg-ink-1 hover:bg-ink-1 [&>th]:h-9">
              <SortableHead
                label="Company"
                field="name"
                sort={params.sort}
                onSort={(sort) => url.set({ sort })}
                className="pl-4"
              />
              <TableHead className="hidden md:table-cell">Industry</TableHead>
              <TableHead className="hidden xl:table-cell">Employees</TableHead>
              <TableHead className="hidden lg:table-cell">Location</TableHead>
              <TableHead>Owner</TableHead>
              <SortableHead
                label="Added"
                field="created_at"
                sort={params.sort}
                onSort={(sort) => url.set({ sort })}
                className="hidden pr-4 sm:table-cell"
              />
            </TableRow>
          </TableHeader>
          <TableBody>
            {companies.isPending ? (
              <SkeletonRows
                columns={[
                  "",
                  "hidden md:table-cell",
                  "hidden xl:table-cell",
                  "hidden lg:table-cell",
                  "",
                  "hidden sm:table-cell",
                ]}
              />
            ) : companies.data?.data.length ? (
              companies.data.data.map((c) => (
                <TableRow
                  key={c.id}
                  className="cursor-pointer"
                  onClick={(e) => {
                    if (!(e.target as HTMLElement).closest("a,button")) router.push(`/companies/${c.id}`);
                  }}
                >
                  <TableCell className="py-2.5 pl-4">
                    <span className="flex items-center gap-3">
                      <CompanyMonogram name={c.name} />
                      <span className="flex min-w-0 flex-col">
                        <Link href={`/companies/${c.id}`} className="truncate font-medium text-fg hover:underline">
                          {c.name}
                        </Link>
                        {c.domain ? <span className="truncate text-xs text-fg-3">{c.domain}</span> : null}
                      </span>
                    </span>
                  </TableCell>
                  <TableCell className="hidden text-fg-1 md:table-cell">
                    {c.industry ?? <span className="text-fg-3">Not set</span>}
                  </TableCell>
                  <TableCell className="hidden text-fg-1 figures xl:table-cell">
                    {c.size ?? <span className="text-fg-3">Unknown</span>}
                  </TableCell>
                  <TableCell className="hidden max-w-48 truncate text-fg-1 lg:table-cell">
                    {[c.city, c.country].filter(Boolean).join(", ") || <span className="text-fg-3">Not set</span>}
                  </TableCell>
                  <TableCell className="max-w-44">
                    <OwnerCell owner={c.owner} />
                  </TableCell>
                  <TableCell className="hidden pr-4 text-fg-3 figures sm:table-cell">
                    {formatDate(c.created_at)}
                  </TableCell>
                </TableRow>
              ))
            ) : (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={6}>
                  {companies.data && companies.data.meta.total > 0 ? (
                    <PastEndNotice
                      noun="companies"
                      total={companies.data.meta.total}
                      onFirstPage={() => url.set({ page: undefined })}
                    />
                  ) : (
                    <ListEmpty
                      filtered={filtered}
                      noun="companies"
                      icon={<Building2Icon />}
                      onClear={() => url.set({ search: undefined, owner: undefined, industry: undefined })}
                      action={<NewCompanyButton />}
                    />
                  )}
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
        <PaginationBar meta={companies.data?.meta} onPageChange={(page) => url.set({ page })} noun="companies" />
      </TablePanel>
    </div>
  );
}
