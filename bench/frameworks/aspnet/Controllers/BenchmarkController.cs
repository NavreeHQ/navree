using System.Text.Json.Serialization;
using Bench.Data;
using Microsoft.AspNetCore.Mvc;
using Microsoft.EntityFrameworkCore;

namespace Bench.Controllers;

public sealed record Greeting(string Message);

public sealed record WorldResource(int Id, [property: JsonPropertyName("randomNumber")] int RandomNumber);

[ApiController]
public sealed class BenchmarkController(BenchDb db) : ControllerBase
{
    [HttpGet("/json")]
    public Greeting Json() => new("Hello, World!");

    [HttpGet("/db")]
    public Task<WorldResource> SingleQuery(CancellationToken ct) => RandomWorld(ct);

    [HttpGet("/queries")]
    public async Task<WorldResource[]> MultipleQueries([FromQuery] string? queries, CancellationToken ct)
    {
        var count = int.TryParse(queries, out var n) ? Math.Clamp(n, 1, 500) : 1;
        var worlds = new WorldResource[count];
        for (var i = 0; i < count; i++)
        {
            worlds[i] = await RandomWorld(ct);
        }
        return worlds;
    }

    private Task<WorldResource> RandomWorld(CancellationToken ct)
    {
        var id = Random.Shared.Next(1, 10001);
        return db.Worlds.AsNoTracking()
            .Where(w => w.Id == id)
            .Select(w => new WorldResource(w.Id, w.RandomNumber))
            .FirstAsync(ct);
    }
}
