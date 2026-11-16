namespace Navree.Design.Inputs;

/// <summary>
/// The XMP rule: a packet that holds any property about a place goes whole. A property is found by its name (an element's or an attribute's, with
/// any namespace prefix or none, in any letter case, with white space round <c>=</c>), not by its namespace, so a writer's own namespace is caught
/// as long as it names the property as the others do. The names, and where each comes from, are in <see cref="IsPlace"/>.
/// </summary>
public static partial class PlaceRules
{
    // What a packet that went is replaced with: an empty xmpmeta element, then spaces, so the packet keeps its length and stays well formed.
    private static readonly byte[] EmptyXmp = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>"u8.ToArray();

    // Names that are a place whatever comes before or after in them.
    private static readonly HashSet<string> PlaceNames = new(StringComparer.Ordinal)
    {
        // photoshop: (City, State, Country); Iptc4xmpCore: (Location, CountryCode, and the creator's contact info: CiAdrCity, CiAdrCtry, CiAdrExtadr,
        // CiAdrPcode, CiAdrRegion, CreatorCity, CreatorCountry, CreatorAddress, CreatorPostalCode, CreatorRegion); Iptc4xmpExt: (LocationCreated,
        // LocationShown, and their City, CountryCode, CountryName, ProvinceState, Sublocation, WorldRegion, LocationId, LocationName); dc:coverage;
        // the Licensor struct's address; all from ExifTool's XMP tag page. lat, lon, lng and long are the W3C geo vocabulary's short names.
        "city", "state", "country", "countrycode", "countryname", "provincestate", "sublocation", "worldregion", "locationid", "locationname", "coverage",
        "ciadrcity", "ciadrctry", "ciadrextadr", "ciadrpcode", "ciadrregion", "creatorcity", "creatorcountry", "creatoraddress", "creatorpostalcode",
        "creatorregion", "licensorcity", "licensorcountry", "licensorextendedaddress", "licensorpostalcode", "licensorregion", "licensorstreetaddress",
        "address", "postalcode", "streetaddress", "lat", "lon", "lng", "long",
    };

    // Endings that make a name a place: exif:GPSDestLatitude, drone-dji:AbsoluteAltitude, xmpDM:shotLocation, prism:location and the like.
    private static readonly string[] PlaceEndings = ["latitude", "longitude", "longtitude", "altitude", "location"];

    /// <summary>Whether XMP text holds a property about a place.</summary>
    /// <param name="text">The packet's bytes, UTF-8.</param>
    /// <returns>True when it does.</returns>
    public static bool HoldsPlace(ReadOnlySpan<byte> text)
    {
        int at = 0;
        while (at < text.Length)
        {
            if (!IsNameStart(text[at]))
            {
                at++;
                continue;
            }

            int start = at;
            while (at < text.Length && IsNameChar(text[at]))
            {
                at++;
            }

            byte before = start > 0 ? text[start - 1] : (byte)' ';
            if (IsNameChar(before))
            {
                continue;
            }

            ReadOnlySpan<byte> name = text[start..at];
            if (at < text.Length && text[at] == ':')
            {
                // A prefix. The one Sony Ericsson's phones write their cell's place under (ExifTool: "Location tags written by some Sony Ericsson phones").
                if (Lower(name) == "cell" && (before is (byte)'<' or (byte)'/' || IsSpace(before)))
                {
                    return true;
                }

                continue;
            }

            int next = at;
            while (next < text.Length && IsSpace(text[next]))
            {
                next++;
            }

            bool closing = before == '/' && start >= 2 && text[start - 2] == '<';
            bool named = before is (byte)':' or (byte)'<' || closing || (next < text.Length && text[next] == '=');
            if (named && IsPlace(Lower(name)))
            {
                return true;
            }
        }

        return false;
    }

    /// <summary>Whether a property's local name, lower case, is about a place.</summary>
    /// <param name="name">The local name, lower case.</param>
    /// <returns>True when it is.</returns>
    internal static bool IsPlace(string name) =>
        name.StartsWith("gps", StringComparison.Ordinal) || PlaceNames.Contains(name) || PlaceEndings.Any(ending => name.EndsWith(ending, StringComparison.Ordinal));

    // Empties the XMP packet at file[from .. end] when it holds a place. A packet holding a NUL byte is not UTF-8 (XMP Part 3 asks for UTF-8 in
    // every format this reads), so it cannot be read here.
    private static void Xmp(Cleaning work, int from, int end)
    {
        Span<byte> packet = work.Bytes.AsSpan(from, end - from);
        if (packet.Contains((byte)0))
        {
            throw Doubted();
        }

        if (HoldsPlace(packet))
        {
            EmptyPacket(packet, true);
            work.Changed = true;
        }
    }

    // Writes an empty packet over bytes: the empty xmpmeta element when it fits and the caller wants it, then spaces.
    private static void EmptyPacket(Span<byte> packet, bool withRoot)
    {
        packet.Fill((byte)' ');
        if (withRoot && packet.Length >= EmptyXmp.Length)
        {
            EmptyXmp.CopyTo(packet);
        }
    }

    private static bool IsNameStart(byte value) => value is (>= (byte)'a' and <= (byte)'z') or (>= (byte)'A' and <= (byte)'Z') or (byte)'_';

    private static bool IsNameChar(byte value) => IsNameStart(value) || value is (>= (byte)'0' and <= (byte)'9') or (byte)'-' or (byte)'.' or >= 0x80;

    private static bool IsSpace(byte value) => value is (byte)' ' or (byte)'\t' or (byte)'\r' or (byte)'\n';

    private static string Lower(ReadOnlySpan<byte> name)
    {
        Span<char> chars = name.Length <= 256 ? stackalloc char[name.Length] : new char[name.Length];
        for (int at = 0; at < name.Length; at++)
        {
            byte value = name[at];
            chars[at] = (char)(value is >= (byte)'A' and <= (byte)'Z' ? value | 0x20 : value);
        }

        return new string(chars);
    }
}
