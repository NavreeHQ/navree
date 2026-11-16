using System.Buffers.Binary;

namespace Navree.Design.Inputs;

/// <summary>
/// TIFF, and the EXIF block, which is a TIFF structure (XMP Part 3, 1.1.6, Tables 10 to 13): an 8-byte header and a chain of directories of
/// 12-byte entries, each a tag, a type, a count and the value or its offset. Every directory is walked: the chain from IFD0, SubIFDs (0x014A), the
/// Exif (0x8769), GPS (0x8825) and Interop (0xA005) directories. The GPS directory is emptied, its out-of-line values first. Read and cleaned as
/// what they are: XMP (700, 0x02BC), IPTC IIM (33723, 0x83BB) and Photoshop's image resources (34377, 0x8649), and the thumbnail
/// (0x0201/0x0202, a JPEG of its own). Emptied because they are a place or cannot be read here: the maker's note (0x927C), GeoTIFF's tags
/// (0x830E, 0x8482, 0x85D8, 0x87AF, 0x87B0, 0x87B1), DNGPrivateData (0xC634) and private tags from 0xC000 up this code does not know (ExifTool's
/// EXIF tag page names the public ones). Nothing is emptied before the whole structure is read, and nothing that would touch a directory, a value
/// that is kept or the picture's own data (strips, tiles) is emptied: such a file is refused, so the pixels cannot change. A camera's raw picture
/// that looks like TIFF (DNGVersion 0xC612; PhotometricInterpretation 32803 or 34892) is refused. The sizes of the types are XMP Part 3's Table 11
/// (1 to 12) and ExifTool's Exif.pm (13 "ifd", 129 "utf8" of Exif 3.0); a value of another type is refused when it has to be emptied.
/// </summary>
public static partial class PlaceRules
{
    private static readonly HashSet<ushort> PlaceTags = [0x927C, 0xC634, 0x830E, 0x8482, 0x85D8, 0x87AF, 0x87B0, 0x87B1];

    private static long TypeSize(ushort type) => type switch
    {
        1 or 2 or 6 or 7 or 129 => 1,
        3 or 8 => 2,
        4 or 9 or 11 or 13 => 4,
        5 or 10 or 12 => 8,
        _ => -1,
    };

    // A private tag this code keeps: PrintIM, Microsoft's padding and offset schema, and the DNG range (a DNG itself is refused).
    private static bool KnownPrivate(ushort tag) => tag is 0xC4A5 or 0xEA1C or 0xEA1D || (tag is >= 0xC612 and <= 0xCFFF && tag != 0xC634);

    private enum Directory
    {
        Image,
        Exif,
        Gps,
        Interop,
    }

    private enum Inside
    {
        Xmp,
        Iptc,
        Resources,
        Thumbnail,
    }

    private readonly record struct Entry(long At, ushort Tag, ushort Type, uint Count);

    private readonly record struct Stretch(long At, long Length)
    {
        public long End => At + Length;

        public bool Meets(Stretch other) => At < other.End && other.At < End;
    }

    /// <summary>One TIFF structure: a whole TIFF file, or an EXIF block inside another file.</summary>
    private sealed class TiffBlock
    {
        private const int MostDirectories = 512;
        private const int MostEntries = 1_000_000;

        private readonly Cleaning work;
        private readonly byte[] file;
        private readonly int from;
        private readonly int length;
        private readonly int depth;
        private readonly bool whole;
        private readonly bool little;
        private readonly List<Stretch> blanks = [];
        private readonly List<Stretch> kept = [];
        private readonly List<(Stretch Where, Inside What)> inner = [];
        private readonly List<(Entry Offsets, Entry Counts)> pictureData = [];
        private readonly HashSet<long> seen = [];
        private int entries;
        private bool raw;

        internal TiffBlock(Cleaning work, int from, int length, int depth, bool whole)
        {
            if (depth > Deepest || length < 8 || from < 0 || (long)from + length > work.Bytes.Length)
            {
                throw Doubted();
            }

            this.work = work;
            file = work.Bytes;
            this.from = from;
            this.length = length;
            this.depth = depth;
            this.whole = whole;
            little = file[from] == 'I' && file[from + 1] == 'I';
            if (!little && !(file[from] == 'M' && file[from + 1] == 'M'))
            {
                throw Doubted();
            }

            if (Read16(2) != 42)
            {
                throw Doubted();
            }
        }

        internal void Clean()
        {
            Walk(Read32(4), Directory.Image, true);
            if (whole && raw)
            {
                throw Doubted(Doubt.Raw);
            }

            // What would be emptied must touch nothing that is kept, nothing read as a part of its own, and none of the picture's own data.
            List<Stretch> empty = Merged(blanks);
            List<Stretch> parts = [.. inner.Select(part => part.Where).OrderBy(part => part.At)];
            for (int at = 1; at < parts.Count; at++)
            {
                if (parts[at - 1].Meets(parts[at]))
                {
                    throw Doubted();
                }
            }

            foreach (Stretch stretch in kept)
            {
                if (Touches(empty, stretch) || Touches(parts, stretch))
                {
                    throw Doubted();
                }
            }

            foreach (Stretch part in parts)
            {
                if (Touches(empty, part))
                {
                    throw Doubted();
                }
            }

            foreach ((Entry offsets, Entry counts) in pictureData)
            {
                long pieces = Math.Min(offsets.Count, counts.Count);
                if (offsets.Count != counts.Count)
                {
                    throw Doubted();
                }

                for (long at = 0; at < pieces; at++)
                {
                    Stretch data = new(Number(offsets, at), Number(counts, at));
                    if (data.End > length || Touches(empty, data) || Touches(parts, data))
                    {
                        throw Doubted();
                    }
                }
            }

            foreach (Stretch stretch in empty)
            {
                work.Zero(from + (int)stretch.At, from + (int)stretch.End);
            }

            foreach ((Stretch where, Inside what) in inner)
            {
                int start = from + (int)where.At;
                int end = from + (int)where.End;
                switch (what)
                {
                    case Inside.Xmp:
                        Xmp(work, start, end);
                        break;
                    case Inside.Iptc:
                        Iptc(work, start, end);
                        break;
                    case Inside.Resources:
                        Resources(work, start, end, depth + 1);
                        break;
                    default:
                        int after = Picture(work, start, end, depth + 1);
                        if (file.AsSpan(after, end - after).IndexOfAnyExcept((byte)0x00, (byte)0xFF) >= 0)
                        {
                            throw Doubted();
                        }

                        break;
                }
            }
        }

        private void Walk(long offset, Directory kind, bool chain)
        {
            for (int hops = 0; offset != 0; hops++)
            {
                if (hops > 64 || seen.Count >= MostDirectories || !seen.Add(offset))
                {
                    throw Doubted();
                }

                int count = Read16(offset);
                long end = offset + 2 + (12L * count) + 4;
                if (end > length)
                {
                    throw Doubted();
                }

                entries += count;
                if (entries > MostEntries)
                {
                    throw Doubted();
                }

                if (kind == Directory.Gps)
                {
                    blanks.Add(new Stretch(offset, end - offset));
                }
                else
                {
                    kept.Add(new Stretch(offset, end - offset));
                }

                Entry? stripOffsets = null;
                Entry? stripCounts = null;
                Entry? tileOffsets = null;
                Entry? tileCounts = null;
                Entry? thumbnail = null;
                Entry? thumbnailLength = null;
                for (int index = 0; index < count; index++)
                {
                    long at = offset + 2 + (12L * index);
                    Entry entry = new(at, (ushort)Read16(at), (ushort)Read16(at + 2), (uint)Read32(at + 4));
                    if (kind == Directory.Gps)
                    {
                        Blank(entry);
                        continue;
                    }

                    switch (entry.Tag)
                    {
                        case 0x8825:
                            Walk(Pointer(entry), Directory.Gps, false);
                            break;
                        case 0x8769:
                            Walk(Pointer(entry), Directory.Exif, false);
                            break;
                        case 0xA005:
                            Walk(Pointer(entry), Directory.Interop, false);
                            break;
                        case 0x014A:
                            Keep(entry);
                            for (long sub = 0; sub < Math.Min(entry.Count, 64u); sub++)
                            {
                                Walk(Number(entry, sub), Directory.Image, true);
                            }

                            break;
                        case 0x02BC:
                            Within(Value(entry), Inside.Xmp);
                            break;
                        case 0x83BB:
                            Within(Value(entry), Inside.Iptc);
                            break;
                        case 0x8649:
                            Within(Value(entry), Inside.Resources);
                            break;
                        case 0x0201:
                            thumbnail = entry;
                            break;
                        case 0x0202:
                            thumbnailLength = entry;
                            break;
                        case 0x0111:
                            stripOffsets = entry;
                            Keep(entry);
                            break;
                        case 0x0117:
                            stripCounts = entry;
                            Keep(entry);
                            break;
                        case 0x0144:
                            tileOffsets = entry;
                            Keep(entry);
                            break;
                        case 0x0145:
                            tileCounts = entry;
                            Keep(entry);
                            break;
                        case 0x0106:
                            raw |= entry.Count >= 1 && Number(entry, 0) is 32803 or 34892;
                            Keep(entry);
                            break;
                        case 0xC612:
                            raw = true;
                            Keep(entry);
                            break;
                        default:
                            if (PlaceTags.Contains(entry.Tag) || (entry.Tag >= 0xC000 && !KnownPrivate(entry.Tag)))
                            {
                                Blank(entry);
                            }
                            else
                            {
                                Keep(entry);
                            }

                            break;
                    }
                }

                if (thumbnail is { } where && thumbnailLength is { } size)
                {
                    Stretch picture = new(Number(where, 0), Number(size, 0));
                    if (picture.End > length)
                    {
                        throw Doubted();
                    }

                    Within(picture, Inside.Thumbnail);
                }
                else if (thumbnail is not null || thumbnailLength is not null)
                {
                    throw Doubted();
                }

                Pair(stripOffsets, stripCounts);
                Pair(tileOffsets, tileCounts);
                if (kind == Directory.Gps || !chain)
                {
                    return;
                }

                offset = Read32(end - 4);
            }
        }

        // A part read as what it is, after the whole structure is known. Four bytes or fewer, in the entry itself, cannot hold a packet, a dataset
        // or a picture, and are left.
        private void Within(Stretch where, Inside what)
        {
            if (where.Length > 4)
            {
                inner.Add((where, what));
            }
        }

        private void Pair(Entry? offsets, Entry? counts)
        {
            if (offsets is { } where && counts is { } size)
            {
                pictureData.Add((where, size));
            }
            else if (offsets is not null || counts is not null)
            {
                throw Doubted();
            }
        }

        // Where an entry's value is, and how long: in the entry itself when it fits in four bytes, else at the offset the entry gives.
        private Stretch Value(Entry entry)
        {
            long size = TypeSize(entry.Type);
            if (size < 0)
            {
                throw Doubted();
            }

            size *= entry.Count;
            long at = size <= 4 ? entry.At + 8 : Read32(entry.At + 8);
            if (at + size > length)
            {
                throw Doubted();
            }

            return new Stretch(at, size);
        }

        private void Keep(Entry entry)
        {
            // A value of a type this code does not know cannot be kept apart; it is not emptied either, as nothing says it is a place.
            if (TypeSize(entry.Type) < 0)
            {
                return;
            }

            Stretch value = Value(entry);
            if (value.Length > 4)
            {
                kept.Add(value);
            }
        }

        private void Blank(Entry entry)
        {
            Stretch value = Value(entry);
            if (value.Length <= 4)
            {
                // In the entry: the four bytes of its value field, which belong to this entry alone.
                work.Zero(from + (int)entry.At + 8, from + (int)entry.At + 12);
            }
            else
            {
                blanks.Add(value);
            }
        }

        // The one offset a pointer entry holds (a LONG or an IFD of count 1).
        private long Pointer(Entry entry) => entry.Type is 4 or 13 && entry.Count == 1 ? Read32(entry.At + 8) : throw Doubted();

        // The index-th number of a SHORT, LONG or IFD entry.
        private long Number(Entry entry, long index)
        {
            if (entry.Type is not (3 or 4 or 13) || index >= entry.Count)
            {
                throw Doubted();
            }

            Stretch value = Value(entry);
            return entry.Type == 3 ? Read16(value.At + (2 * index)) : Read32(value.At + (4 * index));
        }

        private int Read16(long at)
        {
            if (at < 0 || at + 2 > length)
            {
                throw Doubted();
            }

            ReadOnlySpan<byte> two = file.AsSpan(from + (int)at, 2);
            return little ? BinaryPrimitives.ReadUInt16LittleEndian(two) : BinaryPrimitives.ReadUInt16BigEndian(two);
        }

        private long Read32(long at)
        {
            if (at < 0 || at + 4 > length)
            {
                throw Doubted();
            }

            ReadOnlySpan<byte> four = file.AsSpan(from + (int)at, 4);
            return little ? BinaryPrimitives.ReadUInt32LittleEndian(four) : BinaryPrimitives.ReadUInt32BigEndian(four);
        }

        private static List<Stretch> Merged(List<Stretch> stretches)
        {
            List<Stretch> merged = [];
            foreach (Stretch stretch in stretches.Where(stretch => stretch.Length > 0).OrderBy(stretch => stretch.At))
            {
                if (merged.Count > 0 && merged[^1].End >= stretch.At)
                {
                    merged[^1] = new Stretch(merged[^1].At, Math.Max(merged[^1].End, stretch.End) - merged[^1].At);
                }
                else
                {
                    merged.Add(stretch);
                }
            }

            return merged;
        }

        // Whether a stretch meets any of stretches sorted by where they start and not meeting one another.
        private static bool Touches(List<Stretch> sorted, Stretch stretch)
        {
            if (stretch.Length <= 0)
            {
                return false;
            }

            int low = 0;
            int high = sorted.Count - 1;
            while (low <= high)
            {
                int middle = (low + high) / 2;
                if (sorted[middle].End <= stretch.At)
                {
                    low = middle + 1;
                }
                else if (sorted[middle].At >= stretch.End)
                {
                    high = middle - 1;
                }
                else
                {
                    return true;
                }
            }

            return false;
        }
    }

    // IPTC IIM (XMP Part 3, 3.1.4, Tables 34 and 35): datasets of a tag marker 0x1C, a record, a number and a big-endian length. The datasets of
    // record 2 about a place (ExifTool's IPTC page: 26 ContentLocationCode, 27 ContentLocationName, 90 City, 92 Sub-location, 95 Province-State,
    // 100 Country-PrimaryLocationCode, 101 Country-PrimaryLocationName) are filled with spaces. An extended dataset, whose length is longer than
    // two bytes say, is not read here.
    private static void Iptc(Cleaning work, int at, int end)
    {
        byte[] file = work.Bytes;
        while (at < end)
        {
            if (file[at] == 0)
            {
                if (file.AsSpan(at, end - at).ContainsAnyExcept((byte)0))
                {
                    throw Doubted();
                }

                return;
            }

            if (file[at] != 0x1C || at + 5 > end)
            {
                throw Doubted();
            }

            byte record = file[at + 1];
            byte number = file[at + 2];
            int size = BinaryPrimitives.ReadUInt16BigEndian(file.AsSpan(at + 3));
            int data = at + 5;
            if ((size & 0x8000) != 0 || data + size > end)
            {
                throw Doubted();
            }

            if (record == 2 && number is 26 or 27 or 90 or 92 or 95 or 100 or 101)
            {
                work.Spaces(data, data + size);
            }

            at = data + size;
        }
    }

    // Photoshop's image resources (XMP Part 3, 3.1.3, Table 33): "8BIM", a big-endian id, a Pascal name padded to an even length, a big-endian
    // size, the data padded to an even length. Read: 1028 (0x0404) IPTC, 1058 (0x0422) Exif as a TIFF, 1060 (0x0424) XMP (Table 36). Emptied: 0x0423
    // ("possible additional EXIF", ExifTool's Photoshop page, whose layout is not known here), the thumbnails 0x0409 and 0x040C, and a resource of a
    // type other than "8BIM", whose meaning is not known. Every other resource (resolution, print settings, slices) is kept.
    private static void Resources(Cleaning work, int at, int end, int depth)
    {
        if (depth > Deepest)
        {
            throw Doubted();
        }

        byte[] file = work.Bytes;
        while (at < end)
        {
            if (file[at] == 0 && !file.AsSpan(at, end - at).ContainsAnyExcept((byte)0))
            {
                return;
            }

            if (at + 7 > end)
            {
                throw Doubted();
            }

            bool ours = file.AsSpan(at, 4).SequenceEqual("8BIM"u8);
            int id = BinaryPrimitives.ReadUInt16BigEndian(file.AsSpan(at + 4));
            int name = 1 + file[at + 6];
            long sizeAt = (long)at + 6 + name + (name % 2);
            if (sizeAt + 4 > end)
            {
                throw Doubted();
            }

            long size = BinaryPrimitives.ReadUInt32BigEndian(file.AsSpan((int)sizeAt));
            long data = sizeAt + 4;
            if (data + size > end)
            {
                throw Doubted();
            }

            int start = (int)data;
            int stop = (int)(data + size);
            switch (ours ? id : -1)
            {
                case 0x0404:
                    Iptc(work, start, stop);
                    break;
                case 0x0422:
                    new TiffBlock(work, start, stop - start, depth + 1, false).Clean();
                    break;
                case 0x0424:
                    Xmp(work, start, stop);
                    break;
                case -1 or 0x0423 or 0x0409 or 0x040C:
                    work.Zero(start, stop);
                    break;
                default:
                    break;
            }

            at = stop + (int)(size % 2);
        }
    }
}
