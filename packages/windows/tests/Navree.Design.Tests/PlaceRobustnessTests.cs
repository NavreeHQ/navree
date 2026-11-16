using System.Diagnostics;
using System.Text;
using Navree.Design.Inputs;
using static Navree.Design.Tests.PictureSamples;

namespace Navree.Design.Tests;

/// <summary>
/// The cleaner reads files it did not make. Over thousands of copies of a sample of each kind, cut short at random or with bits flipped and bytes
/// overwritten, it always answers (cleaned, refused or left as it was), within a time and an allocation limit, and never fails in a way it did
/// not mean to; what it sends is no longer than what it was given, and cleaning what it sent finds nothing more to take out.
/// </summary>
[TestClass]
public sealed class PlaceRobustnessTests
{
    private const int CopiesOfEach = 800;
    private static readonly TimeSpan MostTime = TimeSpan.FromSeconds(2);
    private const long MostAllocated = 64L * 1024 * 1024;

    // The words given when reading failed in a way nothing in the cleaner expects; they must never be seen.
    private const string Failed = "because reading it failed here";

    private static byte[] RichJpeg()
    {
        byte[] note = [.. "Nikon\0"u8.ToArray(), 2, 0, .. LatitudeRationals];
        byte[] thumbnail = Jpeg((0xE1, ExifSegment(ExifBlock())));
        byte[] whole = Encoding.UTF8.GetBytes(Packet("xmp:Label=\"Lohri\" photoshop:City=\"Chandigarh\""));
        byte[] resources = Resources((0x0404, Iptc("Lohri", "Chandigarh")), (0x0424, Encoding.UTF8.GetBytes(Packet("Iptc4xmpCore:Location=\"Sector 17 Plaza\""))));
        byte[] primary = Jpeg(
            (0xE1, ExifSegment(ExifBlock(makerNote: note, thumbnail: thumbnail))),
            (0xE1, XmpSegment(Packet("exif:GPSLatitude=\"30,44.0N\"", "<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">Lohri</rdf:li></rdf:Alt></dc:title>"))),
            (0xE1, ExtendedXmpSegment(whole, 0, 60)),
            (0xE1, ExtendedXmpSegment(whole, 60, whole.Length - 60)),
            (0xED, [.. "Photoshop 3.0\0"u8.ToArray(), .. resources]),
            (0xE6, [.. "GoPro\0"u8.ToArray(), .. LatitudeRationals]),
            (0xE2, [.. "MPF\0"u8.ToArray(), .. "MM\0*"u8.ToArray(), 0, 0, 0, 8, 0, 0]));
        byte[] second = JpegOfTwoScans((0xE1, XmpSegment(Packet("drone-dji:GpsLatitude=\"+30.7333\""))));
        return [.. primary, .. second];
    }

    private static byte[] RichPng()
    {
        byte[] packet = Encoding.UTF8.GetBytes(Packet("exif:GPSLatitude=\"30,44.0N\""));
        byte[] keyword = "XML:com.adobe.xmp"u8.ToArray();
        return Png(
            Chunk("eXIf", ExifBlock()),
            Chunk("tEXt", [.. "Title"u8.ToArray(), 0, .. "Lohri"u8.ToArray()]),
            Chunk("iTXt", [.. keyword, 0, 1, 0, 0, 0, .. Deflated(packet)]),
            Chunk("zTXt", [.. keyword, 0, 0, .. Deflated(packet)]),
            Chunk("caBX", [.. "jumb"u8.ToArray(), .. LatitudeRationals]));
    }

    private static byte[] RichWebp()
    {
        byte[] frame = [.. new byte[16], .. "VP8 "u8.ToArray(), 4, 0, 0, 0, 1, 2, 3, 4, .. "XYZW"u8.ToArray(), 2, 0, 0, 0, 9, 9];
        return Webp(
            ("ANMF", frame),
            ("EXIF", ExifSegment(ExifBlock(makerNote: [1, 2, 3, 4, 5, 6]))),
            ("XMP ", Encoding.UTF8.GetBytes(Packet("photoshop:Country=\"India\""))),
            ("C2PA", [.. "jumb"u8.ToArray(), .. LatitudeRationals]));
    }

    private static byte[] RichTiff()
    {
        byte[] pixels = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        byte[] packet = Encoding.UTF8.GetBytes(Packet("exif:GPSLatitude=\"30,44.0N\""));
        byte[] iptc = Iptc("Lohri", "Chandigarh");
        Tiff tiff = new(false);
        List<(ushort, ushort, uint, object)> gps = [(0x0001, 2, 2, "N\0"u8.ToArray()), (0x0002, 5, 3, LatitudeRationals)];
        List<(ushort, ushort, uint, object)> exif = [(0x927C, 7, 8, "Pentax\0\0"u8.ToArray())];
        return tiff.Write(
        [
            (0x0100, 3, 1, (uint)(3 << 16)), (0x0101, 3, 1, (uint)(2 << 16)), (0x0106, 3, 1, (uint)(2 << 16)), (0x0111, 4, 1, (uint)8),
            (0x0117, 4, 1, (uint)pixels.Length), (0x02BC, 7, (uint)packet.Length, packet), (0x83BB, 7, (uint)iptc.Length, iptc), (0x8769, 4, 1, exif),
            (0x8825, 4, 1, gps), (0x87AF, 3, 4, new byte[] { 0, 1, 0, 1, 0, 0, 0, 0 }),
        ], before: pixels);
    }

    private static byte[] RichGif() =>
        [.. "GIF89a"u8.ToArray(), 1, 0, 1, 0, 0, 0, 0, 0x21, 0xFF, 11, .. "XMP DataXMP"u8.ToArray(), .. Encoding.UTF8.GetBytes(Packet("exif:GPSLatitude=\"30,44.0N\"")), 1, 0xFF, 0xFE, 0, 0, 0x3B];

    private static IEnumerable<(string Name, byte[] Bytes)> Samples() =>
    [
        ("Chandigarh.jpg", RichJpeg()), ("Chandigarh.png", RichPng()), ("Chandigarh.webp", RichWebp()), ("Chandigarh.tif", RichTiff()), ("Chandigarh.gif", RichGif()),
    ];

    // Copies of a sample: cut at a random length, a bit flipped, a few bytes overwritten, a byte set to 0x00 or 0xFF.
    private static IEnumerable<byte[]> Copies(byte[] sample, Random random)
    {
        for (int made = 0; made < CopiesOfEach; made++)
        {
            byte[] copy = [.. sample];
            switch (made % 4)
            {
                case 0:
                    yield return copy[..random.Next(0, copy.Length)];
                    continue;
                case 1:
                    for (int flips = random.Next(1, 4); flips > 0; flips--)
                    {
                        copy[random.Next(copy.Length)] ^= (byte)(1 << random.Next(8));
                    }

                    break;
                case 2:
                    for (int bytes = random.Next(1, 9); bytes > 0; bytes--)
                    {
                        copy[random.Next(copy.Length)] = (byte)random.Next(256);
                    }

                    break;
                default:
                    copy[random.Next(copy.Length)] = random.Next(2) == 0 ? (byte)0x00 : (byte)0xFF;
                    break;
            }

            yield return copy;
        }
    }

    [TestMethod]
    public void A_file_whose_own_numbers_ask_for_more_than_it_holds_is_refused_without_reading_or_making_room_for_it()
    {
        Tiff tiff = new(true);
        byte[] countless = tiff.Write([(0x010F, 2, uint.MaxValue, "Charkha\0"u8.ToArray()), (0x8825, 4, 1, (uint)0x7FFFFFF0)]);
        byte[] longChunk = [.. PngSignature, 0x7F, 0xFF, 0xFF, 0xF0, .. "zTXt"u8.ToArray(), .. "XML:com.adobe.xmp"u8.ToArray(), 0, 0];
        byte[] bigRiff = [.. "RIFF"u8.ToArray(), 0xF0, 0xFF, 0xFF, 0xFF, .. "WEBP"u8.ToArray(), .. "VP8 "u8.ToArray(), 4, 0, 0, 0, 1, 2, 3, 4];
        byte[] bigResource = Jpeg((0xED, [.. "Photoshop 3.0\0"u8.ToArray(), .. "8BIM"u8.ToArray(), 4, 4, 0, 0, 0x7F, 0xFF, 0xFF, 0xFF, 1, 2]));
        byte[] manyPieces = Jpeg((0xE1, [.. "http://ns.adobe.com/xmp/extension/\0"u8.ToArray(), .. new byte[32], 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, .. "x"u8.ToArray()]));

        foreach ((string name, byte[] file) in new[] { ("a.tif", countless), ("a.png", longChunk), ("a.webp", bigRiff), ("a.jpg", bigResource), ("b.jpg", manyPieces) })
        {
            long before = GC.GetAllocatedBytesForCurrentThread();
            PlaceResult result = PlaceRules.TakeOut(file, name);
            Assert.IsTrue(GC.GetAllocatedBytesForCurrentThread() - before < 1024 * 1024, name + " made room for what its numbers say");
            Assert.IsFalse(result.Why?.Contains(Failed, StringComparison.Ordinal) == true, name + " failed in a way nothing expects");
            Assert.AreNotEqual(PlaceOutcome.Removed, result.Outcome, name);
        }
    }

    [TestMethod]
    public void Every_sample_as_it_is_is_cleaned_and_cleaning_it_again_finds_nothing()
    {
        foreach ((string name, byte[] bytes) in Samples())
        {
            PlaceResult first = PlaceRules.TakeOut([.. bytes], name);
            Assert.AreEqual(name.EndsWith(".gif", StringComparison.Ordinal) ? PlaceOutcome.Refused : PlaceOutcome.Removed, first.Outcome, name + ": " + first.Why);
            if (first.Bytes is { } sent)
            {
                Assert.IsTrue(sent.AsSpan().IndexOf(LatitudeRationals) < 0, name + " still holds the latitude");
                Assert.IsFalse(Encoding.Latin1.GetString(sent).Contains("Chandigarh", StringComparison.Ordinal), name + " still holds the city");
                Assert.AreEqual(PlaceOutcome.None, PlaceRules.TakeOut([.. sent], name).Outcome, name + " cleaned again");
            }
        }
    }

    [TestMethod]
    public void Thousands_of_cut_and_changed_copies_are_each_answered_in_time_within_bounds_and_never_by_a_failure()
    {
        Random random = new(20261116);
        int answered = 0;
        Dictionary<PlaceOutcome, int> outcomes = [];
        TimeSpan slowest = TimeSpan.Zero;
        long most = 0;
        foreach ((string name, byte[] sample) in Samples())
        {
            foreach (byte[] copy in Copies(sample, random))
            {
                int length = copy.Length;
                long before = GC.GetAllocatedBytesForCurrentThread();
                Stopwatch watch = Stopwatch.StartNew();
                PlaceResult result = PlaceRules.TakeOut(copy, name);
                watch.Stop();
                long allocated = GC.GetAllocatedBytesForCurrentThread() - before;
                slowest = watch.Elapsed > slowest ? watch.Elapsed : slowest;
                most = Math.Max(most, allocated);
                answered++;
                outcomes[result.Outcome] = outcomes.GetValueOrDefault(result.Outcome) + 1;

                string seen = name + " copy " + answered + " of " + length + " bytes";
                Assert.IsTrue(watch.Elapsed < MostTime, seen + " took " + watch.Elapsed);
                Assert.IsTrue(allocated < MostAllocated, seen + " allocated " + allocated);
                Assert.IsFalse(result.Why?.Contains(Failed, StringComparison.Ordinal) == true, seen + " failed in a way nothing expects");
                if (result.Outcome == PlaceOutcome.Refused)
                {
                    Assert.IsFalse(string.IsNullOrEmpty(result.Why), seen + " was refused with no words");
                    continue;
                }

                Assert.IsNotNull(result.Bytes, seen);
                Assert.IsTrue(result.Bytes.Length <= length, seen + " grew");
                PlaceResult again = PlaceRules.TakeOut([.. result.Bytes], name);
                Assert.AreEqual(PlaceOutcome.None, again.Outcome, seen + ": cleaning what was sent found more: " + again.Why);
            }
        }

        Assert.AreEqual(5 * CopiesOfEach, answered);
        Console.WriteLine("answered " + answered + ": " + string.Join(", ", outcomes.Select(pair => pair.Key + " " + pair.Value)) + "; slowest " + slowest.TotalMilliseconds + " ms; most allocated " + most + " bytes");
    }
}
