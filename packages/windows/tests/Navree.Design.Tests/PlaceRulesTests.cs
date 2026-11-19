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

        PlaceResult fromPng = PlaceRules.TakeOut(png, "Chandigarh.png");
        PlaceResult fromWebp = PlaceRules.TakeOut(webp, "Chandigarh.webp");

        Assert.AreEqual(PlaceOutcome.Removed, fromPng.Outcome);
        Assert.AreEqual(PlaceOutcome.Removed, fromWebp.Outcome);
        CollectionAssert.AreEqual(png[..(8 + 25)].Concat(png[^(15 + 12)..]).ToArray(), fromPng.Bytes, "the PNG's chunk that held the packet goes, and nothing else changes");
        Assert.IsFalse(Encoding.UTF8.GetString(fromWebp.Bytes!).Contains("30.7333", StringComparison.Ordinal));
        Assert.AreEqual(webp.Length, fromWebp.Bytes!.Length, "the WebP's packet is emptied where it is");
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
    public void A_png_whose_compressed_xmp_cannot_be_expanded_is_refused()
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
    [DataRow(new byte[] { 0x38, 0x42, 0x50, 0x53, 0, 1, 0, 0, 0, 0, 0, 0, 0, 3 }, PictureKind.Photoshop)]
    [DataRow(new byte[] { 0x46, 0x55, 0x4A, 0x49, 0x46, 0x49, 0x4C, 0x4D, 0x43, 0x43, 0x44, 0x2D, 0x52, 0x41, 0x57 }, PictureKind.Raw)]
    [DataRow(new byte[] { 0x46, 0x4F, 0x56, 0x62, 0, 0, 3, 0 }, PictureKind.Raw)]
    [DataRow(new byte[] { 0, 0x4D, 0x52, 0x4D, 0, 0, 0, 8 }, PictureKind.Raw)]
    [DataRow(new byte[] { 0x49, 0x49, 0x1A, 0, 0, 0, 0x48, 0x45, 0x41, 0x50, 0x43, 0x43, 0x44, 0x52 }, PictureKind.Raw)]
    [DataRow(new byte[] { 0x49, 0x49, 0x52, 0x4F, 8, 0, 0, 0 }, PictureKind.Raw)]
    [DataRow(new byte[] { 0x49, 0x49, 0x52, 0x53, 8, 0, 0, 0 }, PictureKind.Raw)]
    [DataRow(new byte[] { 0x4D, 0x4D, 0x4F, 0x52, 0, 0, 0, 8 }, PictureKind.Raw)]
    [DataRow(new byte[] { 0x49, 0x49, 0x55, 0, 0x18, 0, 0, 0 }, PictureKind.Raw)]
    [DataRow(new byte[] { 0x49, 0x49, 0x2A, 0, 0x10, 0, 0, 0, 0x43, 0x52, 2, 0 }, PictureKind.Raw)]
    [DataRow(new byte[] { 0x49, 0x49, 0x2A, 0, 8, 0, 0, 0, 1, 0 }, PictureKind.Tiff)]
    [DataRow(new byte[] { 0x67, 0x69, 0x6D, 0x70, 0x20, 0x78, 0x63, 0x66, 0x20, 0x76, 0x30, 0x31, 0x31, 0 }, PictureKind.Gimp)]
    [DataRow(new byte[] { 0x67, 0x69, 0x6D, 0x70, 0x20, 0x78, 0x63, 0x66, 0x20, 0x66, 0x69, 0x6C, 0x65, 0 }, PictureKind.Gimp)]
    [DataRow(new byte[] { 0x67, 0x69, 0x6D, 0x70, 0x20, 0x78, 0x63, 0x66 }, PictureKind.NotAPicture)]
    [DataRow(new byte[] { 0x49, 0x49, 0xBC, 1, 0x20, 0, 0, 0 }, PictureKind.JpegXr)]
    [DataRow(new byte[] { 0x4D, 0x4D, 0xBC, 1, 0, 0, 0, 0x20 }, PictureKind.NotAPicture)]
    public void The_kind_of_a_file_is_read_from_its_first_bytes(byte[] head, PictureKind kind)
    {
        Assert.AreEqual(kind, PlaceKinds.KindOf(head));
    }

    [TestMethod]
    public void A_photoshop_document_and_a_raw_picture_under_another_name_are_known_by_their_bytes_and_refused_with_words()
    {
        // A Photoshop document made from a phone photo keeps the photo's EXIF block, GPS and all, as one of its resources (1058).
        (byte[] block, _, _, _) = Exif(true);
        byte[] document = [.. "8BPS"u8.ToArray(), 0, 1, 0, 0, 0, 0, 0, 0, 0, 3, .. "8BIM"u8.ToArray(), 0x04, 0x22, 0, 0, 0, 0, (byte)(block.Length >> 8), (byte)block.Length, .. block];
        foreach (string name in new[] { "Wagah border.psd", "Wagah border", "Wagah border.pdf", "Wagah border.jpg" })
        {
            PlaceResult refused = PlaceRules.TakeOut([.. document], name);
            Assert.AreEqual(PlaceOutcome.Refused, refused.Outcome, name + ": a Photoshop document that holds a place is sent as it is");
            StringAssert.Contains(refused.Why, "Photoshop");
            Expect.That(PlaceRules.MustLook(document, name, FileRules.MediaTypeOf(name)), name + " is not looked into before it is sent");
        }

        // A Fujifilm raw picture saved under a name that says nothing, and one that says it is a paper.
        byte[] raw = [.. "FUJIFILMCCD-RAW 0201FF129502X-T2"u8.ToArray(), .. new byte[64]];
        foreach (string name in new[] { "DSCF0412", "DSCF0412.dat", "DSCF0412.pdf" })
        {
            PlaceResult refused = PlaceRules.TakeOut([.. raw], name);
            Assert.AreEqual(PlaceOutcome.Refused, refused.Outcome, name + ": a raw picture under another name is sent as it is");
            StringAssert.Contains(refused.Why, "raw picture");
            Expect.That(PlaceRules.MustLook(raw, name, FileRules.MediaTypeOf(name)), name + " is not looked into before it is sent");
        }

        // An Olympus and a Panasonic raw picture begin almost as a TIFF does: neither is read as a TIFF.
        Assert.AreEqual(PlaceOutcome.Refused, PlaceRules.TakeOut([0x49, 0x49, 0x52, 0x4F, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0], "P1010001.tif").Outcome);
        Assert.AreEqual(PlaceOutcome.Refused, PlaceRules.TakeOut([0x49, 0x49, 0x55, 0, 0x18, 0, 0, 0, 0, 0, 0, 0, 0, 0], "P1010002.tif").Outcome);
    }

    // The first file of a ZIP archive, as its header is laid out: the name's length at byte 26, the length of what follows the name at byte
    // 28, the name at byte 30, then the content, stored as it is.
    private static byte[] Zip(byte[] first, byte[] content, int extra = 0) =>
    [
        .. "PK\u0003\u0004"u8.ToArray(), 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        (byte)content.Length, 0, 0, 0, (byte)content.Length, 0, 0, 0,
        (byte)first.Length, 0, (byte)extra, 0,
        .. first, .. new byte[extra], .. content,
        .. "PK\u0003\u0004"u8.ToArray(), .. new byte[40],
    ];

    [TestMethod]
    public void An_openraster_document_is_known_by_the_first_file_of_its_archive_and_no_other_archive_is_taken_for_one()
    {
        byte[] mimetype = "mimetype"u8.ToArray();
        Assert.AreEqual(PictureKind.OpenRaster, PlaceKinds.KindOf(Zip(mimetype, "image/openraster"u8.ToArray())));

        // Something may stand between the name and the content: the content is found past it.
        Assert.AreEqual(PictureKind.OpenRaster, PlaceKinds.KindOf(Zip(mimetype, "image/openraster"u8.ToArray(), extra: 28)));

        // A book and a spreadsheet begin the same way and say another type; a paper's archive begins with another file; each is no picture.
        Assert.AreEqual(PictureKind.NotAPicture, PlaceKinds.KindOf(Zip(mimetype, "application/epub+zip"u8.ToArray())));
        Assert.AreEqual(PictureKind.NotAPicture, PlaceKinds.KindOf(Zip(mimetype, "application/vnd.oasis.opendocument.spreadsheet"u8.ToArray())));
        Assert.AreEqual(PictureKind.NotAPicture, PlaceKinds.KindOf(Zip("[Content_Types].xml"u8.ToArray(), "image/openraster"u8.ToArray())));
        Assert.AreEqual(PictureKind.NotAPicture, PlaceKinds.KindOf(Zip("mimetypes"u8.ToArray(), "image/openraster"u8.ToArray())));

        // An archive cut short anywhere in its first header is no picture, and reading it throws nothing.
        byte[] whole = Zip(mimetype, "image/openraster"u8.ToArray(), extra: 4);
        for (int length = 0; length < 58; length++)
        {
            Assert.AreEqual(PictureKind.NotAPicture, PlaceKinds.KindOf(whole.AsSpan(0, length)), length + " bytes of an archive are taken for a document");
        }

        // A length that points past the bytes read is no picture either.
        byte[] far = Zip(mimetype, "image/openraster"u8.ToArray());
        far[28] = 0xFF;
        far[29] = 0xFF;
        Assert.AreEqual(PictureKind.NotAPicture, PlaceKinds.KindOf(far));
    }

    [TestMethod]
    public void A_gimp_or_openraster_document_and_a_jpeg_xr_picture_are_known_by_their_bytes_and_refused_with_words_under_any_name()
    {
        (byte[] block, _, _, _) = Exif(true);

        // A GIMP document made from a phone photo, with the photo's EXIF block somewhere inside it.
        byte[] gimp = [.. "gimp xcf v011"u8.ToArray(), 0, 0, 0, 0x0F, 0xA0, 0, 0, 0x0B, 0xB8, .. block];
        foreach (string name in new[] { "Rock Garden.xcf", "Rock Garden", "Rock Garden.pdf", "Rock Garden.png" })
        {
            PlaceResult refused = PlaceRules.TakeOut([.. gimp], name);
            Assert.AreEqual(PlaceOutcome.Refused, refused.Outcome, name + ": a GIMP document is sent as it is");
            StringAssert.Contains(refused.Why, "a GIMP document");
            StringAssert.Contains(refused.Why, "has to keep the place", name + ": the words do not say what a form that asks for the document must do");
            Expect.That(PlaceRules.MustLook(gimp, name, FileRules.MediaTypeOf(name)), name + " is not looked into before it is sent");
        }

        // An OpenRaster document.
        byte[] raster = [.. Zip("mimetype"u8.ToArray(), "image/openraster"u8.ToArray()), .. block];
        foreach (string name in new[] { "Sukhna Lake.ora", "Sukhna Lake", "Sukhna Lake.zip" })
        {
            PlaceResult refused = PlaceRules.TakeOut([.. raster], name);
            Assert.AreEqual(PlaceOutcome.Refused, refused.Outcome, name + ": an OpenRaster document is sent as it is");
            StringAssert.Contains(refused.Why, "an OpenRaster document");
            Expect.That(PlaceRules.MustLook(raster, name, FileRules.MediaTypeOf(name)), name + " is not looked into before it is sent");
        }

        // A JPEG XR picture begins as a TIFF does, with another number where a TIFF has 42: it is not read as a TIFF, and not sent.
        byte[] xr = [0x49, 0x49, 0xBC, 1, 0x20, 0, 0, 0, .. new byte[24], .. block];
        foreach (string name in new[] { "Jallianwala Bagh.jxr", "Jallianwala Bagh.wdp", "Jallianwala Bagh", "Jallianwala Bagh.tif", "Jallianwala Bagh.dat" })
        {
            PlaceResult refused = PlaceRules.TakeOut([.. xr], name);
            Assert.AreEqual(PlaceOutcome.Refused, refused.Outcome, name + ": a JPEG XR picture is sent as it is");
            StringAssert.Contains(refused.Why, "a JPEG XR picture");
            Expect.That(PlaceRules.MustLook(xr, name, FileRules.MediaTypeOf(name)), name + " is not looked into before it is sent");
        }

        // The words for a Photoshop document say the same of the form.
        PlaceResult photoshop = PlaceRules.TakeOut([.. "8BPS"u8.ToArray(), 0, 1, 0, 0, 0, 0, 0, 0, 0, 3], "Wagah border.psd");
        StringAssert.Contains(photoshop.Why, "a Photoshop document");
        StringAssert.Contains(photoshop.Why, "has to keep the place");
    }

    [TestMethod]
    [DataRow("Qila Mubarak.psd")]
    [DataRow("Qila Mubarak.PSB")]
    [DataRow("Qila Mubarak.xcf")]
    [DataRow("Qila Mubarak.ora")]
    [DataRow("Qila Mubarak.kra")]
    [DataRow("Qila Mubarak.pdn")]
    [DataRow("Qila Mubarak.afphoto")]
    [DataRow("Qila Mubarak.afdesign")]
    public void A_file_named_as_a_picture_editors_document_whose_bytes_say_nothing_known_is_refused(string name)
    {
        // Bytes of no kind this code knows: nobody here can say what they hold.
        byte[] unknown = [.. "KDocument-1"u8.ToArray(), .. new byte[64]];
        Expect.That(PlaceRules.IsEditorName(name), name + " is not known as an editor's document");
        Expect.That(PlaceRules.MustLook(unknown, name, FileRules.MediaTypeOf(name)), name + " is not looked into before it is sent");
        PlaceResult refused = PlaceRules.TakeOut([.. unknown], name);
        Assert.AreEqual(PlaceOutcome.Refused, refused.Outcome, name + " is sent as it is");
        StringAssert.Contains(refused.Why, "a picture editor's document");
        StringAssert.Contains(refused.Why, "has to keep the place");
    }

    [TestMethod]
    public void A_name_only_adds_care_and_a_file_known_neither_by_its_bytes_nor_by_its_name_is_sent_as_it_is()
    {
        (byte[] block, _, _, _) = Exif(true);

        // A JPEG named as an editor's document is a JPEG: it is cleaned, not refused.
        Assert.AreEqual(PlaceOutcome.Removed, PlaceRules.TakeOut(Jpeg(block), "Anandpur Sahib.psd").Outcome);
        Assert.AreEqual(PlaceOutcome.Removed, PlaceRules.TakeOut(Jpeg(block), "Anandpur Sahib.xcf").Outcome);

        // A file named as a JPEG XR picture whose bytes are not one cannot be checked, so it is not sent.
        Assert.AreEqual(PlaceOutcome.Refused, PlaceRules.TakeOut([1, 2, 3, 4], "Anandpur Sahib.jxr").Outcome);
        Assert.AreEqual(PlaceOutcome.Refused, PlaceRules.TakeOut([1, 2, 3, 4], "Anandpur Sahib.wdp").Outcome);

        // What is known neither way is not looked into and goes as it is, whatever it holds: a name with no extension, one that is no
        // picture's, and one that only ends like an editor's.
        byte[] unknown = [.. "KDocument-1"u8.ToArray(), .. block];
        foreach (string name in new[] { "Anandpur Sahib", "Anandpur Sahib.dat", "Anandpur Sahib.bin", "notes on psd", ".psd", "Anandpur Sahib.psd.txt" })
        {
            Expect.That(!PlaceRules.MustLook(unknown, name, FileRules.MediaTypeOf(name)), name + " is looked into though nothing says it is a picture");
            PlaceResult sent = PlaceRules.TakeOut([.. unknown], name);
            Assert.AreEqual(PlaceOutcome.None, sent.Outcome, name);
            CollectionAssert.AreEqual(unknown, sent.Bytes, name + " was changed");
        }
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

    [TestMethod]
    public void Iso_bmff_and_quicktime_videos_strip_udta_mdta_mdir_and_xmp_locations_and_shift_chunk_offsets()
    {
        byte[] samples = [0x00, 0x00, 0x01, 0x65, 0x88, 0x84, 0x21, 0xA0];
        byte[] ftyp = Mp4Box("ftyp", "isom"u8.ToArray(), [0, 0, 2, 0], "isommp42"u8.ToArray());
        byte[] xyz = Mp4Box("\u00A9xyz", [0, 18, 0x15, 0xC7], "+30.9010+075.8573/"u8.ToArray());
        byte[] loci = Mp4Box("loci", [0, 0, 0, 0, 0x15, 0xC7], "Ludhiana\0"u8.ToArray(), [0], [0, 0x3C, 0, 0, 0, 0x97, 0, 0, 0, 0, 0, 0], "Punjab\0\0"u8.ToArray());
        byte[] title = Mp4Box("\u00A9nam", [0, 13, 0x15, 0xC7], "Ludhiana loom"u8.ToArray());

        // 1. Fast-start MP4 (moov before mdat) with stco, ©xyz, loci, and ©nam: location boxes go, ©nam stays, and stco shifts to match mdat.
        byte[] udta = Mp4Box("udta", xyz, loci, title);
        byte[] dummyMoov = BuildMoovWithTrack("vide", "avc1", useCo64: false, chunkOffset: 0, udta);
        uint originalSampleOffset = (uint)(ftyp.Length + dummyMoov.Length + 8);
        byte[] realMoov = BuildMoovWithTrack("vide", "avc1", useCo64: false, chunkOffset: originalSampleOffset, udta);
        byte[] mp4 = [.. ftyp, .. realMoov, .. Mp4Box("mdat", samples)];

        PlaceResult cleaned = PlaceRules.TakeOut([.. mp4], "loom.mp4");
        Assert.AreEqual(PlaceOutcome.Removed, cleaned.Outcome, cleaned.Why);
        Assert.IsNotNull(cleaned.Bytes);
        Assert.IsLessThan(mp4.Length, cleaned.Bytes.Length);
        Assert.IsFalse(Encoding.Latin1.GetString(cleaned.Bytes).Contains("+30.9010+075.8573", StringComparison.Ordinal));
        Assert.IsFalse(Encoding.Latin1.GetString(cleaned.Bytes).Contains("loci", StringComparison.Ordinal));
        StringAssert.Contains(Encoding.Latin1.GetString(cleaned.Bytes), "Ludhiana loom");

        int stcoPos = Encoding.Latin1.GetString(cleaned.Bytes).IndexOf("stco", StringComparison.Ordinal);
        Expect.That(stcoPos >= 4, "stco box missing in cleaned MP4");
        uint newOffset = System.Buffers.Binary.BinaryPrimitives.ReadUInt32BigEndian(cleaned.Bytes.AsSpan(stcoPos + 12, 4));
        CollectionAssert.AreEqual(samples, cleaned.Bytes.AsSpan((int)newOffset, samples.Length).ToArray(), "stco offset did not follow mdat after moov shrank");

        // 2. QuickTime MOV with co64 and mdta keys + ilst: location keys are removed and surviving keys renumbered.
        byte[] qtFtyp = Mp4Box("ftyp", "qt  "u8.ToArray(), [0, 0, 0, 0], "qt  \0\0\0\0"u8.ToArray());
        byte[] keyMake = MdtaKey("com.apple.quicktime.make");
        byte[] keyIso = MdtaKey("com.apple.quicktime.location.ISO6709");
        byte[] keyAcc = MdtaKey("com.apple.quicktime.location.accuracy.horizontal");
        byte[] keyModel = MdtaKey("com.apple.quicktime.model");
        byte[] keysBox = Mp4Box("keys", [0, 0, 0, 0, 0, 0, 0, 4], keyMake, keyIso, keyAcc, keyModel);
        byte[] ilstBox = Mp4Box(
            "ilst",
            MdtaIlstItem(1, "Apple"u8.ToArray()),
            MdtaIlstItem(2, "+31.6340+074.8723+234.0/"u8.ToArray()),
            MdtaIlstItem(3, "5.000000"u8.ToArray()),
            MdtaIlstItem(4, "iPhone 15"u8.ToArray()));
        byte[] qtMeta = Mp4Box("meta", Mp4Hdlr("mdta"), keysBox, ilstBox);
        byte[] dummyMovMoov = BuildMoovWithTrack("vide", "hvc1", useCo64: true, chunkOffset: 0, qtMeta);
        ulong movSampleOffset = (ulong)(qtFtyp.Length + dummyMovMoov.Length + 8);
        byte[] movMoov = BuildMoovWithTrack("vide", "hvc1", useCo64: true, chunkOffset: movSampleOffset, qtMeta);
        byte[] mov = [.. qtFtyp, .. movMoov, .. Mp4Box("mdat", samples)];

        PlaceResult cleanedMov = PlaceRules.TakeOut([.. mov], "amritsar.mov");
        Assert.AreEqual(PlaceOutcome.Removed, cleanedMov.Outcome, cleanedMov.Why);
        string movText = Encoding.Latin1.GetString(cleanedMov.Bytes!);
        Assert.IsFalse(movText.Contains("ISO6709", StringComparison.Ordinal));
        Assert.IsFalse(movText.Contains("+31.6340", StringComparison.Ordinal));
        StringAssert.Contains(movText, "com.apple.quicktime.make");
        StringAssert.Contains(movText, "com.apple.quicktime.model");
        StringAssert.Contains(movText, "iPhone 15");

        int co64Pos = movText.IndexOf("co64", StringComparison.Ordinal);
        ulong updatedCo64 = System.Buffers.Binary.BinaryPrimitives.ReadUInt64BigEndian(cleanedMov.Bytes.AsSpan(co64Pos + 12, 8));
        CollectionAssert.AreEqual(samples, cleanedMov.Bytes.AsSpan((int)updatedCo64, samples.Length).ToArray());

        // 3. Top-level XMP UUID box holding place is removed; XMP UUID with no place is left byte for byte.
        byte[] xmpUuid = [0xBE, 0x7A, 0xCF, 0xCB, 0x97, 0xA9, 0x42, 0xE8, 0x9C, 0x71, 0x99, 0x94, 0x91, 0xE3, 0xAF, 0xAC];
        byte[] placeXmp = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:Description photoshop:City=\"Pune\"/></x:xmpmeta>"u8.ToArray();
        byte[] cleanXmp = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:Description dc:title=\"Workshop\"/></x:xmpmeta>"u8.ToArray();
        byte[] mdatFirst = Mp4Box("mdat", samples);
        byte[] cleanTrackMoov = BuildMoovWithTrack("vide", "avc1", useCo64: false, chunkOffset: (uint)(ftyp.Length + 8));

        byte[] withPlaceXmp = [.. ftyp, .. mdatFirst, .. cleanTrackMoov, .. Mp4Box("uuid", xmpUuid, placeXmp)];
        PlaceResult xmpRemoved = PlaceRules.TakeOut([.. withPlaceXmp], "pune.mp4");
        Assert.AreEqual(PlaceOutcome.Removed, xmpRemoved.Outcome);
        Assert.IsFalse(Encoding.UTF8.GetString(xmpRemoved.Bytes!).Contains("Pune", StringComparison.Ordinal));

        byte[] withCleanXmp = [.. ftyp, .. mdatFirst, .. cleanTrackMoov, .. Mp4Box("uuid", xmpUuid, cleanXmp)];
        PlaceResult untouched = PlaceRules.TakeOut([.. withCleanXmp], "workshop.mp4");
        Assert.AreEqual(PlaceOutcome.None, untouched.Outcome);
        CollectionAssert.AreEqual(withCleanXmp, untouched.Bytes);

        // 4. Telemetry tracks (gpmd / camm), non-BMFF video containers, and truncated MP4 boxes are refused.
        byte[] telemetryMoov = BuildMoovWithTrack("gpmd", "gpmd", useCo64: false, chunkOffset: (uint)(ftyp.Length + 8));
        byte[] goproMp4 = [.. ftyp, .. mdatFirst, .. telemetryMoov];
        Assert.AreEqual(PlaceOutcome.Refused, PlaceRules.TakeOut(goproMp4, "gopro.mp4").Outcome);
        Assert.AreEqual(PlaceOutcome.Refused, PlaceRules.TakeOut([0x1A, 0x45, 0xDF, 0xA3, .. "webm"u8.ToArray()], "clip.webm").Outcome);
        Assert.AreEqual(PlaceOutcome.Refused, PlaceRules.TakeOut([.. ftyp, 0, 0, 0, 50, .. "moov"u8.ToArray()], "truncated.mp4").Outcome);
    }

    [TestMethod]
    public void Video_artwork_sony_xml_utf16_padding_saio_telemetry_and_camera_brands_are_cleaned_or_refused()
    {
        byte[] samples = [0x00, 0x00, 0x01, 0x65, 0x44, 0x55, 0x66, 0x77];
        (byte[] gpsExif, _, _, _) = Exif(little: true, withGps: true);
        byte[] geotaggedJpeg = Jpeg(gpsExif);

        // 1. Vendor-branded camera .MOV/.MP4 (Canon CAEP, Nikon niko, Sony XAVC) are classified as videos (not OtherMedia), while Canon CR3 stays OtherMedia.
        byte[] canonMovFtyp = Mp4Box("ftyp", "qt  "u8.ToArray(), [0, 0, 2, 0], "qt  CAEP"u8.ToArray());
        byte[] nikonMovFtyp = Mp4Box("ftyp", "qt  "u8.ToArray(), [0, 0, 2, 0], "qt  niko"u8.ToArray());
        Assert.AreEqual(PictureKind.NotAPicture, PlaceKinds.KindOf(canonMovFtyp));
        Assert.AreEqual(VideoKind.IsoBmff, PlaceKinds.VideoOf(canonMovFtyp));
        Assert.AreEqual(VideoKind.IsoBmff, PlaceKinds.VideoOf(nikonMovFtyp));

        // 2. QuickTime mdta ilst with geotagged JPEG artwork (type 13) and UTF-16BE coordinates (type 2): JPEG GPS is stripped in place, UTF-16BE item goes.
        byte[] artworkKey = MdtaKey("com.apple.quicktime.artwork");
        byte[] customTextKey = MdtaKey("com.vendor.note");
        byte[] titleKey = MdtaKey("com.apple.quicktime.title");
        byte[] keysBox = Mp4Box("keys", [0, 0, 0, 0, 0, 0, 0, 3], artworkKey, customTextKey, titleKey);
        byte[] artworkItem = MdtaTypedIlstItem(1, 13, geotaggedJpeg);
        byte[] utf16Coords = Encoding.BigEndianUnicode.GetBytes("+30.9010+075.8573/");
        byte[] utf16Item = MdtaTypedIlstItem(2, 2, utf16Coords);
        byte[] titleItem = MdtaTypedIlstItem(3, 1, "Amritsar workshop"u8.ToArray());
        byte[] mdtaMeta = Mp4Box("meta", Mp4Hdlr("mdta"), keysBox, Mp4Box("ilst", artworkItem, utf16Item, titleItem));
        byte[] mdatFirst = Mp4Box("mdat", samples);
        byte[] mdtaMoov = BuildMoovWithTrack("vide", "avc1", useCo64: false, chunkOffset: (uint)(canonMovFtyp.Length + 8), mdtaMeta);
        byte[] canonMov = [.. canonMovFtyp, .. mdatFirst, .. mdtaMoov];

        PlaceResult cleanedMdta = PlaceRules.TakeOut([.. canonMov], "MVI_0104.MOV");
        Assert.AreEqual(PlaceOutcome.Removed, cleanedMdta.Outcome, cleanedMdta.Why);
        Assert.IsNotNull(cleanedMdta.Bytes);
        Assert.AreEqual(-1, Find(cleanedMdta.Bytes, utf16Coords), "UTF-16BE coordinates survived in mdta");
        Assert.AreEqual(-1, Find(cleanedMdta.Bytes, [30, 0, 0, 0, 1, 0, 0, 0, 54, 0, 0, 0]), "EXIF GPS latitude survived in mdta JPEG artwork");
        Assert.AreEqual(-1, Find(cleanedMdta.Bytes, "N\0\0\0"u8.ToArray()), "EXIF GPS hemisphere survived in mdta JPEG artwork");
        StringAssert.Contains(Encoding.Latin1.GetString(cleanedMdta.Bytes), "Charkha", "cleaned JPEG artwork maker tag should remain");
        StringAssert.Contains(Encoding.Latin1.GetString(cleanedMdta.Bytes), "Amritsar workshop");
        Assert.AreEqual(PlaceOutcome.None, PlaceRules.TakeOut([.. cleanedMdta.Bytes], "MVI_0104.MOV").Outcome);

        // 3. Sony NonRealTimeMeta XML (<Group name="ExifGPS"><Item name="Latitude" .../>) and 3GPP UTF-16BE dscp are stripped.
        string sonyXml = "<NonRealTimeMeta xmlns=\"urn:schemas-professionalDisc:nonRealTimeMeta:ver.2.00\"><Group name=\"ExifGPS\"><Item name=\"Latitude\" value=\"30;54;3.600\"/><Item name=\"Longitude\" value=\"75;51;26.580\"/></Group></NonRealTimeMeta>";
        Assert.IsTrue(PlaceRules.HoldsPlace(Encoding.UTF8.GetBytes(sonyXml)), "Sony NonRealTimeMeta ExifGPS XML must be recognised as holding a place");
        byte[] xmlMeta = Mp4Box("meta", [0, 0, 0, 0], Mp4Hdlr("meta"), Mp4Box("xml ", [0, 0, 0, 0], Encoding.UTF8.GetBytes(sonyXml)));
        byte[] utf16Dscp = Mp4Box("dscp", [0, 0, 0, 0, 0x15, 0xC7, 0xFE, 0xFF], utf16Coords, [0, 0]);
        byte[] sonyMoov = BuildMoovWithTrack("vide", "avc1", useCo64: false, chunkOffset: (uint)(canonMovFtyp.Length + 8), xmlMeta, Mp4Box("udta", utf16Dscp));
        PlaceResult cleanedSony = PlaceRules.TakeOut([.. canonMovFtyp, .. mdatFirst, .. sonyMoov], "C0001.MP4");
        Assert.AreEqual(PlaceOutcome.Removed, cleanedSony.Outcome, cleanedSony.Why);
        Assert.IsNotNull(cleanedSony.Bytes);
        Assert.IsFalse(Encoding.Latin1.GetString(cleanedSony.Bytes).Contains("ExifGPS", StringComparison.Ordinal));

        // 4. Non-zero nested free/skip boxes (tombstoned ©xyz / EXIF) are zeroed while zero-filled free boxes stay byte-for-byte; saio shifts with mdat.
        byte[] tombstonedFree = Mp4Box("free", "+30.9010+075.8573/"u8.ToArray(), gpsExif);
        byte[] xyz = Mp4Box("\u00A9xyz", [0, 18, 0x15, 0xC7], "+30.9010+075.8573/"u8.ToArray());
        byte[] dummySaioMoov = BuildMoovWithSaio(chunkOffset: 0, saioOffset: 0, Mp4Box("udta", xyz), tombstonedFree);
        uint oldSampleOffset = (uint)(canonMovFtyp.Length + dummySaioMoov.Length + 8);
        byte[] realSaioMoov = BuildMoovWithSaio(chunkOffset: oldSampleOffset, saioOffset: oldSampleOffset + 2, Mp4Box("udta", xyz), tombstonedFree);
        byte[] saioMp4 = [.. canonMovFtyp, .. realSaioMoov, .. mdatFirst];

        PlaceResult cleanedSaio = PlaceRules.TakeOut([.. saioMp4], "aux.mp4");
        Assert.AreEqual(PlaceOutcome.Removed, cleanedSaio.Outcome, cleanedSaio.Why);
        Assert.IsNotNull(cleanedSaio.Bytes);
        Assert.IsFalse(Encoding.Latin1.GetString(cleanedSaio.Bytes).Contains("+30.9010+075.8573", StringComparison.Ordinal));
        int saioPos = Find(cleanedSaio.Bytes, "saio"u8.ToArray());
        Assert.IsTrue(saioPos >= 4);
        // Version 0 + flags 1 (4B) + aux_info_type/param (8B) + count=1 (4B) => entry at saioPos + 4 + 16 = saioPos + 20.
        uint shiftedSaio = BinaryPrimitives.ReadUInt32BigEndian(cleanedSaio.Bytes.AsSpan(saioPos + 20, 4));
        int newMdatPos = Find(cleanedSaio.Bytes, "mdat"u8.ToArray()) - 4;
        Assert.AreEqual((uint)(newMdatPos + 8 + 2), shiftedSaio, "saio offset must shift alongside mdat");

        // 5. DJI djmd, fdsc, klv , GPS-named handler, NMEA $GPRMC subtitle mdat, and 192-byte BDAV .m2ts are all refused.
        foreach (string telemetry in new[] { "djmd", "dbgi", "fdsc", "klv " })
        {
            byte[] tMoov = BuildMoovWithTrack(telemetry, telemetry, useCo64: false, chunkOffset: (uint)(canonMovFtyp.Length + 8));
            Assert.AreEqual(PlaceOutcome.Refused, PlaceRules.TakeOut([.. canonMovFtyp, .. mdatFirst, .. tMoov], "drone.mp4").Outcome, telemetry);
        }

        byte[] gpsNamedHdlr = Mp4Box("hdlr", new byte[8], "vide"u8.ToArray(), new byte[12], "GPS Metadata\0"u8.ToArray());
        byte[] gpsHdlrMdia = Mp4Box("mdia", Mp4Box("mdhd", new byte[24]), gpsNamedHdlr, Mp4Box("minf", Mp4Box("vmhd", new byte[12]), Mp4Box("stbl", Mp4Box("stsd", [0, 0, 0, 0, 0, 0, 0, 1], Mp4Box("avc1", new byte[8])), Mp4Box("stco", [0, 0, 0, 0, 0, 0, 0, 1], [0, 0, 0, 32]))));
        byte[] gpsHdlrMoov = Mp4Box("moov", Mp4Box("mvhd", new byte[100]), Mp4Box("trak", Mp4Box("tkhd", new byte[84]), gpsHdlrMdia));
        Assert.AreEqual(PlaceOutcome.Refused, PlaceRules.TakeOut([.. canonMovFtyp, .. mdatFirst, .. gpsHdlrMoov], "cam.mp4").Outcome);

        byte[] nmeaMdat = Mp4Box("mdat", "$GPRMC,123519,A,3054.060,N,07551.438,E,022.4,084.4,230394,003.1,W*6A"u8.ToArray());
        byte[] sbtlMoov = BuildMoovWithTrack("sbtl", "tx3g", useCo64: false, chunkOffset: (uint)(canonMovFtyp.Length + 8));
        Assert.AreEqual(PlaceOutcome.Refused, PlaceRules.TakeOut([.. canonMovFtyp, .. nmeaMdat, .. sbtlMoov], "dashcam.mp4").Outcome);

        byte[] m2ts = new byte[576];
        m2ts[4] = 0x47;
        m2ts[196] = 0x47;
        m2ts[388] = 0x47;
        Assert.AreEqual(VideoKind.Mpeg, PlaceKinds.VideoOf(m2ts));
        Assert.AreEqual(PlaceOutcome.Refused, PlaceRules.TakeOut(m2ts, "00000").Outcome);
    }

    private static byte[] Mp4Box(string kind, params byte[][] payloads)
    {
        int length = 8 + payloads.Sum(p => p.Length);
        byte[] box = new byte[length];
        System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(box.AsSpan(0, 4), (uint)length);
        Encoding.Latin1.GetBytes(kind).CopyTo(box.AsSpan(4, 4));
        int at = 8;
        foreach (byte[] p in payloads)
        {
            p.CopyTo(box.AsSpan(at));
            at += p.Length;
        }

        return box;
    }

    private static byte[] Mp4Hdlr(string subtype) =>
        Mp4Box("hdlr", new byte[8], Encoding.Latin1.GetBytes(subtype), new byte[12]);

    private static byte[] MdtaKey(string name)
    {
        byte[] utf8 = Encoding.UTF8.GetBytes(name);
        byte[] raw = new byte[8 + utf8.Length];
        System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(raw.AsSpan(0, 4), (uint)raw.Length);
        "mdta"u8.CopyTo(raw.AsSpan(4, 4));
        utf8.CopyTo(raw.AsSpan(8));
        return raw;
    }

    private static byte[] MdtaIlstItem(uint index, byte[] utf8Value) =>
        MdtaTypedIlstItem(index, 1, utf8Value);

    private static byte[] MdtaTypedIlstItem(uint index, uint dataType, byte[] rawValue)
    {
        byte[] typeHeader = new byte[8];
        System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(typeHeader.AsSpan(0, 4), dataType);
        byte[] dataBox = Mp4Box("data", typeHeader, rawValue);
        byte[] item = new byte[8 + dataBox.Length];
        System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(item.AsSpan(0, 4), (uint)item.Length);
        System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(item.AsSpan(4, 4), index);
        dataBox.CopyTo(item.AsSpan(8));
        return item;
    }

    private static byte[] BuildMoovWithSaio(uint chunkOffset, uint saioOffset, params byte[][] extraMoovBoxes)
    {
        byte[] stcoOff = new byte[4];
        System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(stcoOff, chunkOffset);
        byte[] stco = Mp4Box("stco", [0, 0, 0, 0, 0, 0, 0, 1], stcoOff);

        byte[] saioOff = new byte[4];
        System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(saioOff, saioOffset);
        // FullBox version 0, flags 1 (aux_info_type present): 4B version/flags + 8B aux_info_type/param + 4B count=1 + 4B offset.
        byte[] saio = Mp4Box("saio", [0, 0, 0, 1], "cenc\0\0\0\0"u8.ToArray(), [0, 0, 0, 1], saioOff);

        byte[] stsd = Mp4Box("stsd", [0, 0, 0, 0, 0, 0, 0, 1], Mp4Box("avc1", new byte[8]));
        byte[] stbl = Mp4Box("stbl", stsd, stco, saio);
        byte[] minf = Mp4Box("minf", Mp4Box("vmhd", new byte[12]), stbl);
        byte[] mdia = Mp4Box("mdia", Mp4Box("mdhd", new byte[24]), Mp4Hdlr("vide"), minf);
        byte[] trak = Mp4Box("trak", Mp4Box("tkhd", new byte[84]), mdia);
        return Mp4Box("moov", [Mp4Box("mvhd", new byte[100]), trak, .. extraMoovBoxes]);
    }

    private static byte[] BuildMoovWithTrack(string handler, string sampleEntry, bool useCo64, ulong chunkOffset, params byte[][] extraMoovBoxes)
    {
        byte[] chunkBox;
        if (useCo64)
        {
            byte[] off = new byte[8];
            System.Buffers.Binary.BinaryPrimitives.WriteUInt64BigEndian(off, chunkOffset);
            chunkBox = Mp4Box("co64", [0, 0, 0, 0, 0, 0, 0, 1], off);
        }
        else
        {
            byte[] off = new byte[4];
            System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(off, (uint)chunkOffset);
            chunkBox = Mp4Box("stco", [0, 0, 0, 0, 0, 0, 0, 1], off);
        }

        byte[] stsd = Mp4Box("stsd", [0, 0, 0, 0, 0, 0, 0, 1], Mp4Box(sampleEntry, new byte[8]));
        byte[] stbl = Mp4Box("stbl", stsd, chunkBox);
        byte[] minf = Mp4Box("minf", Mp4Box("vmhd", new byte[12]), stbl);
        byte[] mdia = Mp4Box("mdia", Mp4Box("mdhd", new byte[24]), Mp4Hdlr(handler), minf);
        byte[] trak = Mp4Box("trak", Mp4Box("tkhd", new byte[84]), mdia);
        return Mp4Box("moov", [Mp4Box("mvhd", new byte[100]), trak, .. extraMoovBoxes]);
    }
}
