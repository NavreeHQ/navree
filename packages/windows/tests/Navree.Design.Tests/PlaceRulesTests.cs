using System.Buffers.Binary;
using System.Text;
using Navree.Design.Inputs;

namespace Navree.Design.Tests;

/// <summary>The place a photograph was taken is taken out of it where it is written, and nothing else in the picture changes.</summary>
[TestClass]
public sealed class PlaceRulesTests
{
    // A TIFF structure of the kind an EXIF block is: one directory with the maker's name and a pointer to a GPS directory that holds a latitude
    // reference (inline) and a latitude of three rationals (out of line). Returns the bytes and where the GPS directory and its values are.
    private static (byte[] Block, int GpsAt, int GpsEnd, int ValueAt) Exif(bool little, bool withGps = true)
    {
        using MemoryStream stream = new();
        void U16(int at, int value)
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

        void U32(int at, long value)
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

        stream.Write(Encoding.ASCII.GetBytes(little ? "II" : "MM"));
        U16(2, 42);
        U32(4, 8);

        // IFD0 at 8: two entries (Make, GPS pointer), then the next directory (none).
        int entries = withGps ? 2 : 1;
        U16(8, entries);
        int make = 8 + 2 + (12 * entries) + 4;
        int gps = make + 8;
        U16(10, 0x010F);
        U16(12, 2);
        U32(14, 8);
        U32(18, make);
        if (withGps)
        {
            U16(22, 0x8825);
            U16(24, 4);
            U32(26, 1);
            U32(30, gps);
        }

        U32(10 + (12 * entries), 0);
        stream.Position = make;
        stream.Write(Encoding.ASCII.GetBytes("Charkha\0"));
        int valueAt = 0;
        int gpsEnd = 0;
        if (withGps)
        {
            // GPS directory at gps: two entries, then the next (none), then the three rationals of the latitude.
            valueAt = gps + 2 + 24 + 4;
            U16(gps, 2);
            U16(gps + 2, 1);
            U16(gps + 4, 2);
            U32(gps + 6, 2);
            stream.Position = gps + 10;
            stream.Write("N\0\0\0"u8);
            U16(gps + 14, 2);
            U16(gps + 16, 5);
            U32(gps + 18, 3);
            U32(gps + 22, valueAt);
            U32(gps + 26, 0);
            stream.Position = valueAt;
            stream.Write([30, 0, 0, 0, 1, 0, 0, 0, 54, 0, 0, 0, 1, 0, 0, 0, 7, 0, 0, 0, 1, 0, 0, 0]);
            gpsEnd = gps + 2 + 24 + 4;
        }

        return (stream.ToArray(), gps, gpsEnd, valueAt);
    }

    private static byte[] Jpeg(byte[] block, string xmp = "")
    {
        using MemoryStream stream = new();
        stream.Write([0xFF, 0xD8]);
        void Segment(byte marker, byte[] data)
        {
            stream.Write([0xFF, marker]);
            Span<byte> length = stackalloc byte[2];
            BinaryPrimitives.WriteUInt16BigEndian(length, (ushort)(data.Length + 2));
            stream.Write(length);
            stream.Write(data);
        }

        Segment(0xE0, "JFIF\0\u0001\u0001\0\0\u0001\0\u0001\0\0"u8.ToArray());
        if (block.Length > 0)
        {
            Segment(0xE1, [.. "Exif\0\0"u8.ToArray(), .. block]);
        }

        if (xmp.Length > 0)
        {
            Segment(0xE1, [.. "http://ns.adobe.com/xap/1.0/\0"u8.ToArray(), .. Encoding.UTF8.GetBytes(xmp)]);
        }

        Segment(0xDA, [0, 1, 2, 3]);
        stream.Write([0x11, 0x22, 0x33, 0xFF, 0x00, 0x44, 0xFF, 0xD9]);
        return stream.ToArray();
    }

    private static int Find(byte[] whole, byte[] part) => whole.AsSpan().IndexOf(part);

    [TestMethod]
    [DataRow(true)]
    [DataRow(false)]
    public void The_place_is_taken_out_of_a_jpeg_and_nothing_else_moves(bool little)
    {
        (byte[] block, int gps, int gpsEnd, int valueAt) = Exif(little);
        byte[] file = Jpeg(block);
        byte[] original = [.. file];
        int offset = Find(file, block);

        PlaceResult result = PlaceRules.TakeOut(file, "Ludhiana-market.jpg");

        Assert.AreEqual(PlaceOutcome.Removed, result.Outcome);
        Assert.AreEqual(original.Length, file.Length);
        for (int at = 0; at < file.Length; at++)
        {
            bool inGps = at >= offset + gps && at < offset + gpsEnd;
            bool inValue = at >= offset + valueAt && at < offset + valueAt + 24;
            if (inGps || inValue)
            {
                Assert.AreEqual(0, file[at], "a byte of the place was left at " + at);
            }
            else
            {
                Assert.AreEqual(original[at], file[at], "a byte outside the place changed at " + at);
            }
        }

        Assert.IsTrue(Find(file, "Charkha"u8.ToArray()) >= 0, "the maker's name stays");
        Assert.AreEqual(PlaceOutcome.None, PlaceRules.TakeOut(file, "Ludhiana-market.jpg").Outcome, "taken out once, there is nothing left to take");
    }

    [TestMethod]
    public void A_jpeg_with_no_place_is_not_changed()
    {
        byte[] file = Jpeg(Exif(true, withGps: false).Block);
        byte[] original = [.. file];
        Assert.AreEqual(PlaceOutcome.None, PlaceRules.TakeOut(file, "Amritsar.jpeg").Outcome);
        CollectionAssert.AreEqual(original, file);
        byte[] none = Jpeg([]);
        Assert.AreEqual(PlaceOutcome.None, PlaceRules.TakeOut(none, "plain.JPG").Outcome);
    }

    private const string EmptyPacket = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>";

    private static string Packet(string properties, string elements = "") =>
        "<?xpacket begin=\"﻿\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">"
        + "<rdf:Description rdf:about=\"\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" xmp:Rating=\"5\" " + properties + ">" + elements + "</rdf:Description></rdf:RDF></x:xmpmeta>"
        + new string(' ', 200) + "<?xpacket end=\"w\"?>";

    // The XMP packet's bytes inside a JPEG made by Jpeg(): from after the namespace of its APP1 segment to the segment's end.
    private static string PacketIn(byte[] file)
    {
        int name = Find(file, "http://ns.adobe.com/xap/1.0/\0"u8.ToArray());
        int start = name + "http://ns.adobe.com/xap/1.0/\0".Length;
        int length = (file[name - 2] << 8 | file[name - 1]) - 2 - "http://ns.adobe.com/xap/1.0/\0".Length;
        return Encoding.UTF8.GetString(file, start, length);
    }

    [TestMethod]
    [DataRow("exif:GPSLatitude=\"30,44.0N\" exif:GPSLongitude=\"76,46.8E\"", "", "30,44.0N")]
    [DataRow("drone-dji:GpsLatitude=\"+30.7333\" drone-dji:GpsLongtitude=\"+76.7794\" drone-dji:AbsoluteAltitude=\"+350.20\"", "", "30.7333")]
    [DataRow("exif:GPSLatitude = \"30,44.0N\"", "", "30,44.0N")]
    [DataRow("exif:GPSLatitude\t=\r\n'30,44.0N'", "", "30,44.0N")]
    [DataRow("EXIF:gpslatitude=\"30,44.0N\"", "", "30,44.0N")]
    [DataRow("", "<exif:GPSLatitude>30,44.0N</exif:GPSLatitude>", "30,44.0N")]
    [DataRow("", "<exif:GPSLatitude rdf:parseType=\"Resource\"><rdf:value>30,44.0N</rdf:value></exif:GPSLatitude>", "30,44.0N")]
    [DataRow("", "<GPSLatitude xmlns=\"http://ns.adobe.com/exif/1.0/\">30,44.0N</GPSLatitude>", "30,44.0N")]
    [DataRow("photoshop:City=\"Chandigarh\" photoshop:State=\"Punjab\" photoshop:Country=\"India\"", "", "Chandigarh")]
    [DataRow("", "<Iptc4xmpCore:Location>Sector 17 Plaza</Iptc4xmpCore:Location>", "Sector 17 Plaza")]
    [DataRow("", "<Iptc4xmpExt:LocationCreated><rdf:Bag><rdf:li Iptc4xmpExt:City=\"Chandigarh\" Iptc4xmpExt:Sublocation=\"Sector 17 Plaza\"/></rdf:Bag></Iptc4xmpExt:LocationCreated>", "Sector 17 Plaza")]
    [DataRow("", "<Iptc4xmpCore:CreatorContactInfo Iptc4xmpCore:CiAdrCity=\"Chandigarh\" Iptc4xmpCore:CiAdrExtadr=\"Sector 17 Plaza\"/>", "Sector 17 Plaza")]
    [DataRow("dc:coverage=\"Sector 17 Plaza, Chandigarh\"", "", "Sector 17 Plaza")]
    [DataRow("xmpDM:shotLocation=\"Sector 17 Plaza\"", "", "Sector 17 Plaza")]
    public void An_xmp_packet_that_holds_anything_about_a_place_goes_whole_and_keeps_its_length(string properties, string elements, string place)
    {
        string xmp = Packet(properties, elements);
        byte[] file = Jpeg([], xmp);
        int length = file.Length;

        PlaceResult result = PlaceRules.TakeOut(file, "Chandigarh.jpg");

        Assert.AreEqual(PlaceOutcome.Removed, result.Outcome);
        Assert.AreEqual(length, file.Length);
        string after = PacketIn(file);
        Assert.AreEqual(Encoding.UTF8.GetByteCount(xmp), Encoding.UTF8.GetByteCount(after), "the packet keeps its length");
        Assert.AreEqual(EmptyPacket + new string(' ', after.Length - EmptyPacket.Length), after, "the packet is an empty one, padded with spaces");
        Assert.IsFalse(Encoding.UTF8.GetString(file).Contains(place, StringComparison.Ordinal));
    }

    [TestMethod]
    [DataRow("hdrgm:Version=\"1.0\" hdrgm:HDRCapacityMin=\"0\" hdrgm:HDRCapacityMax=\"4.7\" crs:MaskOpacity=\"50\" xmpMM:InstanceID=\"xmp.iid:1\"", "")]
    [DataRow("", "<dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">Lohri in the city, with the whole family</rdf:li></rdf:Alt></dc:description>")]
    public void An_xmp_packet_with_nothing_about_a_place_is_left_as_it_was(string properties, string elements)
    {
        byte[] file = Jpeg([], Packet(properties, elements));
        byte[] original = [.. file];

        Assert.AreEqual(PlaceOutcome.None, PlaceRules.TakeOut(file, "Lohri.jpg").Outcome);
        CollectionAssert.AreEqual(original, file);
    }

    [TestMethod]
    public void An_xmp_packet_about_a_place_goes_whole_from_a_png_and_from_a_webp_too()
    {
        byte[] packet = Encoding.UTF8.GetBytes(Packet("drone-dji:GpsLatitude=\"+30.7333\" drone-dji:GpsLongitude=\"+76.7794\""));
        byte[] itxt = [.. "XML:com.adobe.xmp"u8.ToArray(), 0, 0, 0, 0, 0, .. packet];
        byte[] png = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, .. Chunk("IHDR", [0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]), .. Chunk("iTXt", itxt), .. Chunk("IDAT", [1, 2, 3]), .. Chunk("IEND", [])];
        byte[] chunk = [.. "XMP "u8.ToArray(), .. BitConverter.GetBytes((uint)packet.Length), .. packet, .. (packet.Length % 2 == 1 ? new byte[] { 0 } : [])];
        byte[] inner = [.. "WEBP"u8.ToArray(), .. "VP8 "u8.ToArray(), 4, 0, 0, 0, 9, 8, 7, 6, .. chunk];
        byte[] webp = [.. "RIFF"u8.ToArray(), .. BitConverter.GetBytes((uint)inner.Length), .. inner];

        Assert.AreEqual(PlaceOutcome.Removed, PlaceRules.TakeOut(png, "Chandigarh.png").Outcome);
        Assert.AreEqual(PlaceOutcome.Removed, PlaceRules.TakeOut(webp, "Chandigarh.webp").Outcome);
        Assert.IsFalse(Encoding.UTF8.GetString(png).Contains("30.7333", StringComparison.Ordinal));
        Assert.IsFalse(Encoding.UTF8.GetString(webp).Contains("30.7333", StringComparison.Ordinal));
        int itxtAt = Find(png, "iTXt"u8.ToArray());
        Assert.AreEqual(PlaceRules.Crc32(png.AsSpan(itxtAt, 4 + itxt.Length)), BinaryPrimitives.ReadUInt32BigEndian(png.AsSpan(itxtAt + 4 + itxt.Length)), "the chunk's check value matches what it now holds");
    }

    [TestMethod]
    public void An_xmp_packet_that_is_not_utf8_is_refused_because_it_cannot_be_read()
    {
        byte[] file = Jpeg([]);
        byte[] packet = Encoding.Unicode.GetBytes(Packet("exif:GPSLatitude=\"30,44.0N\""));
        byte[] withXmp = [.. file[..20], 0xFF, 0xE1, (byte)((packet.Length + 31) >> 8), (byte)(packet.Length + 31), .. "http://ns.adobe.com/xap/1.0/\0"u8.ToArray(), .. packet, .. file[20..]];

        Assert.AreEqual(PlaceOutcome.Refused, PlaceRules.TakeOut(withXmp, "Mohali.jpg").Outcome);
    }

    private static byte[] Chunk(string kind, byte[] data)
    {
        byte[] whole = new byte[12 + data.Length];
        BinaryPrimitives.WriteUInt32BigEndian(whole, (uint)data.Length);
        Encoding.ASCII.GetBytes(kind).CopyTo(whole, 4);
        data.CopyTo(whole, 8);
        BinaryPrimitives.WriteUInt32BigEndian(whole.AsSpan(8 + data.Length), PlaceRules.Crc32(whole.AsSpan(4, 4 + data.Length)));
        return whole;
    }

    [TestMethod]
    public void The_place_is_taken_out_of_a_png_and_the_check_value_of_its_chunk_is_made_again()
    {
        (byte[] block, _, _, _) = Exif(false);
        byte[] header = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        byte[] ihdr = Chunk("IHDR", [0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]);
        byte[] exif = Chunk("eXIf", block);
        byte[] data = Chunk("IDAT", [1, 2, 3, 4, 5]);
        byte[] end = Chunk("IEND", []);
        byte[] file = [.. header, .. ihdr, .. exif, .. data, .. end];
        int exifAt = header.Length + ihdr.Length;

        PlaceResult result = PlaceRules.TakeOut(file, "Jalandhar.png");

        Assert.AreEqual(PlaceOutcome.Removed, result.Outcome);
        Assert.AreEqual(header.Length + ihdr.Length + exif.Length + data.Length + end.Length, file.Length);
        uint stored = BinaryPrimitives.ReadUInt32BigEndian(file.AsSpan(exifAt + 8 + block.Length));
        Assert.AreEqual(PlaceRules.Crc32(file.AsSpan(exifAt + 4, 4 + block.Length)), stored, "the chunk's check value matches what it now holds");
        CollectionAssert.AreEqual(data, file[(exifAt + exif.Length)..(exifAt + exif.Length + data.Length)], "the pixels are as they were");
    }

    [TestMethod]
    public void The_check_value_is_the_one_png_uses()
    {
        // The check value of the type IEND with no data, which every PNG ends with.
        Assert.AreEqual(0xAE426082u, PlaceRules.Crc32("IEND"u8));
    }

    [TestMethod]
    public void A_png_with_its_xmp_compressed_is_refused_because_the_place_cannot_be_looked_for()
    {
        byte[] keyword = [.. "XML:com.adobe.xmp"u8.ToArray(), 0, 1, 0, 0, 0, 9, 9];
        byte[] file = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, .. Chunk("iTXt", keyword), .. Chunk("IEND", [])];
        Assert.AreEqual(PlaceOutcome.Refused, PlaceRules.TakeOut(file, "Moga.png").Outcome);
    }

    [TestMethod]
    public void The_place_is_taken_out_of_a_webp_with_or_without_the_six_bytes_a_jpeg_puts_before_the_block()
    {
        foreach (bool prefixed in new[] { false, true })
        {
            (byte[] block, int gps, int gpsEnd, int valueAt) = Exif(true);
            byte[] body = prefixed ? [.. "Exif\0\0"u8.ToArray(), .. block] : block;
            byte[] exif = [.. "EXIF"u8.ToArray(), .. BitConverter.GetBytes((uint)body.Length), .. body, .. (body.Length % 2 == 1 ? new byte[] { 0 } : [])];
            byte[] pixels = [.. "VP8 "u8.ToArray(), 4, 0, 0, 0, 9, 8, 7, 6];
            byte[] inner = [.. "WEBP"u8.ToArray(), .. exif, .. pixels];
            byte[] file = [.. "RIFF"u8.ToArray(), .. BitConverter.GetBytes((uint)inner.Length), .. inner];
            byte[] original = [.. file];
            int offset = Find(file, block);

            PlaceResult result = PlaceRules.TakeOut(file, "Bathinda.webp");

            Assert.AreEqual(PlaceOutcome.Removed, result.Outcome, "prefixed: " + prefixed);
            for (int at = 0; at < file.Length; at++)
            {
                bool changed = at >= offset + gps && at < offset + gpsEnd || at >= offset + valueAt && at < offset + valueAt + 24;
                Assert.AreEqual(changed ? 0 : original[at], file[at], "byte " + at + ", prefixed: " + prefixed);
            }
        }
    }

    [TestMethod]
    public void The_place_is_taken_out_of_a_tiff()
    {
        (byte[] block, int gps, int gpsEnd, int valueAt) = Exif(false);
        byte[] original = [.. block];
        Assert.AreEqual(PlaceOutcome.Removed, PlaceRules.TakeOut(block, "scan.tif").Outcome);
        for (int at = 0; at < block.Length; at++)
        {
            bool changed = at >= gps && at < gpsEnd || at >= valueAt && at < valueAt + 24;
            Assert.AreEqual(changed ? 0 : original[at], block[at]);
        }
    }

    [TestMethod]
    public void A_picture_of_a_kind_the_place_cannot_be_taken_out_of_is_refused_with_words_and_a_file_that_is_not_a_picture_is_left_alone()
    {
        byte[] heic = [0, 0, 0, 24, .. "ftypheic"u8.ToArray(), 0, 0, 0, 0, .. "mif1heic"u8.ToArray()];
        PlaceResult refused = PlaceRules.TakeOut(heic, "Chandigarh.heic");
        Assert.AreEqual(PlaceOutcome.Refused, refused.Outcome);
        StringAssert.Contains(refused.Why, "HEIC");
        StringAssert.Contains(refused.Why, "JPEG");

        Assert.AreEqual(PlaceOutcome.Refused, PlaceRules.TakeOut([1, 2, 3, 4], "raw.dng").Outcome, "a raw picture is not read");
        Assert.AreEqual(PlaceOutcome.Refused, PlaceRules.TakeOut([1, 2, 3, 4], "broken.jpg").Outcome, "a name that says JPEG over bytes that are not one cannot be checked");
        Assert.AreEqual(PlaceOutcome.None, PlaceRules.TakeOut([1, 2, 3, 4], "bill.pdf").Outcome);
        Assert.AreEqual(PlaceOutcome.None, PlaceRules.TakeOut([.. "GIF89a"u8.ToArray(), 1, 2, 3], "dance.gif").Outcome);
        Assert.AreEqual(PlaceOutcome.Refused, PlaceRules.TakeOut([.. "GIF89a"u8.ToArray(), .. "exif:GPSLatitude"u8.ToArray()], "dance.gif").Outcome, "a place written into a GIF is found by its words and refused");
    }

    [TestMethod]
    [DataRow(new byte[] { 0xFF, 0xD8, 0xFF, 0xE0 }, PictureKind.Jpeg)]
    [DataRow(new byte[] { 0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A }, PictureKind.Png)]
    [DataRow(new byte[] { 0x52, 0x49, 0x46, 0x46, 4, 0, 0, 0, 0x57, 0x45, 0x42, 0x50 }, PictureKind.Webp)]
    [DataRow(new byte[] { 0x4D, 0x4D, 0, 0x2A, 0, 0, 0, 8 }, PictureKind.Tiff)]
    [DataRow(new byte[] { 0x49, 0x49, 0x2B, 0, 8, 0, 0, 0 }, PictureKind.BigTiff)]
    [DataRow(new byte[] { 0x47, 0x49, 0x46, 0x38, 0x39, 0x61 }, PictureKind.Gif)]
    [DataRow(new byte[] { 0, 0, 0, 16, 0x66, 0x74, 0x79, 0x70, 0x61, 0x76, 0x69, 0x66, 0, 0, 0, 0 }, PictureKind.Avif)]
    [DataRow(new byte[] { 0, 0, 0, 20, 0x66, 0x74, 0x79, 0x70, 0x6D, 0x69, 0x66, 0x31, 0, 0, 0, 0, 0x68, 0x65, 0x69, 0x63 }, PictureKind.Heic)]
    [DataRow(new byte[] { 0, 0, 0, 20, 0x66, 0x74, 0x79, 0x70, 0x71, 0x74, 0x20, 0x20, 0, 0, 0, 0, 0x71, 0x74, 0x20, 0x20 }, PictureKind.NotAPicture)]
    [DataRow(new byte[] { 0, 0, 0, 20, 0x66, 0x74, 0x79, 0x70, 0x63, 0x72, 0x78, 0x20, 0, 0, 0, 0, 0x69, 0x73, 0x6F, 0x6D }, PictureKind.OtherMedia)]
    [DataRow(new byte[] { 0xFF, 0x0A, 0xFA, 0x7F }, PictureKind.JpegXl)]
    [DataRow(new byte[] { 0x25, 0x50, 0x44, 0x46, 0x2D }, PictureKind.NotAPicture)]
    [DataRow(new byte[] { 0xEF, 0xBB, 0xBF, 0x0A, 0x3C, 0x3F, 0x78, 0x6D, 0x6C, 0x3F, 0x3E, 0x3C, 0x53, 0x56, 0x47 }, PictureKind.Svg)]
    public void The_kind_of_a_file_is_read_from_its_first_bytes(byte[] head, PictureKind kind)
    {
        Assert.AreEqual(kind, PlaceKinds.KindOf(head));
    }

    [TestMethod]
    [DataRow("Golden Temple.jfif")]
    [DataRow("Golden Temple")]
    [DataRow("Golden Temple.txt")]
    public void A_jpeg_is_cleaned_whatever_its_name_says(string name)
    {
        (byte[] block, _, _, _) = Exif(true);
        byte[] file = Jpeg(block);
        Assert.AreEqual(PlaceOutcome.Removed, PlaceRules.TakeOut(file, name).Outcome);
    }

    [TestMethod]
    public void A_jpeg_whose_place_runs_past_its_end_is_refused_not_sent()
    {
        (byte[] block, _, _, _) = Exif(true);
        // Cut the block short, in the middle of the GPS directory.
        byte[] file = Jpeg(block[..^20]);
        Assert.AreEqual(PlaceOutcome.Refused, PlaceRules.TakeOut(file, "cut.jpg").Outcome);
    }
}
