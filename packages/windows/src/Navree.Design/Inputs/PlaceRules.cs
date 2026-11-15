using System.Buffers.Binary;
using System.Text;

namespace Navree.Design.Inputs;

/// <summary>What became of the place a picture was taken.</summary>
public enum PlaceOutcome
{
    /// <summary>The picture holds no place (or is not a picture): nothing was changed.</summary>
    None,

    /// <summary>The place was taken out; the bytes handed in were changed in place.</summary>
    Removed,

    /// <summary>The picture may hold a place and this client cannot take it out of this kind of picture, or cannot read it well enough to be sure.</summary>
    Refused,
}

/// <summary>What <see cref="PlaceRules.TakeOut"/> found.</summary>
/// <param name="Outcome">What became of the place.</param>
/// <param name="Why">When refused, the words to say at the field.</param>
public sealed record PlaceResult(PlaceOutcome Outcome, string? Why = null);

/// <summary>
/// Takes the place a photograph was taken (its GPS tags) out of the bytes of a picture before it is sent, unless the field keeps it. The pictures
/// are changed where the place is written and nowhere else, so every pixel and every other tag stays as it was: the GPS directory of the EXIF
/// block and the GPS values of an XMP packet are emptied in place, with the sizes of everything left as they were.
/// Covered: JPEG, PNG, WebP and TIFF (the EXIF block and the XMP packet); GIF, BMP and SVG have no EXIF block and are checked for GPS text.
/// Not covered, and refused when they are chosen for a field that does not keep the place: HEIC, HEIF, AVIF, JPEG XL, raw formats, and a picture
/// of a covered kind that cannot be read to the end. Not covered at all: the place in a video, and a place written in a maker's own note.
/// </summary>
public static class PlaceRules
{
    private const ushort GpsPointer = 0x8825;

    /// <summary>Whether a media type is a picture, which is the only kind this looks into.</summary>
    /// <param name="mediaType">The media type.</param>
    /// <returns>True for a picture.</returns>
    public static bool IsPicture(string mediaType) => mediaType.StartsWith("image/", StringComparison.OrdinalIgnoreCase);

    /// <summary>Takes the place out of a picture's bytes, in place.</summary>
    /// <param name="file">The whole file. Changed when the outcome is <see cref="PlaceOutcome.Removed"/>.</param>
    /// <param name="name">The file's name, which says what it should be when its bytes do not.</param>
    /// <returns>What became of the place.</returns>
    public static PlaceResult TakeOut(byte[] file, string name)
    {
        ArgumentNullException.ThrowIfNull(file);
        string type = FileRules.MediaTypeOf(name);
        try
        {
            if (Is(file, [0xFF, 0xD8, 0xFF]))
            {
                return Done(Jpeg(file));
            }

            if (Is(file, [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]))
            {
                return Done(Png(file));
            }

            if (Is(file, "RIFF"u8) && file.Length >= 12 && Is(file.AsSpan(8), "WEBP"u8))
            {
                return Done(Webp(file));
            }

            if (Is(file, "II*\0"u8) || Is(file, "MM\0*"u8))
            {
                return Done(Exif(file, 0, file.Length));
            }

            if (Is(file, "GIF8"u8) || Is(file, "BM"u8) || type == "image/svg+xml")
            {
                return Contains(file, "GPSLatitude"u8) || Contains(file, "GPSLongitude"u8) ? Refuse(type) : new PlaceResult(PlaceOutcome.None);
            }
        }
        catch (FormatException)
        {
            return Refuse(type);
        }

        // Not one of the kinds above: a picture of another kind (or one whose bytes are not what its name says) may hold a place nobody here can see.
        return IsPicture(type) ? Refuse(type) : new PlaceResult(PlaceOutcome.None);
    }

    private static PlaceResult Done(bool removed) => new(removed ? PlaceOutcome.Removed : PlaceOutcome.None);

    private static PlaceResult Refuse(string type)
    {
        string kind = type.StartsWith("image/", StringComparison.Ordinal) ? type["image/".Length..].ToUpperInvariant() : "this kind of";
        return new PlaceResult(PlaceOutcome.Refused, "The place it was taken cannot be taken out of a " + kind + " picture here, so it was not added. Save it as a JPEG or PNG picture and choose that.");
    }

    // ---- JPEG -------------------------------------------------------------------------------------------------

    private static bool Jpeg(byte[] file)
    {
        bool removed = false;
        int at = 2;
        while (at + 4 <= file.Length)
        {
            if (file[at] != 0xFF)
            {
                throw new FormatException("a JPEG marker was expected");
            }

            byte marker = file[at + 1];
            if (marker == 0xFF)
            {
                at++;
                continue;
            }

            if (marker is 0xD9 or 0xDA)
            {
                // The end of the picture, or the start of its pixels: the metadata comes before.
                return removed;
            }

            if (marker is 0x01 or (>= 0xD0 and <= 0xD8))
            {
                at += 2;
                continue;
            }

            int length = BinaryPrimitives.ReadUInt16BigEndian(file.AsSpan(at + 2));
            int start = at + 4;
            int end = at + 2 + length;
            if (length < 2 || end > file.Length)
            {
                throw new FormatException("a JPEG segment runs past the end");
            }

            if (marker == 0xE1)
            {
                if (HasPrefix(file, start, end, "Exif\0\0"u8))
                {
                    removed |= Exif(file, start + 6, end - start - 6);
                }
                else if (HasPrefix(file, start, end, "http://ns.adobe.com/xap/1.0/\0"u8) || HasPrefix(file, start, end, "http://ns.adobe.com/xmp/extension/\0"u8))
                {
                    removed |= Xmp(file, start, end);
                }
            }

            at = end;
        }

        return removed;
    }

    // ---- PNG --------------------------------------------------------------------------------------------------

    private static bool Png(byte[] file)
    {
        bool removed = false;
        int at = 8;
        while (at + 12 <= file.Length)
        {
            uint length = BinaryPrimitives.ReadUInt32BigEndian(file.AsSpan(at));
            int start = at + 8;
            if (length > int.MaxValue || start + (long)length + 4 > file.Length)
            {
                throw new FormatException("a PNG chunk runs past the end");
            }

            int end = start + (int)length;
            string kind = Encoding.ASCII.GetString(file, at + 4, 4);
            if (kind == "IEND")
            {
                return removed;
            }

            bool changed = false;
            if (kind == "eXIf")
            {
                changed = Exif(file, start, (int)length);
            }
            else if (kind == "iTXt" && KeywordIs(file, start, end, "XML:com.adobe.xmp"))
            {
                int flag = start + "XML:com.adobe.xmp".Length + 1;
                if (flag < end && file[flag] != 0)
                {
                    throw new FormatException("a compressed XMP packet cannot be read");
                }

                changed = Xmp(file, start, end);
            }
            else if (kind is "tEXt" or "zTXt" or "iTXt" && KeywordStarts(file, start, end, "Raw profile type"))
            {
                throw new FormatException("a profile written as text cannot be read");
            }

            if (changed)
            {
                removed = true;
                uint crc = Crc32(file.AsSpan(at + 4, 4 + (int)length));
                BinaryPrimitives.WriteUInt32BigEndian(file.AsSpan(end), crc);
            }

            at = end + 4;
        }

        return removed;
    }

    private static bool KeywordIs(byte[] file, int start, int end, string keyword) =>
        start + keyword.Length < end && file[start + keyword.Length] == 0 && Encoding.Latin1.GetString(file, start, keyword.Length) == keyword;

    private static bool KeywordStarts(byte[] file, int start, int end, string prefix) =>
        start + prefix.Length <= end && Encoding.Latin1.GetString(file, start, prefix.Length) == prefix;

    // ---- WebP -------------------------------------------------------------------------------------------------

    private static bool Webp(byte[] file)
    {
        bool removed = false;
        int at = 12;
        while (at + 8 <= file.Length)
        {
            uint size = BinaryPrimitives.ReadUInt32LittleEndian(file.AsSpan(at + 4));
            int start = at + 8;
            if (size > int.MaxValue || start + (long)size > file.Length)
            {
                throw new FormatException("a WebP chunk runs past the end");
            }

            int end = start + (int)size;
            string kind = Encoding.ASCII.GetString(file, at, 4);
            if (kind == "EXIF")
            {
                // Some writers put the six bytes "Exif" and two zeros before the block, as a JPEG does; the block itself is the same.
                int offset = HasPrefix(file, start, end, "Exif\0\0"u8) ? 6 : 0;
                removed |= Exif(file, start + offset, (int)size - offset);
            }
            else if (kind == "XMP ")
            {
                removed |= Xmp(file, start, end);
            }

            at = end + (int)(size & 1);
        }

        return removed;
    }

    // ---- the EXIF block (a TIFF structure) ---------------------------------------------------------------------

    // Empties the GPS directory of the TIFF structure at file[from .. from + length], the out-of-line values first, then the directory itself.
    // Everything keeps its place and its size, so nothing else in the file moves. Returns true when a directory holding something was emptied.
    private static bool Exif(byte[] file, int from, int length)
    {
        if (length < 8 || from < 0 || from + (long)length > file.Length)
        {
            throw new FormatException("the EXIF block is too short");
        }

        bool little = file[from] == (byte)'I' && file[from + 1] == (byte)'I';
        if (!little && !(file[from] == (byte)'M' && file[from + 1] == (byte)'M'))
        {
            throw new FormatException("the EXIF block has no byte order");
        }

        ushort Read16(long at)
        {
            if (at < 0 || at + 2 > length)
            {
                throw new FormatException("the EXIF block is cut");
            }

            return little ? BinaryPrimitives.ReadUInt16LittleEndian(file.AsSpan(from + (int)at)) : BinaryPrimitives.ReadUInt16BigEndian(file.AsSpan(from + (int)at));
        }

        uint Read32(long at)
        {
            if (at < 0 || at + 4 > length)
            {
                throw new FormatException("the EXIF block is cut");
            }

            return little ? BinaryPrimitives.ReadUInt32LittleEndian(file.AsSpan(from + (int)at)) : BinaryPrimitives.ReadUInt32BigEndian(file.AsSpan(from + (int)at));
        }

        if (Read16(2) != 42)
        {
            throw new FormatException("the EXIF block is not a TIFF structure");
        }

        long first = Read32(4);
        int entries = Read16(first);
        long gps = -1;
        for (int entry = 0; entry < entries; entry++)
        {
            long place = first + 2 + (12L * entry);
            if (Read16(place) == GpsPointer)
            {
                gps = Read32(place + 8);
                break;
            }
        }

        if (gps < 0)
        {
            return false;
        }

        int count = Read16(gps);
        if (count == 0)
        {
            return false;
        }

        long directoryEnd = gps + 2 + (12L * count) + 4;
        if (directoryEnd > length)
        {
            throw new FormatException("the GPS directory runs past the end");
        }

        for (int entry = 0; entry < count; entry++)
        {
            long place = gps + 2 + (12L * entry);
            long size = TypeSize(Read16(place + 2)) * (long)Read32(place + 4);
            if (size > 4)
            {
                long where = Read32(place + 8);
                if (where < 0 || where + size > length)
                {
                    throw new FormatException("a GPS value runs past the end");
                }

                Array.Clear(file, from + (int)where, (int)size);
            }
        }

        Array.Clear(file, from + (int)gps, (int)(directoryEnd - gps));
        return true;
    }

    private static int TypeSize(ushort type) => type switch
    {
        1 or 2 or 6 or 7 => 1,
        3 or 8 => 2,
        4 or 9 or 11 => 4,
        5 or 10 or 12 or 16 or 17 or 18 => 8,
        _ => 1,
    };

    // ---- XMP --------------------------------------------------------------------------------------------------

    // Empties the value of every GPS property of the packet at file[from .. end], written as an attribute (exif:GPSLatitude="...") or as an element
    // (<exif:GPSLatitude>...</exif:GPSLatitude>), with spaces, so the packet stays well formed and keeps its length. Returns true when a value was emptied.
    private static bool Xmp(byte[] file, int from, int end)
    {
        bool removed = false;
        ReadOnlySpan<byte> mark = "GPS"u8;
        int at = from;
        while (at + 4 < end)
        {
            int found = file.AsSpan(at, end - at).IndexOf(mark);
            if (found < 0)
            {
                break;
            }

            int name = at + found;
            at = name + mark.Length;

            // A property of the exif namespace: "exif:" (or any prefix) then GPS then the rest of the name, then "=" or ">".
            if (name == 0 || file[name - 1] != (byte)':')
            {
                continue;
            }

            int after = at;
            while (after < end && (char.IsAsciiLetterOrDigit((char)file[after])))
            {
                after++;
            }

            if (after >= end)
            {
                break;
            }

            int valueStart;
            int valueEnd;
            if (file[after] == (byte)'=' && after + 1 < end && file[after + 1] is (byte)'"' or (byte)'\'')
            {
                byte quote = file[after + 1];
                valueStart = after + 2;
                valueEnd = Array.IndexOf(file, quote, valueStart, end - valueStart);
            }
            else if (file[after] == (byte)'>')
            {
                valueStart = after + 1;
                valueEnd = Array.IndexOf(file, (byte)'<', valueStart, end - valueStart);
            }
            else
            {
                continue;
            }

            if (valueEnd < 0)
            {
                throw new FormatException("an XMP value is not closed");
            }

            for (int index = valueStart; index < valueEnd; index++)
            {
                if (file[index] != (byte)' ')
                {
                    removed = true;
                    file[index] = (byte)' ';
                }
            }

            at = valueEnd;
        }

        return removed;
    }

    // ---- bytes ------------------------------------------------------------------------------------------------

    private static bool Is(ReadOnlySpan<byte> file, ReadOnlySpan<byte> head) => file.Length >= head.Length && file[..head.Length].SequenceEqual(head);

    private static bool Contains(byte[] file, ReadOnlySpan<byte> text) => file.AsSpan().IndexOf(text) >= 0;

    private static bool HasPrefix(byte[] file, int start, int end, ReadOnlySpan<byte> prefix) =>
        end - start >= prefix.Length && file.AsSpan(start, prefix.Length).SequenceEqual(prefix);

    private static readonly uint[] Table = MakeTable();

    private static uint[] MakeTable()
    {
        uint[] table = new uint[256];
        for (uint n = 0; n < 256; n++)
        {
            uint c = n;
            for (int bit = 0; bit < 8; bit++)
            {
                c = (c & 1) != 0 ? 0xEDB88320u ^ (c >> 1) : c >> 1;
            }

            table[n] = c;
        }

        return table;
    }

    /// <summary>The CRC-32 a PNG chunk is checked by: of its type and its data.</summary>
    /// <param name="bytes">The bytes.</param>
    /// <returns>The check value.</returns>
    public static uint Crc32(ReadOnlySpan<byte> bytes)
    {
        uint c = 0xFFFFFFFFu;
        foreach (byte value in bytes)
        {
            c = Table[(c ^ value) & 0xFF] ^ (c >> 8);
        }

        return c ^ 0xFFFFFFFFu;
    }
}
