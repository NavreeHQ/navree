using System.ComponentModel.DataAnnotations;
using System.Linq.Expressions;
using Bench.Auth;
using Bench.Data;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;
using Microsoft.EntityFrameworkCore;

namespace Bench.Controllers;

public sealed class DealListQuery
{
    [FromQuery(Name = "page"), Range(1, int.MaxValue)]
    public int Page { get; set; } = 1;

    [FromQuery(Name = "per_page"), Range(1, 100)]
    public int PerPage { get; set; } = 20;
}

public sealed class CreateDealRequest
{
    [Required, StringLength(200, MinimumLength = 1)]
    public string? Title { get; set; }

    [Required, Range(typeof(long), "0", "1000000000000")]
    public long? AmountCents { get; set; }

    [Required, AllowedValues("lead", "qualified", "proposal", "negotiation", "won", "lost")]
    public string? Stage { get; set; }
}

public sealed record OwnerResource(Guid Id, string Name);

public sealed record DealResource(Guid Id, string Title, long AmountCents, string Stage, OwnerResource Owner, DateTime CreatedAt)
{
    public static readonly Expression<Func<Deal, DealResource>> Projection = d =>
        new DealResource(d.Id, d.Title, d.AmountCents, d.Stage, new OwnerResource(d.Owner.Id, d.Owner.Name), d.CreatedAt);
}

public sealed record PageMeta(int CurrentPage, int PerPage, int Total, int LastPage);

public sealed record DealPage(IReadOnlyList<DealResource> Data, PageMeta Meta);

[ApiController]
[Authorize]
[Route("api/deals")]
public sealed class DealsController(BenchDb db) : ControllerBase
{
    [HttpGet]
    public async Task<DealPage> List([FromQuery] DealListQuery query, CancellationToken ct)
    {
        var total = await db.Deals.CountAsync(ct);

        var offset = (long)(query.Page - 1) * query.PerPage;
        List<DealResource> data = offset > int.MaxValue
            ? [] // further than any table this API could hold
            : await db.Deals.AsNoTracking()
                .OrderByDescending(d => d.CreatedAt)
                .ThenByDescending(d => d.Id)
                .Skip((int)offset)
                .Take(query.PerPage)
                .Select(DealResource.Projection)
                .ToListAsync(ct);

        var lastPage = Math.Max(1, (total + query.PerPage - 1) / query.PerPage);
        return new DealPage(data, new PageMeta(query.Page, query.PerPage, total, lastPage));
    }

    [HttpPost]
    public async Task<ActionResult<DealResource>> Create(CreateDealRequest request, CancellationToken ct)
    {
        var owner = CurrentUser.Get(HttpContext);
        var now = DateTime.UtcNow;
        var deal = new Deal
        {
            Title = request.Title!,
            AmountCents = request.AmountCents!.Value,
            Stage = request.Stage!,
            OwnerId = owner.Id,
            CreatedAt = now,
            UpdatedAt = now,
        };
        db.Deals.Add(deal);
        await db.SaveChangesAsync(ct);

        var resource = new DealResource(deal.Id, deal.Title, deal.AmountCents, deal.Stage, new OwnerResource(owner.Id, owner.Name), deal.CreatedAt);
        return StatusCode(StatusCodes.Status201Created, resource);
    }
}
