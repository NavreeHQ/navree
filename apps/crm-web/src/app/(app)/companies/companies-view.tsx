"use client";

import { Can } from "@casl/react";
import { Building2Icon, PlusIcon } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { ListEmpty, SkeletonRows, SortableHead, TablePanel } from "@/components/data-table";
import { OptionFilter, OwnerFilter, SearchFilter } from "@/components/filters";
import { PageHeader } from "@/components/page-header";
import { PaginationBar } from "@/components/pagination-bar";
import { Button } from "@/components/ui/button";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { OwnerCell } from "@/components/user-avatar";
import { useCurrentUser } from "@/features/auth/queries";
import { CompanyFormDialog, INDUSTRIES } from "@/features/companies/company-form";
import { useCompanies, type CompanyListParams } from "@/features/companies/queries";
import { useUrlState } from "@/hooks/use-url-state";
import type { CompanySort } from "@/lib/api/types";
import { formatDate, formatNumber } from "@/lib/format";

const SORTS: CompanySort[] = ["name", "-name", "created_at", "-created_at"];

function NewCompanyButton() {
  return (
    <Can I="create" a="Company">
      <CompanyFormDialog
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
        actions={<NewCompanyButton />}
      />
      <TablePanel>
        <div className="flex flex-wrap items-center gap-2 border-b p-3">
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
            <TableRow className="hover:bg-transparent [&>th]:h-9 [&>th]:text-xs [&>th]:font-medium [&>th]:text-muted-foreground">
              <SortableHead
                label="Company"
                field="name"
                sort={params.sort}
                onSort={(sort) => url.set({ sort })}
                className="pl-4"
              />
              <TableHead>Industry</TableHead>
              <TableHead className="hidden xl:table-cell">Employees</TableHead>
              <TableHead>Location</TableHead>
              <TableHead>Owner</TableHead>
              <SortableHead
                label="Added"
                field="created_at"
                sort={params.sort}
                onSort={(sort) => url.set({ sort })}
                className="pr-4"
              />
            </TableRow>
          </TableHeader>
          <TableBody>
            {companies.isPending ? (
              <SkeletonRows columns={5} />
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
                    <Link href={`/companies/${c.id}`} className="font-medium hover:underline">
                      {c.name}
                    </Link>
                    {c.domain ? <div className="text-xs text-muted-foreground">{c.domain}</div> : null}
                  </TableCell>
                  <TableCell>{c.industry ?? <span className="text-muted-foreground">Not set</span>}</TableCell>
                  <TableCell className="hidden figures xl:table-cell">
                    {c.size ?? <span className="text-muted-foreground">Unknown</span>}
                  </TableCell>
                  <TableCell className="max-w-48 truncate">
                    {[c.city, c.country].filter(Boolean).join(", ") || (
                      <span className="text-muted-foreground">Not set</span>
                    )}
                  </TableCell>
                  <TableCell>
                    <OwnerCell owner={c.owner} />
                  </TableCell>
                  <TableCell className="pr-4 text-muted-foreground figures">{formatDate(c.created_at)}</TableCell>
                </TableRow>
              ))
            ) : (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={6}>
                  <ListEmpty
                    filtered={filtered}
                    noun="companies"
                    icon={<Building2Icon />}
                    onClear={() => url.set({ search: undefined, owner: undefined, industry: undefined })}
                    action={<NewCompanyButton />}
                  />
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
