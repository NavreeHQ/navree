using System.ComponentModel.DataAnnotations;
using System.ComponentModel.DataAnnotations.Schema;
using Microsoft.EntityFrameworkCore;

namespace Bench.Data;

// The schema already exists (bench/db/init.sql); these entities only map it.
public sealed class BenchDb(DbContextOptions<BenchDb> options) : DbContext(options)
{
    public DbSet<World> Worlds => Set<World>();
    public DbSet<User> Users => Set<User>();
    public DbSet<Deal> Deals => Set<Deal>();
}

[Table("world")]
public sealed class World
{
    [Column("id"), DatabaseGenerated(DatabaseGeneratedOption.None)]
    public int Id { get; set; }

    [Column("randomnumber")]
    public int RandomNumber { get; set; }
}

[Table("users")]
public sealed class User
{
    [Column("id")]
    public Guid Id { get; set; }

    [Column("name")]
    public required string Name { get; set; }

    [Column("email")]
    public required string Email { get; set; }
}

[Table("deals")]
public sealed class Deal
{
    [Column("id")]
    public Guid Id { get; set; }

    [Column("title"), MaxLength(200)]
    public required string Title { get; set; }

    [Column("amount_cents")]
    public long AmountCents { get; set; }

    [Column("stage"), MaxLength(20)]
    public required string Stage { get; set; }

    [Column("owner_id")]
    public Guid OwnerId { get; set; }

    public User Owner { get; set; } = null!;

    [Column("created_at")]
    public DateTime CreatedAt { get; set; }

    [Column("updated_at")]
    public DateTime UpdatedAt { get; set; }
}
