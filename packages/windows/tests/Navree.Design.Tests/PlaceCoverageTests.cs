using System.Buffers.Binary;
using System.Text;
using Navree.Design.Inputs;
using static Navree.Design.Tests.PictureSamples;

namespace Navree.Design.Tests;

/// <summary>Every place a picture can hold a place is looked at: what is cleaned is cleaned, what cannot be is refused, and the picture's own bytes stay.</summary>
[TestClass]
public sealed class PlaceCoverageTests
{
    private static int Find(byte[] whole, ReadOnlySpan<byte> part) => whole.AsSpan().IndexOf(part);

    private static bool Holds(byte[] file, string words) => Encoding.Latin1.GetString(file).Contains(words, StringComparison.Ordinal);

    private static PlaceResult Clean(byte[] file, string name = "Chandigarh.jpg")
    {
        PlaceResult result = PlaceRules.TakeOut(file, name);
        Assert.IsTrue(result.Outcome == PlaceOutcome.Refused || result.Bytes is not null, "a result that is not a refusal says what to send");
        return result;
    }

    // ---- JPEG: everything up to the end of the picture, not only what comes before the first scan --------------------

    [TestMethod]
    public void A_segment_between_the_scans_of_a_picture_is_looked_at_and_the_scans_are_not_changed()
    {
        byte[] file = JpegOfTwoScans((0xE1, XmpSegment(Packet("photoshop:City=\"Chandigarh\""))));
        byte[] scans = Scans(file);

        PlaceResult result = Clean(file);

        Assert.AreEqual(PlaceOutcome.Removed, result.Outcome);
        Assert.IsFalse(Holds(result.Bytes!, "Chandigarh"));
        CollectionAssert.AreEqual(scans, Scans(result.Bytes!), "the compressed picture data is exactly as it was");
        CollectionAssert.AreEqual(ScanData.Concat(SecondScanData).ToArray(), Scans(result.Bytes!), "both scans are there");
    }

    [TestMethod]
    public void An_application_segment_this_code_does_not_read_is_emptied_and_the_ones_that_change_how_the_picture_looks_are_kept()
    {
        byte[] gopro = [.. "GoPro\0"u8.ToArray(), .. "GPS5"u8.ToArray(), .. LatitudeRationals];
        byte[] icc = [.. "ICC_PROFILE\0"u8.ToArray(), 1, 1, .. new byte[40]];
        byte[] adobe = [.. "Adobe"u8.ToArray(), 0, 100, 0, 0, 0, 0, 1];
        byte[] file = Jpeg((0xE6, gopro), (0xE2, icc), (0xEE, adobe), (0xEB, "JP\0\u0001jumb"u8.ToArray()));
        byte[] scans = Scans(file);

        PlaceResult result = Clean(file);

        Assert.AreEqual(PlaceOutcome.Removed, result.Outcome);
        Assert.IsTrue(Find(result.Bytes!, LatitudeRationals) < 0, "the segment that held a place is empty");
        Assert.IsTrue(Find(result.Bytes!, icc) >= 0, "the colour profile stays");
        Assert.IsTrue(Find(result.Bytes!, adobe) >= 0, "Adobe's colour transform stays");
        Assert.IsFalse(Holds(result.Bytes!, "jumb"));
        CollectionAssert.AreEqual(scans, Scans(result.Bytes!));
        Assert.AreEqual(file.Length, result.Bytes!.Length);
    }

    [TestMethod]
    public void A_makers_note_is_emptied_because_it_cannot_be_read_here()
    {
        byte[] note = [.. "Apple iOS\0"u8.ToArray(), 0, 1, .. "MM"u8.ToArray(), .. Encoding.ASCII.GetBytes(PlaceWords)];
        byte[] file = Jpeg((0xE1, ExifSegment(ExifBlock(withGps: false, makerNote: note))));

        PlaceResult result = Clean(file);

        Assert.AreEqual(PlaceOutcome.Removed, result.Outcome);
        Assert.IsFalse(Holds(result.Bytes!, PlaceWords));
        Assert.IsTrue(Holds(result.Bytes!, "Charkha"), "the maker's name in IFD0 stays");
        Assert.IsTrue(Holds(result.Bytes!, "2026:11:16 10:15:00"), "the time it was taken stays");
    }

    [TestMethod]
    public void The_thumbnail_in_the_exif_block_is_cleaned_as_a_picture_of_its_own()
    {
        byte[] thumbnail = Jpeg((0xE1, ExifSegment(ExifBlock())));
        byte[] file = Jpeg((0xE1, ExifSegment(ExifBlock(withGps: false, thumbnail: thumbnail))));

        PlaceResult result = Clean(file);

        Assert.AreEqual(PlaceOutcome.Removed, result.Outcome);
        Assert.IsTrue(Find(result.Bytes!, LatitudeRationals) < 0);
    }

    [TestMethod]
    public void The_place_in_photoshops_iptc_and_xmp_resources_is_taken_out()
    {
        byte[] resources = Resources((0x0404, Iptc("Lohri", "Chandigarh")), (0x0424, Encoding.UTF8.GetBytes(Packet("Iptc4xmpCore:Location=\"Sector 17 Plaza\""))));
        byte[] file = Jpeg((0xED, [.. "Photoshop 3.0\0"u8.ToArray(), .. resources]));

        PlaceResult result = Clean(file);

        Assert.AreEqual(PlaceOutcome.Removed, result.Outcome);
        Assert.IsFalse(Holds(result.Bytes!, "Chandigarh"));
        Assert.IsFalse(Holds(result.Bytes!, PlaceWords));
        Assert.IsTrue(Holds(result.Bytes!, "Lohri"), "the picture's title stays");
    }

    [TestMethod]
    public void An_extended_xmp_packet_is_read_whole_even_when_a_name_is_split_between_its_pieces_and_the_pieces_come_out_of_order()
    {
        // Split inside "photoshop:City", where neither half is a name about a place.
        byte[] whole = Encoding.UTF8.GetBytes(Packet("xmp:Label=\"" + new string('x', 300) + "\" photoshop:City=\"Chandigarh\""));
        int split = Encoding.UTF8.GetString(whole).IndexOf(":City", StringComparison.Ordinal) + 3;
        byte[] file = Jpeg((0xE1, ExtendedXmpSegment(whole, split, whole.Length - split)), (0xE1, ExtendedXmpSegment(whole, 0, split)));

        PlaceResult result = Clean(file);

        Assert.AreEqual(PlaceOutcome.Removed, result.Outcome);
        Assert.IsFalse(Holds(result.Bytes!, "Chandigarh"));
    }

    // ---- JPEG: what follows the first picture ----------------------------------------------------------------------

    [TestMethod]
    public void Every_picture_after_the_first_is_cleaned_as_a_picture_of_its_own()
    {
        byte[] mpf = [.. "MPF\0"u8.ToArray(), .. "MM\0*"u8.ToArray(), 0, 0, 0, 8, 0, 0];
        byte[] primary = Jpeg((0xE1, ExifSegment(ExifBlock())), (0xE2, mpf));
        byte[] gainMap = Jpeg((0xE1, ExifSegment(ExifBlock())), (0xE1, XmpSegment(Packet("hdrgm:Version=\"1.0\" drone-dji:GpsLatitude=\"+30.7333\""))));
        byte[] file = [.. primary, .. gainMap];
        byte[] scans = [.. Scans(primary), .. Scans(gainMap)];

        PlaceResult result = Clean(file);

        Assert.AreEqual(PlaceOutcome.Removed, result.Outcome);
        Assert.IsTrue(Find(result.Bytes!, LatitudeRationals) < 0, "neither picture keeps its latitude");
        Assert.IsFalse(Holds(result.Bytes!, "30.7333"));
        CollectionAssert.AreEqual(scans, Scans(result.Bytes![..primary.Length]).Concat(Scans(result.Bytes![primary.Length..])).ToArray());
    }

    [TestMethod]
    public void A_motion_photo_is_refused_with_words_because_its_video_can_hold_the_place()
    {
        string directory = "<Container:Directory><rdf:Seq><rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Semantic=\"Primary\" Item:Mime=\"image/jpeg\"/></rdf:li>"
            + "<rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Semantic=\"MotionPhoto\" Item:Mime=\"video/mp4\" Item:Length=\"36\"/></rdf:li></rdf:Seq></Container:Directory>";
        byte[] primary = Jpeg((0xE1, XmpSegment(Packet("Camera:MotionPhoto=\"1\"", directory))));
        byte[] video = [0, 0, 0, 20, .. "ftyp"u8.ToArray(), .. "isom"u8.ToArray(), 0, 0, 2, 0, .. "mp41"u8.ToArray(), 0, 0, 0, 16, .. "udta"u8.ToArray(), .. "©xyz+30.7333"u8.ToArray()];

        PlaceResult result = Clean([.. primary, .. video]);

        Assert.AreEqual(PlaceOutcome.Refused, result.Outcome);
        StringAssert.Contains(result.Why, "video");
    }

    [TestMethod]
    public void Anything_else_after_the_picture_is_refused_and_filling_is_not()
    {
        byte[] picture = Jpeg((0xE1, ExifSegment(ExifBlock())));

        Assert.AreEqual(PlaceOutcome.Refused, Clean([.. picture, .. "SEFH"u8.ToArray(), 1, 2, 3, 4]).Outcome);
        Assert.AreEqual(PlaceOutcome.Removed, Clean([.. picture, 0, 0, 0, 0, 0xFF, 0xFF]).Outcome);
    }

    [TestMethod]
    public void A_jpeg_cut_short_is_refused()
    {
        byte[] picture = Jpeg((0xE1, ExifSegment(ExifBlock())));

        Assert.AreEqual(PlaceOutcome.Refused, Clean(picture[..^2]).Outcome, "no end of the picture");
        Assert.AreEqual(PlaceOutcome.Refused, Clean(picture[..^6]).Outcome, "cut in the scan");
    }

    // ---- TIFF -------------------------------------------------------------------------------------------------------

    private static byte[] TiffPicture(byte[] pixels, params (ushort Tag, ushort Type, uint Count, object Value)[] more)
    {
        Tiff tiff = new(false);
        List<(ushort, ushort, uint, object)> ifd0 =
        [
            (0x0100, 3, 1, (uint)(2 << 16)), (0x0101, 3, 1, (uint)(2 << 16)), (0x0106, 3, 1, (uint)(2 << 16)),
            (0x0111, 4, 1, (uint)8), (0x0117, 4, 1, (uint)pixels.Length), .. more,
        ];
        return tiff.Write(ifd0, before: pixels);
    }

    [TestMethod]
    public void A_tiffs_xmp_packet_is_looked_at_and_its_pixels_are_not_changed()
    {
        byte[] pixels = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        byte[] packet = Encoding.UTF8.GetBytes(Packet("exif:GPSLatitude=\"30,44.0N\" exif:GPSLongitude=\"76,46.8E\""));
        byte[] file = TiffPicture(pixels, (0x02BC, 7, (uint)packet.Length, packet));

        PlaceResult result = Clean(file, "scan.tif");

        Assert.AreEqual(PlaceOutcome.Removed, result.Outcome);
        Assert.IsFalse(Holds(result.Bytes!, "30,44.0N"));
        CollectionAssert.AreEqual(pixels, result.Bytes![8..20], "the pixels are as they were");
    }

    [TestMethod]
    public void A_tiffs_iptc_place_is_taken_out()
    {
        byte[] iptc = Iptc("Lohri", "Chandigarh");
        byte[] file = TiffPicture([1, 2, 3, 4], (0x83BB, 7, (uint)iptc.Length, iptc));

        PlaceResult result = Clean(file, "scan.tif");

        Assert.AreEqual(PlaceOutcome.Removed, result.Outcome);
        Assert.IsFalse(Holds(result.Bytes!, "Chandigarh"));
        Assert.IsTrue(Holds(result.Bytes!, "Lohri"));
    }

    [TestMethod]
    public void A_place_whose_values_lie_on_the_pixels_is_refused_and_the_pixels_are_never_emptied()
    {
        // A GPS value that says it lies where the picture's own data lies.
        byte[] pixels = [.. LatitudeRationals];
        Tiff tiff = new(false);
        List<(ushort, ushort, uint, object)> gps = [(0x0002, 5, 3, (uint)8)];
        byte[] file = tiff.Write(
        [
            (0x0100, 3, 1, (uint)(2 << 16)), (0x0101, 3, 1, (uint)(2 << 16)), (0x0111, 4, 1, (uint)8), (0x0117, 4, 1, (uint)pixels.Length), (0x8825, 4, 1, gps),
        ], before: pixels);

        Assert.AreEqual(PlaceOutcome.Refused, Clean(file, "scan.tif").Outcome);
    }

    [TestMethod]
    public void A_cameras_raw_picture_is_refused_by_its_name_or_by_its_bytes()
    {
        byte[] plain = TiffPicture([1, 2, 3, 4]);
        byte[] dng = TiffPicture([1, 2, 3, 4], (0xC612, 1, 4, new byte[] { 1, 4, 0, 0 }));

        Assert.AreEqual(PlaceOutcome.Refused, Clean(plain, "DSC_0042.NEF").Outcome);
        Assert.AreEqual(PlaceOutcome.Refused, Clean(dng, "scan.tif").Outcome);
        Assert.AreNotEqual(PlaceOutcome.Refused, Clean(TiffPicture([1, 2, 3, 4]), "scan.tif").Outcome);
    }

    // ---- PNG --------------------------------------------------------------------------------------------------------

    private static byte[] Text(string keyword, string text) => [.. Encoding.Latin1.GetBytes(keyword), 0, .. Encoding.UTF8.GetBytes(text)];

    [TestMethod]
    public void Xmp_in_a_pngs_tEXt_or_compressed_zTXt_or_iTXt_chunk_is_read_and_a_chunk_holding_a_place_goes()
    {
        byte[] packet = Encoding.UTF8.GetBytes(Packet("exif:GPSLatitude=\"30,44.0N\""));
        byte[] keyword = "XML:com.adobe.xmp"u8.ToArray();
        byte[][] chunks =
        [
            Chunk("tEXt", [.. keyword, 0, .. packet]),
            Chunk("zTXt", [.. keyword, 0, 0, .. Deflated(packet)]),
            Chunk("iTXt", [.. keyword, 0, 1, 0, 0, 0, .. Deflated(packet)]),
        ];

        foreach (byte[] chunk in chunks)
        {
            byte[] file = Png(Chunk("tEXt", Text("Title", "Lohri")), chunk);
            PlaceResult result = Clean(file, "Chandigarh.png");
            Assert.AreEqual(PlaceOutcome.Removed, result.Outcome, Encoding.ASCII.GetString(chunk, 4, 4));
            CollectionAssert.AreEqual(Png(Chunk("tEXt", Text("Title", "Lohri"))), result.Bytes, "the chunk goes, and everything else is as it was");
        }
    }

    [TestMethod]
    public void A_compressed_chunk_that_would_expand_past_the_limit_is_refused()
    {
        byte[] huge = new byte[20 * 1024 * 1024];
        byte[] file = Png(Chunk("zTXt", [.. "XML:com.adobe.xmp"u8.ToArray(), 0, 0, .. Deflated(huge)]));
        long before = GC.GetAllocatedBytesForCurrentThread();

        PlaceResult result = Clean(file, "Chandigarh.png");

        Assert.AreEqual(PlaceOutcome.Refused, result.Outcome);
        Assert.IsTrue(GC.GetAllocatedBytesForCurrentThread() - before < 32L * 1024 * 1024, "what is read stops at the limit");
    }

    [TestMethod]
    public void A_profile_written_as_text_and_a_chunk_this_code_does_not_know_go_and_one_it_cannot_skip_is_refused()
    {
        byte[] profile = Chunk("tEXt", Text("Raw profile type exif", "\nexif\n  60\n45786966000049492a00"));
        byte[] unknown = Chunk("caBX", [.. "jumb"u8.ToArray(), .. LatitudeRationals]);

        PlaceResult result = Clean(Png(profile, unknown), "Chandigarh.png");

        Assert.AreEqual(PlaceOutcome.Removed, result.Outcome);
        CollectionAssert.AreEqual(Png(), result.Bytes);
        Assert.AreEqual(PlaceOutcome.Refused, Clean(Png(Chunk("CRIT", [1, 2])), "Chandigarh.png").Outcome);
    }

    [TestMethod]
    public void A_png_cut_short_or_with_something_after_its_end_is_refused()
    {
        byte[] file = Png();

        Assert.AreEqual(PlaceOutcome.Refused, Clean(file[..^12], "a.png").Outcome, "no IEND");
        Assert.AreEqual(PlaceOutcome.Refused, Clean([.. file, 1, 2, 3], "a.png").Outcome);
        Assert.AreEqual(PlaceOutcome.None, Clean([.. file, 0, 0], "a.png").Outcome);
    }

    // ---- WebP -------------------------------------------------------------------------------------------------------

    [TestMethod]
    public void A_webp_chunk_this_code_does_not_know_is_emptied_and_something_after_the_file_is_refused()
    {
        byte[] file = Webp(("C2PA", [.. "jumb"u8.ToArray(), .. LatitudeRationals]));

        PlaceResult result = Clean(file, "a.webp");

        Assert.AreEqual(PlaceOutcome.Removed, result.Outcome);
        Assert.IsTrue(Find(result.Bytes!, LatitudeRationals) < 0);
        Assert.IsTrue(Find(result.Bytes!, WebpPixels) >= 0);
        Assert.AreEqual(PlaceOutcome.Refused, Clean([.. Webp(), 7, 7], "a.webp").Outcome);
    }

    [TestMethod]
    public void The_place_in_a_webp_exif_chunk_is_taken_out()
    {
        byte[] file = Webp(("EXIF", ExifBlock()));
        PlaceResult result = Clean(file, "a.webp");

        Assert.AreEqual(PlaceOutcome.Removed, result.Outcome);
        Assert.IsTrue(Find(result.Bytes!, LatitudeRationals) < 0);
        int size = BinaryPrimitives.ReadInt32LittleEndian(result.Bytes.AsSpan(4));
        Assert.AreEqual(result.Bytes!.Length - 8, size);
    }
}
