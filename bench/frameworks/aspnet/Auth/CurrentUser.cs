using System.Security.Claims;
using Bench.Data;
using Microsoft.AspNetCore.Authentication.JwtBearer;
using Microsoft.AspNetCore.Http.Features;
using Microsoft.EntityFrameworkCore;
using Microsoft.IdentityModel.JsonWebTokens;

namespace Bench.Auth;

public static class CurrentUser
{
    // JwtBearer has checked the signature, exp and iss; now the user must exist.
    // A failure here makes the request unauthenticated, so [Authorize] answers 401.
    public static async Task LoadAsync(TokenValidatedContext context)
    {
        var sub = context.Principal?.FindFirstValue(JwtRegisteredClaimNames.Sub);
        if (!Guid.TryParse(sub, out var id))
        {
            context.Fail("The token has no valid subject.");
            return;
        }
        var db = context.HttpContext.RequestServices.GetRequiredService<BenchDb>();
        var user = await db.Users.AsNoTracking().FirstOrDefaultAsync(u => u.Id == id, context.HttpContext.RequestAborted);
        if (user is null)
        {
            context.Fail("Unknown user.");
            return;
        }
        context.HttpContext.Features.Set(user);
    }

    /// The user loaded by <see cref="LoadAsync"/>.
    public static User Get(HttpContext http) => http.Features.GetRequiredFeature<User>();
}
