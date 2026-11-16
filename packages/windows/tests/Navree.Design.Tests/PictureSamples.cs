using System.Buffers.Binary;
using System.IO.Compression;
using System.Text;
using Navree.Design.Inputs;

namespace Navree.Design.Tests;

/// <summary>
/// Small pictures made byte by byte, so no photo of anybody is kept in the repository. Every place in them is a city's centre: Chandigarh's
/// (30.7333 N, 76.7794 E), written as the formats write it.
/// </summary>
internal static class PictureSamples
{
    /// <summary>Chandigarh's latitude as an EXIF GPS value: 30 degrees, 44 minutes, 0 seconds, three rationals, little endian.</summary>
    internal static readonly byte[] LatitudeRationals = [30, 0, 0, 0, 1, 0, 0, 0, 44, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0];

    /// <summary>The words of a place written in XMP and IPTC in these samples.</summary>
    internal const string PlaceWords = "Sector 17 Plaza";

    /// <summary>The compressed picture data every sample JPEG holds (made so that it holds no 0xFF byte but a stuffed one and a restart marker).</summary>
    internal static readonly byte[] ScanData = [0x11, 0x22, 0x33, 0xFF, 0x00, 0x44, 0x55, 0xFF, 0xD0, 0x66, 0x77, 0x88, 0x99];

    /// <summary>A second scan's data, for a picture with two scans.</summary>
    internal static readonly byte[] SecondScanData = [0x21, 0x32, 0x43, 0xFF, 0x00, 0x54, 0x65];

    /// <summary>A TIFF structure as an EXIF block holds it (little endian): IFD0 with the maker's name, an Exif IFD with a maker's note, and a GPS IFD with a latitude.</summary>
    /// <param name="withGps">Whether it points to a GPS directory.</param>
    /// <param name="makerNote">The bytes of the maker's note, or none.</param>
    /// <param name="thumbnail">A JPEG thumbnail for IFD1, or none.</param>
    /// <returns>The block.</returns>
    internal static byte[] ExifBlock(bool withGps = true, byte[]? makerNote = null, byte[]? thumbnail = null)
    {
        Tiff tiff = new(true);
        List<(ushort Tag, ushort Type, uint Count, object Value)> ifd0 = [(0x010F, 2, 8, "Charkha\0"u8.ToArray())];
        List<(ushort, ushort, uint, object)> exif = [(0x9003, 2, 20, "2026:11:16 10:15:00\0"u8.ToArray())];
        if (makerNote is not null)
        {
            exif.Add((0x927C, 7, (uint)makerNote.Length, makerNote));
        }

        ifd0.Add((0x8769, 4, 1, exif));
        if (withGps)
        {
            ifd0.Add((0x8825, 4, 1, new List<(ushort, ushort, uint, object)> { (0x0001, 2, 2, "N\0"u8.ToArray()), (0x0002, 5, 3, LatitudeRationals) }));
        }

        List<(ushort, ushort, uint, object)>? ifd1 = thumbnail is null ? null : [(0x0201, 4, 1, new Pointer(thumbnail)), (0x0202, 4, 1, (uint)thumbnail.Length)];
        return tiff.Write(ifd0, ifd1);
    }

    /// <summary>A JPEG made of the segments given, then a scan of <see cref="ScanData"/>, then EOI.</summary>
    /// <param name="segments">Each segment's marker and data (without its length).</param>
    /// <returns>The file.</returns>
    internal static byte[] Jpeg(params (byte Marker, byte[] Data)[] segments)
    {
        using MemoryStream stream = new();
        stream.Write([0xFF, 0xD8]);
        Segment(stream, 0xE0, "JFIF\0\u0001\u0001\0\0\u0001\0\u0001\0\0"u8.ToArray());
        foreach ((byte marker, byte[] data) in segments)
        {
            Segment(stream, marker, data);
        }

        Segment(stream, 0xDB, new byte[65]);
        Segment(stream, 0xC0, [8, 0, 1, 0, 1, 1, 1, 0x11, 0]);
        Segment(stream, 0xDA, [1, 1, 0, 0, 0x3F, 0]);
        stream.Write(ScanData);
        stream.Write([0xFF, 0xD9]);
        return stream.ToArray();
    }

    /// <summary>A JPEG of two scans with the segments given between them, as a progressive picture may have.</summary>
    /// <param name="between">The segments between the scans.</param>
    /// <returns>The file.</returns>
    internal static byte[] JpegOfTwoScans(params (byte Marker, byte[] Data)[] between)
    {
        byte[] one = Jpeg();
        using MemoryStream stream = new();
        stream.Write(one.AsSpan(0, one.Length - 2));
        foreach ((byte marker, byte[] data) in between)
        {
            Segment(stream, marker, data);
        }

        Segment(stream, 0xDA, [1, 1, 0, 0, 0x3F, 0]);
        stream.Write(SecondScanData);
        stream.Write([0xFF, 0xD9]);
        return stream.ToArray();
    }

    /// <summary>Writes one segment: its marker, its length, its data.</summary>
    internal static void Segment(Stream stream, byte marker, byte[] data)
    {
        stream.Write([0xFF, marker, (byte)((data.Length + 2) >> 8), (byte)(data.Length + 2)]);
        stream.Write(data);
    }

    /// <summary>The data of an EXIF APP1 segment.</summary>
    internal static byte[] ExifSegment(byte[] block) => [.. "Exif\0\0"u8.ToArray(), .. block];

    /// <summary>The data of an XMP APP1 segment.</summary>
    internal static byte[] XmpSegment(string packet) => [.. "http://ns.adobe.com/xap/1.0/\0"u8.ToArray(), .. Encoding.UTF8.GetBytes(packet)];

    /// <summary>The data of one extended XMP APP1 segment: the signature, the GUID, the full length, this piece's offset, the piece.</summary>
    internal static byte[] ExtendedXmpSegment(byte[] whole, int offset, int length)
    {
        byte[] header = new byte[8];
        BinaryPrimitives.WriteUInt32BigEndian(header, (uint)whole.Length);
        BinaryPrimitives.WriteUInt32BigEndian(header.AsSpan(4), (uint)offset);
        return [.. "http://ns.adobe.com/xmp/extension/\0"u8.ToArray(), .. "0123456789ABCDEF0123456789ABCDEF"u8.ToArray(), .. header, .. whole.AsSpan(offset, length)];
    }

    /// <summary>An XMP packet holding the properties and elements given.</summary>
    internal static string Packet(string properties, string elements = "") =>
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\" "
        + properties + ">" + elements + "</rdf:Description></rdf:RDF></x:xmpmeta>";

    /// <summary>IPTC IIM datasets of record 2: the object name and the city.</summary>
    internal static byte[] Iptc(string title, string city)
    {
        using MemoryStream stream = new();
        void Set(byte record, byte number, string value)
        {
            byte[] bytes = Encoding.ASCII.GetBytes(value);
            stream.Write([0x1C, record, number, (byte)(bytes.Length >> 8), (byte)bytes.Length]);
            stream.Write(bytes);
        }

        Set(2, 5, title);
        Set(2, 90, city);
        Set(2, 92, PlaceWords);
        return stream.ToArray();
    }

    /// <summary>Photoshop image resources: each an "8BIM" resource of the id given, with an empty name.</summary>
    internal static byte[] Resources(params (ushort Id, byte[] Data)[] resources)
    {
        using MemoryStream stream = new();
        foreach ((ushort id, byte[] data) in resources)
        {
            stream.Write("8BIM"u8);
            stream.Write([(byte)(id >> 8), (byte)id, 0, 0, (byte)(data.Length >> 24), (byte)(data.Length >> 16), (byte)(data.Length >> 8), (byte)data.Length]);
            stream.Write(data);
            if (data.Length % 2 == 1)
            {
                stream.WriteByte(0);
            }
        }

        return stream.ToArray();
    }

    /// <summary>The bytes of a JPEG's compressed picture data: everything from the end of each SOS header to the marker that ends its scan.</summary>
    internal static byte[] Scans(byte[] file)
    {
        using MemoryStream stream = new();
        int at = 2;
        while (at + 4 <= file.Length && file[at] == 0xFF)
        {
            byte marker = file[at + 1];
            if (marker == 0xD9)
            {
                break;
            }

            int length = file[at + 2] << 8 | file[at + 3];
            at += 2 + length;
            if (marker == 0xDA)
            {
                int start = at;
                while (!(file[at] == 0xFF && file[at + 1] != 0 && file[at + 1] is < 0xD0 or > 0xD7))
                {
                    at++;
                }

                stream.Write(file.AsSpan(start, at - start));
            }
        }

        return stream.ToArray();
    }

    // ---- PNG and WebP -------------------------------------------------------------------------------------------

    /// <summary>One PNG chunk with its check value.</summary>
    internal static byte[] Chunk(string kind, byte[] data)
    {
        byte[] whole = new byte[12 + data.Length];
        BinaryPrimitives.WriteUInt32BigEndian(whole, (uint)data.Length);
        Encoding.ASCII.GetBytes(kind).CopyTo(whole, 4);
        data.CopyTo(whole, 8);
        BinaryPrimitives.WriteUInt32BigEndian(whole.AsSpan(8 + data.Length), PlaceRules.Crc32(whole.AsSpan(4, 4 + data.Length)));
        return whole;
    }

    /// <summary>The PNG signature.</summary>
    internal static readonly byte[] PngSignature = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

    /// <summary>A PNG of one pixel's header and data with the chunks given between them.</summary>
    internal static byte[] Png(params byte[][] chunks) =>
        [.. PngSignature, .. Chunk("IHDR", [0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]), .. chunks.SelectMany(chunk => chunk), .. Chunk("IDAT", PngPixels), .. Chunk("IEND", [])];

    /// <summary>The data of the sample PNG's IDAT chunk.</summary>
    internal static readonly byte[] PngPixels = [0x78, 0x9C, 0x63, 0x60, 0x60, 0x60, 0x00, 0x00, 0x00, 0x04, 0x00, 0x01];

    /// <summary>zlib-compressed bytes.</summary>
    internal static byte[] Deflated(byte[] bytes)
    {
        using MemoryStream stream = new();
        using (ZLibStream zlib = new(stream, CompressionLevel.Optimal, true))
        {
            zlib.Write(bytes);
        }

        return stream.ToArray();
    }

    /// <summary>A WebP of a tiny VP8 frame followed by the chunks given.</summary>
    internal static byte[] Webp(params (string Kind, byte[] Data)[] chunks)
    {
        using MemoryStream inner = new();
        inner.Write("WEBP"u8);
        foreach ((string kind, byte[] data) in (IEnumerable<(string, byte[])>)[("VP8 ", WebpPixels), .. chunks])
        {
            inner.Write(Encoding.ASCII.GetBytes(kind));
            inner.Write(BitConverter.GetBytes((uint)data.Length));
            inner.Write(data);
            if (data.Length % 2 == 1)
            {
                inner.WriteByte(0);
            }
        }

        byte[] body = inner.ToArray();
        return [.. "RIFF"u8.ToArray(), .. BitConverter.GetBytes((uint)body.Length), .. body];
    }

    /// <summary>The sample WebP's frame bytes.</summary>
    internal static readonly byte[] WebpPixels = [0x9D, 0x01, 0x2A, 1, 0, 1, 0, 7, 6, 5];

    // ---- TIFF ---------------------------------------------------------------------------------------------------

    /// <summary>A value an IFD entry points to that is written elsewhere and given by its offset.</summary>
    internal sealed record Pointer(byte[] Data);

    /// <summary>Writes a TIFF structure: directories given as lists of entries, sub-directories as nested lists, out-of-line values after them.</summary>
    internal sealed class Tiff(bool little)
    {
        private readonly MemoryStream stream = new();

        private void U16(long at, int value)
        {
            Span<byte> two = stackalloc byte[2];
            if (little)
            {
                BinaryPrimitives.WriteUInt16LittleEndian(two, (ushort)value);
            }
            else
            {
                BinaryPrimitives.WriteUInt16BigEndian(two, (ushort)value);
            }

            stream.Position = at;
            stream.Write(two);
        }

        private void U32(long at, long value)
        {
            Span<byte> four = stackalloc byte[4];
            if (little)
            {
                BinaryPrimitives.WriteUInt32LittleEndian(four, (uint)value);
            }
            else
            {
                BinaryPrimitives.WriteUInt32BigEndian(four, (uint)value);
            }

            stream.Position = at;
            stream.Write(four);
        }

        private long Append(byte[] data)
        {
            long at = stream.Length;
            if (at % 2 == 1)
            {
                stream.Position = at;
                stream.WriteByte(0);
                at++;
            }

            stream.Position = at;
            stream.Write(data);
            return at;
        }

        /// <summary>The whole structure, with its header.</summary>
        internal byte[] Write(List<(ushort Tag, ushort Type, uint Count, object Value)> first, List<(ushort Tag, ushort Type, uint Count, object Value)>? second = null, byte[]? before = null)
        {
            stream.Write(Encoding.ASCII.GetBytes(little ? "II" : "MM"));
            U16(2, 42);
            U32(4, 0);
            if (before is not null)
            {
                _ = Append(before);
            }

            long ifd0 = Directory(first);
            U32(4, ifd0);
            if (second is not null)
            {
                long ifd1 = Directory(second);
                U32(ifd0 + 2 + (12 * first.Count), ifd1);
            }

            return stream.ToArray();
        }

        private long Directory(List<(ushort Tag, ushort Type, uint Count, object Value)> entries)
        {
            long at = Append(new byte[2 + (12 * entries.Count) + 4]);
            U16(at, entries.Count);
            int index = 0;
            foreach ((ushort tag, ushort type, uint count, object value) in entries.OrderBy(entry => entry.Tag))
            {
                long entry = at + 2 + (12 * index++);
                U16(entry, tag);
                U16(entry + 2, type);
                U32(entry + 4, count);
                long field = entry + 8;
                switch (value)
                {
                    case uint number:
                        U32(field, number);
                        break;
                    case List<(ushort, ushort, uint, object)> inner:
                        U32(field, Directory(inner));
                        break;
                    case Pointer pointer:
                        U32(field, Append(pointer.Data));
                        break;
                    case byte[] bytes when bytes.Length <= 4:
                        stream.Position = field;
                        stream.Write(bytes);
                        break;
                    case byte[] bytes:
                        U32(field, Append(bytes));
                        break;
                    default:
                        throw new ArgumentException("an entry's value of an unknown sort");
                }
            }

            return at;
        }
    }
}
