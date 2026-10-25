"use client";

import { Can } from "@casl/react";
import { Building2Icon, GlobeIcon, MapPinIcon, PencilIcon, PlusIcon, Trash2Icon, UsersIcon } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { Money } from "@/components/money";
import { DetailRow, Panel } from "@/components/page-header";
import { MetaItem, RecordHeader, RecordMissing, RecordSkeleton } from "@/components/record-header";
import { StageBadge } from "@/components/stage";
import { Button } from "@/components/ui/button";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { OwnerCell } from "@/components/user-avatar";
import { RecordActivity } from "@/features/activities/components";
import { CompanyFormDialog } from "@/features/companies/company-form";
import { useCompany, useDeleteCompany } from "@/features/companies/queries";
import { ContactFormDialog } from "@/features/contacts/contact-form";
import { ContactStatusBadge } from "@/features/contacts/status-badge";
import { useContacts } from "@/features/contacts/queries";
import { DealFormDialog } from "@/features/deals/deal-form";
import { useDeals } from "@/features/deals/queries";
import type { CompanyDetail as CompanyDetailT } from "@/lib/api/types";
import { formatDate, formatDateLong, formatNumber } from "@/lib/format";

const BACK = { href: "/companies", label: "Companies" };

function Figures({ company }: { company: CompanyDetailT }) {
  return (
    <dl className="grid grid-cols-3 divide-x rounded-lg border bg-card">
      <div className="flex flex-col gap-1 px-4 py-3">
        <dt className="text-xs text-muted-foreground">Open pipeline</dt>
        <dd className="text-xl font-semibold font-condensed">
          <Money cents={company.open_deals_amount_cents} compact />
        </dd>
      </div>
      <div className="flex flex-col gap-1 px-4 py-3">
        <dt className="text-xs text-muted-foreground">Open deals</dt>
        <dd className="text-xl font-semibold font-condensed figures">{formatNumber(company.open_deals_count)}</dd>
      </div>
      <div className="flex flex-col gap-1 px-4 py-3">
        <dt className="text-xs text-muted-foreground">Contacts</dt>
        <dd className="text-xl font-semibold font-condensed figures">{formatNumber(company.contacts_count)}</dd>
      </div>
    </dl>
  );
}

function CompanyDeals({ company }: { company: CompanyDetailT }) {
  const deals = useDeals({ company_id: company.id, per_page: 50 });
  return (
    <div className="flex flex-col gap-3">
      <div className="flex justify-end">
        <Can I="create" a="Deal">
          <DealFormDialog
            company={{ id: company.id, name: company.name }}
            trigger={
              <Button variant="outline" size="sm">
                <PlusIcon data-icon="inline-start" />
                New deal
              </Button>
            }
          />
        </Can>
      </div>
      {deals.data?.data.length ? (
        <Table className="text-[13px]">
          <TableHeader>
            <TableRow className="hover:bg-transparent [&>th]:h-8 [&>th]:text-xs [&>th]:font-medium [&>th]:text-muted-foreground">
              <TableHead>Deal</TableHead>
              <TableHead>Stage</TableHead>
              <TableHead className="text-right">Value</TableHead>
              <TableHead>Owner</TableHead>
              <TableHead>Close</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {deals.data.data.map((d) => (
              <TableRow key={d.id}>
                <TableCell className="py-2.5">
                  <Link href={`/deals/${d.id}`} className="font-medium hover:underline">
                    {d.title}
                  </Link>
                </TableCell>
                <TableCell>
                  <StageBadge stage={d.stage} />
                </TableCell>
                <TableCell className="text-right font-medium">
                  <Money cents={d.amount_cents} currency={d.currency} />
                </TableCell>
                <TableCell>
                  <OwnerCell owner={d.owner} />
                </TableCell>
                <TableCell className="text-muted-foreground figures">
                  {formatDate(d.closed_at ?? d.expected_close_date) || "Not set"}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      ) : (
        <p className="py-2 text-sm text-muted-foreground">
          {deals.isPending ? "Loading deals…" : "No deals with this company yet."}
        </p>
      )}
    </div>
  );
}

function CompanyContacts({ company }: { company: CompanyDetailT }) {
  const contacts = useContacts({ company_id: company.id, per_page: 50 });
  return (
    <div className="flex flex-col gap-3">
      <div className="flex justify-end">
        <Can I="create" a="Contact">
          <ContactFormDialog
            company={{ id: company.id, name: company.name }}
            trigger={
              <Button variant="outline" size="sm">
                <PlusIcon data-icon="inline-start" />
                New contact
              </Button>
            }
          />
        </Can>
      </div>
      {contacts.data?.data.length ? (
        <ul className="divide-y rounded-lg border">
          {contacts.data.data.map((p) => (
            <li key={p.id} className="flex items-center justify-between gap-3 px-3 py-2.5 text-sm">
              <div className="flex min-w-0 flex-col">
                <Link href={`/contacts/${p.id}`} className="font-medium hover:underline">
                  {p.full_name}
                </Link>
                <span className="truncate text-xs text-muted-foreground">
                  {[p.title, p.email].filter(Boolean).join(", ") || "No title or email"}
                </span>
              </div>
              <ContactStatusBadge status={p.status} />
            </li>
          ))}
        </ul>
      ) : (
        <p className="py-2 text-sm text-muted-foreground">
          {contacts.isPending ? "Loading contacts…" : "No contacts at this company yet."}
        </p>
      )}
    </div>
  );
}

export function CompanyDetail({ id }: { id: string }) {
  const router = useRouter();
  const company = useCompany(id);
  const remove = useDeleteCompany();

  if (company.isPending) return <RecordSkeleton />;
  if (company.isError) return <RecordMissing error={company.error} noun="company" back={BACK} />;
  const c = company.data;

  return (
    <div className="flex flex-col">
      <RecordHeader
        back={BACK}
        title={c.name}
        meta={
          <>
            {c.domain ? (
              <MetaItem icon={GlobeIcon}>
                <a
                  href={c.website ?? `https://${c.domain}`}
                  target="_blank"
                  rel="noreferrer"
                  className="hover:text-foreground hover:underline"
                >
                  {c.domain}
                </a>
              </MetaItem>
            ) : null}
            {c.industry ? <MetaItem icon={Building2Icon}>{c.industry}</MetaItem> : null}
            {c.size ? <MetaItem icon={UsersIcon}>{c.size} employees</MetaItem> : null}
            {c.city || c.country ? (
              <MetaItem icon={MapPinIcon}>{[c.city, c.country].filter(Boolean).join(", ")}</MetaItem>
            ) : null}
          </>
        }
        actions={
          <>
            <Can I="update" this={c}>
              <CompanyFormDialog
                company={c}
                trigger={
                  <Button variant="outline">
                    <PencilIcon data-icon="inline-start" />
                    Edit
                  </Button>
                }
              />
            </Can>
            <Can I="delete" this={c}>
              <ConfirmDialog
                trigger={
                  <Button variant="ghost" aria-label="Delete company">
                    <Trash2Icon />
                  </Button>
                }
                title={`Delete ${c.name}?`}
                description="The company is removed from lists and search. Its deals and contacts stay, without a company link in reports."
                confirmLabel="Delete company"
                pending={remove.isPending}
                onConfirm={() =>
                  remove.mutate(c.id, {
                    onSuccess: () => {
                      toast.success(`${c.name} deleted`);
                      router.push("/companies");
                    },
                  })
                }
              />
            </Can>
          </>
        }
      />

      <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_19rem]">
        <div className="flex min-w-0 flex-col gap-6">
          <Figures company={c} />
          <Tabs defaultValue="activity" className="gap-4">
            <TabsList>
              <TabsTrigger value="activity">Activity</TabsTrigger>
              <TabsTrigger value="deals">
                Deals <span className="text-muted-foreground figures">{c.open_deals_count}</span>
              </TabsTrigger>
              <TabsTrigger value="contacts">
                Contacts <span className="text-muted-foreground figures">{c.contacts_count}</span>
              </TabsTrigger>
            </TabsList>
            <TabsContent value="activity">
              <Panel>
                <RecordActivity subjectType="company" subjectId={c.id} />
              </Panel>
            </TabsContent>
            <TabsContent value="deals">
              <Panel>
                <CompanyDeals company={c} />
              </Panel>
            </TabsContent>
            <TabsContent value="contacts">
              <Panel>
                <CompanyContacts company={c} />
              </Panel>
            </TabsContent>
          </Tabs>
        </div>
        <Panel title="Details" className="h-fit" bodyClassName="py-2">
          <dl>
            <DetailRow label="Owner">
              <OwnerCell owner={c.owner} />
            </DetailRow>
            <DetailRow label="Website">
              {c.website ? (
                <a href={c.website} target="_blank" rel="noreferrer" className="text-primary hover:underline">
                  {c.website.replace(/^https?:\/\//, "")}
                </a>
              ) : null}
            </DetailRow>
            <DetailRow label="Phone">
              {c.phone ? (
                <a href={`tel:${c.phone}`} className="figures hover:underline">
                  {c.phone}
                </a>
              ) : null}
            </DetailRow>
            <DetailRow label="Industry">{c.industry}</DetailRow>
            <DetailRow label="Employees">{c.size}</DetailRow>
            <DetailRow label="City">{c.city}</DetailRow>
            <DetailRow label="Country">{c.country}</DetailRow>
            <DetailRow label="Added">{formatDateLong(c.created_at)}</DetailRow>
            <DetailRow label="Last updated">{formatDateLong(c.updated_at)}</DetailRow>
          </dl>
        </Panel>
      </div>
    </div>
  );
}
