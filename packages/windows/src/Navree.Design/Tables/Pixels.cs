namespace Navree.Design.Tables;

/// <summary>Sizes in effective pixels made whole in physical pixels.</summary>
public static class Pixels
{
    /// <summary>
    /// The smallest size, in effective pixels, that is a whole number of physical pixels and at
    /// least <paramref name="epx"/>. A size of zero or less gives zero.
    /// </summary>
    /// <param name="epx">The size in effective pixels.</param>
    /// <param name="scale">Physical pixels for each effective pixel; more than zero.</param>
    /// <returns>The rounded size, in effective pixels.</returns>
    public static double RoundUp(double epx, double scale)
    {
        if (!(scale > 0) || double.IsInfinity(scale))
        {
            throw new ArgumentOutOfRangeException(nameof(scale), scale, "The scale must be more than zero.");
        }

        if (double.IsNaN(epx))
        {
            throw new ArgumentOutOfRangeException(nameof(epx), epx, "The size must be a number.");
        }

        if (epx <= 0)
        {
            return 0;
        }

        return Math.Ceiling((epx * scale) - 1e-9) / scale;
    }
}
