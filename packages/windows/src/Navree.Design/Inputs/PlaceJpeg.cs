using System.Buffers.Binary;

namespace Navree.Design.Inputs;

/// <summary>
/// JPEG. The marker structure is T.81's (B.1.1.2 to B.1.1.5, Table B.1): a marker is 0xFF and a byte that is not 0 or 0xFF, may be preceded by 0xFF
/// filling, SOI, EOI, RSTm and TEM stand alone, every other marker starts a segment whose length counts itself; in the compressed data every 0xFF is
/// followed by a stuffed 0 (or is a restart marker), so the first other marker ends a scan; tables and miscellaneous segments (APPn and COM among
/// them) may come before every scan (B.2.4), so every segment up to EOI is read. What each application segment is, is its identifier (XMP Part 3,
/// 1.1.3, Table 5; ExifTool's JPEG tag page): Exif, XMP, extended XMP and Photoshop's resources are read; JFIF, the colour profile, the multi-picture
/// index and Adobe's colour transform are kept, as they say how the picture looks or where its other pictures are and hold no place; every other
/// application segment (a GoPro's GPMF, JUMBF, a maker's own) is emptied, as a decoder ignores application data it does not know. A comment is
/// a person's or a program's words and is kept. After the first picture, every byte is walked: another picture (the further pictures of a
/// multi-picture file, an Ultra HDR gain map, both stored as pictures appended to the first) is cleaned as a picture of its own; 0x00 and 0xFF
/// filling is passed over; a motion photo's video, "at the end of the file" (Motion Photo 1.0), and anything else are refused.
/// </summary>
public static partial class PlaceRules
{
    private static readonly byte[] StandardXmp = "http://ns.adobe.com/xap/1.0/\0"u8.ToArray();

    private static readonly byte[] ExtendedXmp = "http://ns.adobe.com/xmp/extension/\0"u8.ToArray();

    // One piece of an extended XMP packet (XMP Part 3, 1.1.3.1): where its bytes are, and its offset in the whole serialization.
    private readonly record struct Piece(int From, int End, uint Offset);

    private static void JpegFile(Cleaning work)
    {
        byte[] file = work.Bytes;
        int at = Picture(work, 0, file.Length, 0);
        int pictures = 1;
        while (at < file.Length)
        {
            int found = file.AsSpan(at).IndexOfAnyExcept((byte)0x00, (byte)0xFF);
            if (found < 0)
            {
                return;
            }

            int next = at + found;

            // Another picture: its SOI's 0xFF was passed over as filling.
            if (file[next] == 0xD8 && next > at && file[next - 1] == 0xFF && next + 1 < file.Length && file[next + 1] == 0xFF)
            {
                if (++pictures > MostPictures)
                {
                    throw Doubted(Doubt.After);
                }

                at = Picture(work, next - 1, file.Length, 0);
                continue;
            }

            // An ISO media file starts with its first box's size, then 'ftyp'; the size's first bytes are zeros and were passed over.
            for (int box = Math.Max(at, next - 3); box <= next; box++)
            {
                if (box + 8 <= file.Length && file.AsSpan(box + 4, 4).SequenceEqual("ftyp"u8))
                {
                    throw Doubted(Doubt.Video);
                }
            }

            throw Doubted(Doubt.After);
        }
    }

    // Cleans the picture that starts at file[start] (its SOI) and ends at or before end; returns where it ends, just past its EOI.
    private static int Picture(Cleaning work, int start, int end, int depth)
    {
        if (depth > Deepest)
        {
            throw Doubted();
        }

        byte[] file = work.Bytes;
        if (end - start < 4 || file[start] != 0xFF || file[start + 1] != 0xD8)
        {
            throw Doubted();
        }

        List<Piece> pieces = [];
        int at = start + 2;
        while (true)
        {
            if (at + 2 > end || file[at] != 0xFF)
            {
                throw Doubted();
            }

            while (at + 2 < end && file[at + 1] == 0xFF)
            {
                at++;
            }

            byte marker = file[at + 1];
            at += 2;
            if (marker == 0xD9)
            {
                Extended(work, pieces);
                return at;
            }

            if (marker is 0x01 or (>= 0xD0 and <= 0xD7))
            {
                continue;
            }

            // 0x02 to 0xBF are reserved (Table B.1); a second SOI inside a picture is not a picture this reads.
            if (marker is 0x00 or 0xD8 or 0xFF or (>= 0x02 and <= 0xBF))
            {
                throw Doubted();
            }

            if (at + 2 > end)
            {
                throw Doubted();
            }

            int length = BinaryPrimitives.ReadUInt16BigEndian(file.AsSpan(at));
            int segmentEnd = at + length;
            if (length < 2 || segmentEnd > end)
            {
                throw Doubted();
            }

            Segment(work, marker, at + 2, segmentEnd, depth, pieces);
            at = segmentEnd;
            if (marker == 0xDA)
            {
                at = ScanEnd(file, at, end);
            }
        }
    }

    // Where the compressed data that follows a scan header ends: at the first marker that is not a stuffed zero, a restart marker or filling.
    private static int ScanEnd(byte[] file, int at, int end)
    {
        while (true)
        {
            int found = file.AsSpan(at, end - at).IndexOf((byte)0xFF);
            if (found < 0)
            {
                throw Doubted();
            }

            int mark = at + found;
            if (mark + 1 >= end)
            {
                throw Doubted();
            }

            byte next = file[mark + 1];
            if (next is 0x00 or (>= 0xD0 and <= 0xD7))
            {
                at = mark + 2;
            }
            else if (next == 0xFF)
            {
                at = mark + 1;
            }
            else
            {
                return mark;
            }
        }
    }

    private static void Segment(Cleaning work, byte marker, int data, int end, int depth, List<Piece> pieces)
    {
        byte[] file = work.Bytes;
        ReadOnlySpan<byte> bytes = file.AsSpan(data, end - data);
        switch (marker)
        {
            case 0xE0 when bytes.StartsWith("JFIF\0"u8):
            case 0xE2 when bytes.StartsWith("ICC_PROFILE\0"u8) || bytes.StartsWith("MPF\0"u8):
            case 0xEE when bytes.StartsWith("Adobe"u8):
                return;
            case 0xE1 when bytes.StartsWith("Exif\0\0"u8):
                new TiffBlock(work, data + 6, end - data - 6, depth + 1, false).Clean();
                return;
            case 0xE1 when bytes.StartsWith(StandardXmp):
                Xmp(work, data + StandardXmp.Length, end);
                return;
            case 0xE1 when bytes.StartsWith(ExtendedXmp):
                // The signature, a GUID of 32 characters, the full length, this piece's offset, then the piece.
                int header = data + ExtendedXmp.Length;
                if (header + 40 > end)
                {
                    throw Doubted();
                }

                pieces.Add(new Piece(header + 40, end, BinaryPrimitives.ReadUInt32BigEndian(file.AsSpan(header + 36))));
                return;
            case 0xED when bytes.StartsWith("Photoshop 3.0\0"u8):
                Resources(work, data + "Photoshop 3.0\0"u8.Length, end, depth + 1);
                return;
            case >= 0xE0 and <= 0xEF:
                work.Zero(data, end);
                return;
            default:
                return;
        }
    }

    // The extended XMP of one picture, read whole: its pieces in the order of their offsets (a JPEG reader "should tolerate the marker segments in
    // any order", and the text is split "without regard to XML tokens"), so a name split across two pieces is found. When it holds a place, every
    // piece is emptied: the first becomes an empty xmpmeta element, the rest spaces.
    private static void Extended(Cleaning work, List<Piece> pieces)
    {
        if (pieces.Count == 0)
        {
            return;
        }

        List<Piece> ordered = [.. pieces.OrderBy(piece => piece.Offset)];
        byte[] joined = new byte[ordered.Sum(piece => piece.End - piece.From)];
        int at = 0;
        foreach (Piece piece in ordered)
        {
            work.Bytes.AsSpan(piece.From, piece.End - piece.From).CopyTo(joined.AsSpan(at));
            at += piece.End - piece.From;
        }

        if (joined.AsSpan().Contains((byte)0))
        {
            throw Doubted();
        }

        if (!HoldsPlace(joined))
        {
            return;
        }

        bool first = true;
        foreach (Piece piece in ordered)
        {
            EmptyPacket(work.Bytes.AsSpan(piece.From, piece.End - piece.From), first);
            first = false;
        }

        work.Changed = true;
    }
}
