using System.Globalization;

namespace Navree.Presence;

/// <summary>The count on the taskbar button, as plain rules.</summary>
public static class BadgeRules
{
    /// <summary>The most the system's own badge shows as a number; past it the system writes "99+".</summary>
    public const long Largest = 99;

    /// <summary>The count that is worth showing: none, or a number of one or more.</summary>
    /// <param name="count">What the core said (none clears the badge).</param>
    /// <returns>The count, or null for no badge.</returns>
    public static long? Shown(long? count) => count is long number and > 0 ? number : null;

    /// <summary>What the badge says, as the system's own badge does.</summary>
    /// <param name="count">The count.</param>
    /// <returns>The words, or null for no badge.</returns>
    public static string? Label(long? count) =>
        Shown(count) switch
        {
            null => null,
            > Largest => "99+",
            long number => number.ToString(CultureInfo.InvariantCulture),
        };

    /// <summary>What the system's badge is given: a number up to one past the largest (the system writes the "+" itself).</summary>
    /// <param name="count">The count.</param>
    /// <returns>The number, or 0 to clear the badge.</returns>
    public static uint ForSystem(long? count) => Shown(count) is long number ? (uint)Math.Min(number, Largest + 1) : 0;

    /// <summary>
    /// What a 16 pixel overlay icon is drawn with. Two characters is what fits, so a count past 99 is written "9+".
    /// </summary>
    /// <param name="count">The count.</param>
    /// <returns>The characters, or null for no icon.</returns>
    public static string? OverlayText(long? count) =>
        Shown(count) switch
        {
            null => null,
            > Largest => "9+",
            long number => number.ToString(CultureInfo.InvariantCulture),
        };

    /// <summary>What a screen reader is told of the taskbar button's count.</summary>
    /// <param name="app">The app's name.</param>
    /// <param name="count">The count.</param>
    /// <returns>The words.</returns>
    public static string Description(string app, long? count) =>
        Label(count) is string label ? app + ", " + label + " unread" : app;
}

/// <summary>
/// Small pictures the app draws itself, as pixels: the overlay a count is shown in on the taskbar button when the app has
/// no package identity, and the app's own icon for the places that need one. Each is straight (not premultiplied) BGRA,
/// <c>size * size * 4</c> bytes, a corner of the square left clear.
/// </summary>
public static class IconArt
{
    // Three columns and five rows for each character, the top row first; a set bit is a lit pixel, the left column the highest.
    private static readonly Dictionary<char, byte[]> Font = new()
    {
        ['0'] = [0b111, 0b101, 0b101, 0b101, 0b111],
        ['1'] = [0b010, 0b110, 0b010, 0b010, 0b111],
        ['2'] = [0b111, 0b001, 0b111, 0b100, 0b111],
        ['3'] = [0b111, 0b001, 0b111, 0b001, 0b111],
        ['4'] = [0b101, 0b101, 0b111, 0b001, 0b001],
        ['5'] = [0b111, 0b100, 0b111, 0b001, 0b111],
        ['6'] = [0b111, 0b100, 0b111, 0b101, 0b111],
        ['7'] = [0b111, 0b001, 0b010, 0b010, 0b010],
        ['8'] = [0b111, 0b101, 0b111, 0b101, 0b111],
        ['9'] = [0b111, 0b101, 0b111, 0b001, 0b111],
        ['+'] = [0b000, 0b010, 0b111, 0b010, 0b000],
        ['N'] = [0b101, 0b111, 0b111, 0b111, 0b101],
    };

    /// <summary>How many times a character of the font is enlarged to fill a square of this size with this many characters.</summary>
    /// <param name="size">The square's side, in pixels.</param>
    /// <param name="characters">How many characters.</param>
    /// <returns>The enlargement, at least one.</returns>
    public static int Scale(int size, int characters)
    {
        int across = Math.Max(1, characters * 4 - 1);
        int byWidth = (size - 2) / across;
        int byHeight = (size - 4) / 5;
        return Math.Max(1, Math.Min(byWidth, byHeight));
    }

    /// <summary>A red disc with the count written in white: the overlay on the taskbar button.</summary>
    /// <param name="text">The characters (digits and a plus).</param>
    /// <param name="size">The side, in pixels.</param>
    /// <returns>The pixels.</returns>
    public static byte[] Overlay(string text, int size)
    {
        ArgumentException.ThrowIfNullOrEmpty(text);
        byte[] pixels = new byte[size * size * 4];
        Disc(pixels, size, 0xC4, 0x2B, 0x1C);
        int scale = Scale(size, text.Length);
        Write(pixels, size, text, scale, 0xFF, 0xFF, 0xFF, size / 2.0, size / 2.0);
        return pixels;
    }

    /// <summary>The app's own icon: a dark rounded square with a green N.</summary>
    /// <param name="size">The side, in pixels.</param>
    /// <returns>The pixels.</returns>
    public static byte[] App(int size)
    {
        byte[] pixels = new byte[size * size * 4];
        double radius = size * 0.22;
        for (int y = 0; y < size; y++)
        {
            for (int x = 0; x < size; x++)
            {
                double alpha = InRoundedSquare(x + 0.5, y + 0.5, size, radius);
                if (alpha > 0)
                {
                    Put(pixels, size, x, y, 0x12, 0x14, 0x12, alpha);
                }
            }
        }

        int scale = Math.Max(1, (size - 4) / 6);
        Write(pixels, size, "N", scale, 0x39, 0xE6, 0x7A, size / 2.0, size / 2.0);
        return pixels;
    }

    // Coverage of a pixel centre by a square with rounded corners: 1 inside, 0 outside, a pixel's width of edge between.
    private static double InRoundedSquare(double x, double y, int size, double radius)
    {
        double dx = Math.Max(Math.Max(radius - x, 0), x - (size - radius));
        double dy = Math.Max(Math.Max(radius - y, 0), y - (size - radius));
        double distance = Math.Sqrt(dx * dx + dy * dy);
        return Math.Clamp(radius - distance + 0.5, 0, 1);
    }

    private static void Disc(byte[] pixels, int size, byte red, byte green, byte blue)
    {
        double radius = size / 2.0;
        for (int y = 0; y < size; y++)
        {
            for (int x = 0; x < size; x++)
            {
                double dx = x + 0.5 - radius;
                double dy = y + 0.5 - radius;
                double alpha = Math.Clamp(radius - Math.Sqrt(dx * dx + dy * dy) + 0.5, 0, 1);
                if (alpha > 0)
                {
                    Put(pixels, size, x, y, red, green, blue, alpha);
                }
            }
        }
    }

    private static void Write(byte[] pixels, int size, string text, int scale, byte red, byte green, byte blue, double centreX, double centreY)
    {
        int width = (text.Length * 4 - 1) * scale;
        int height = 5 * scale;
        int left = (int)Math.Round(centreX - width / 2.0);
        int top = (int)Math.Round(centreY - height / 2.0);
        for (int at = 0; at < text.Length; at++)
        {
            if (!Font.TryGetValue(text[at], out byte[]? rows))
            {
                continue;
            }

            for (int row = 0; row < 5; row++)
            {
                for (int column = 0; column < 3; column++)
                {
                    if ((rows[row] & (0b100 >> column)) == 0)
                    {
                        continue;
                    }

                    for (int dy = 0; dy < scale; dy++)
                    {
                        for (int dx = 0; dx < scale; dx++)
                        {
                            int x = left + (at * 4 + column) * scale + dx;
                            int y = top + row * scale + dy;
                            if (x >= 0 && x < size && y >= 0 && y < size)
                            {
                                Put(pixels, size, x, y, red, green, blue, 1.0);
                            }
                        }
                    }
                }
            }
        }
    }

    private static void Put(byte[] pixels, int size, int x, int y, byte red, byte green, byte blue, double alpha)
    {
        int at = (y * size + x) * 4;
        pixels[at] = blue;
        pixels[at + 1] = green;
        pixels[at + 2] = red;
        pixels[at + 3] = (byte)Math.Round(alpha * 255);
    }
}
