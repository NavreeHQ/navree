using System.Globalization;

namespace Navree.Design;

/// <summary>The locale the client was started with: what it writes itself (a day, a time, a number) follows it, not the machine's own settings.</summary>
public static class ClientCulture
{
    /// <summary>The culture the client chose, or none (the person's own is then used).</summary>
    public static CultureInfo? Chosen { get; set; }

    /// <summary>The culture dates, times and numbers the client writes are in.</summary>
    public static CultureInfo Current => Chosen ?? CultureInfo.CurrentCulture;

    /// <summary>The culture for a locale name such as <c>en-IN</c>; none when the name is not one this machine knows.</summary>
    /// <param name="name">The locale's name.</param>
    /// <returns>The culture, or none.</returns>
    public static CultureInfo? Named(string? name)
    {
        if (string.IsNullOrWhiteSpace(name))
        {
            return null;
        }

        try
        {
            return CultureInfo.GetCultureInfo(name);
        }
        catch (CultureNotFoundException)
        {
            return null;
        }
    }
}
