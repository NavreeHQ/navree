// ASP.NET Core MVC controllers + EF Core (Npgsql) + JwtBearer, see bench/SPEC.md.
using System.Text;
using System.Text.Json;
using Bench.Auth;
using Bench.Data;
using Microsoft.AspNetCore.Authentication.JwtBearer;
using Microsoft.AspNetCore.Mvc;
using Microsoft.EntityFrameworkCore;
using Microsoft.IdentityModel.JsonWebTokens;
using Microsoft.IdentityModel.Tokens;
using Npgsql;

var builder = WebApplication.CreateBuilder(args);
var config = builder.Configuration;

builder.Services
    .AddControllers()
    .AddJsonOptions(o => o.JsonSerializerOptions.PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower)
    .ConfigureApiBehaviorOptions(o =>
    {
        // Keep [ApiController]'s automatic 400 ValidationProblemDetails, but send it
        // as application/json rather than application/problem+json (see README).
        var problemDetailsResponse = o.InvalidModelStateResponseFactory;
        o.InvalidModelStateResponseFactory = context =>
        {
            var result = problemDetailsResponse(context);
            if (result is ObjectResult objectResult)
            {
                objectResult.ContentTypes.Clear();
                objectResult.ContentTypes.Add("application/json");
            }
            return result;
        };
    });

builder.Services.AddDbContextPool<BenchDb>(o => o.UseNpgsql(ConnectionString(config)));

var jwtSecret = config["JWT_SECRET"] ?? throw new InvalidOperationException("JWT_SECRET is not set");
builder.Services
    .AddAuthentication(JwtBearerDefaults.AuthenticationScheme)
    .AddJwtBearer(o =>
    {
        o.MapInboundClaims = false; // keep "sub" as "sub"
        o.TokenValidationParameters = new TokenValidationParameters
        {
            ValidIssuer = config["JWT_ISSUER"] ?? "keel",
            ValidateAudience = false,
            IssuerSigningKey = new SymmetricSecurityKey(Encoding.UTF8.GetBytes(jwtSecret)),
            ValidAlgorithms = [SecurityAlgorithms.HmacSha256],
            NameClaimType = JwtRegisteredClaimNames.Sub,
        };
        o.Events = new JwtBearerEvents { OnTokenValidated = CurrentUser.LoadAsync };
    });
builder.Services.AddAuthorization();

var app = builder.Build();
app.UseAuthentication();
app.UseAuthorization();
app.MapControllers();
app.Run();

// DATABASE_URL (postgres://user:pass@host:port/db) or the DB_* variables, as an
// Npgsql connection string with a pool of at most 64 connections.
static string ConnectionString(IConfiguration config)
{
    var csb = new NpgsqlConnectionStringBuilder();
    if (config["DATABASE_URL"] is { Length: > 0 } databaseUrl)
    {
        var uri = new Uri(databaseUrl);
        var userInfo = uri.UserInfo.Split(':', 2);
        csb.Host = uri.Host;
        csb.Port = uri.Port > 0 ? uri.Port : 5432;
        csb.Database = Uri.UnescapeDataString(uri.AbsolutePath.TrimStart('/'));
        csb.Username = Uri.UnescapeDataString(userInfo[0]);
        csb.Password = userInfo.Length > 1 ? Uri.UnescapeDataString(userInfo[1]) : null;
    }
    else
    {
        csb.Host = config["DB_HOST"];
        csb.Port = int.TryParse(config["DB_PORT"], out var port) ? port : 5432;
        csb.Database = config["DB_NAME"];
        csb.Username = config["DB_USER"];
        csb.Password = config["DB_PASSWORD"];
    }
    csb.MaxPoolSize = 64;
    return csb.ConnectionString;
}
