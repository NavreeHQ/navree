// A picture leaves the device only if this code can say where every place in it could be and has emptied each; what it cannot be sure of is refused with words, never sent as it is.
namespace Navree.Design.Inputs;

/// <summary>What became of the place a picture was taken.</summary>
public enum PlaceOutcome
{
    /// <summary>The picture holds no place (or is not a picture): nothing was changed.</summary>
    None,

    /// <summary>The place was taken out: <see cref="PlaceResult.Bytes"/> is what to send.</summary>
    Removed,

    /// <summary>The picture may hold a place and this client cannot take it out of this kind of picture, or cannot read it well enough to be sure.</summary>
    Refused,
}

/// <summary>What <see cref="PlaceRules.TakeOut"/> found.</summary>
/// <param name="Outcome">What became of the place.</param>
/// <param name="Why">When refused, the words to say at the field.</param>
public sealed record PlaceResult(PlaceOutcome Outcome, string? Why = null)
{
    /// <summary>What to send when the picture was not refused: the bytes handed in (changed in place), or new bytes when parts of the file went.</summary>
    public byte[]? Bytes { get; init; }
}

/// <summary>
/// Takes the place a photograph was taken out of a picture before it is sent, unless the field keeps it. What a file is comes from its first bytes
/// (<see cref="PlaceKinds"/>), and for each kind every part that can hold a place is read: what holds one is emptied where it is written, so the
/// picture's own data (a JPEG's scans, a PNG's IDAT, a TIFF's strips and tiles) never changes, and a part this code does not read is emptied or the
/// file refused. Where each kind can hold a place, with the specification each fact comes from, is written beside the code of that kind:
/// <list type="bullet">
/// <item>JPEG (PlaceJpeg.cs): every segment up to the end of the picture, scans included; every picture after the first; anything else after it is refused, a motion photo's video with words of its own.</item>
/// <item>TIFF and the EXIF block (PlaceTiff.cs): every directory, its GPS directory, the maker's note, the XMP, IPTC and Photoshop tags, the thumbnail; a camera's raw picture is refused.</item>
/// <item>PNG (PlacePng.cs) and WebP (PlaceWebp.cs): the EXIF and XMP chunks, the text chunks, and chunks this code does not know.</item>
/// <item>XMP anywhere (PlaceXmp.cs): a packet that holds any property about a place goes whole.</item>
/// <item>GIF, BMP and SVG: searched for a property about a place, and refused when one is found.</item>
/// </list>
/// Refused by their bytes: HEIC, HEIF, AVIF, JPEG XL, JPEG 2000, JPEG XR, BigTIFF, a Photoshop, GIMP or OpenRaster document, and the raw
/// pictures whose first bytes say what they are. Refused by their name, when their bytes say nothing this code knows: a file named as a
/// picture, as a camera's raw picture, or as a picture editor's document. A file that is none of these by its bytes or its name is not
/// looked into and is sent as it is. Not looked into either: words a person wrote (a caption, a comment), a video.
/// </summary>
public static partial class PlaceRules
{
    // How deep pictures and blocks inside one another are followed (a thumbnail in an EXIF block in a JPEG is two), and how many pictures one JPEG file may hold.
    private const int Deepest = 4;
    private const int MostPictures = 64;

    // The raw formats of cameras, by extension (XMP Specification Part 3, 1.1.1.1, Table 4): many look like TIFF and are not ("Do not attempt to
    // process camera raw files with generic TIFF software"), so a file named so is refused whatever its bytes look like, unless they are a JPEG,
    // PNG or WebP. TIF is in that table too, for two makers, and is left out: a TIFF is read as one.
    private static readonly HashSet<string> RawExtensions = new(StringComparer.OrdinalIgnoreCase)
    {
        "dng", "crw", "cr2", "erf", "x3f", "raf", "3fr", "fff", "kdc", "dcr", "mos", "rwl", "mfw", "mef", "mrw", "orf", "nef", "nrw", "rw2",
        "raw", "pef", "iiq", "srw", "arw", "srf", "sr2",
    };

    // The documents of picture editors, by extension: Photoshop, GIMP, OpenRaster, Krita, Paint.NET, Affinity. Such a document keeps what the
    // photo it was made from held, in a form this code does not read, so a file named so is refused unless its bytes are a picture this code
    // cleans. Three of them are also known by their bytes (PlaceKinds).
    private static readonly HashSet<string> EditorExtensions = new(StringComparer.OrdinalIgnoreCase)
    {
        "psd", "psb", "xcf", "ora", "kra", "pdn", "afphoto", "afdesign",
    };

    /// <summary>Whether a media type is a picture.</summary>
    /// <param name="mediaType">The media type.</param>
    /// <returns>True for a picture.</returns>
    public static bool IsPicture(string mediaType) => mediaType.StartsWith("image/", StringComparison.OrdinalIgnoreCase);

    /// <summary>
    /// Whether a file must be looked into before it is sent from a field that does not keep the place: a picture by its bytes or by its name, or
    /// a camera's raw picture or a picture editor's document by its name. A file that is none of these is not looked into and is sent as it is.
    /// </summary>
    /// <param name="head">The file's first bytes (<see cref="PlaceKinds.Head"/> of them, or all of a shorter file).</param>
    /// <param name="name">The file's name.</param>
    /// <param name="mediaType">The media type its name says.</param>
    /// <returns>True when it must be looked into.</returns>
    public static bool MustLook(ReadOnlySpan<byte> head, string name, string mediaType) =>
        PlaceKinds.KindOf(head) != PictureKind.NotAPicture || IsPicture(mediaType) || IsRawName(name) || IsEditorName(name);

    /// <summary>Takes the place out of a picture.</summary>
    /// <param name="file">The whole file. It may be changed in place; when the outcome is a refusal it must not be sent.</param>
    /// <param name="name">The file's name, which can only make the code more careful: it never makes a picture be read as another kind.</param>
    /// <returns>What became of the place, and what to send.</returns>
    public static PlaceResult TakeOut(byte[] file, string name)
    {
        ArgumentNullException.ThrowIfNull(file);
        ArgumentNullException.ThrowIfNull(name);
        PictureKind kind = PlaceKinds.KindOf(file.AsSpan(0, Math.Min(file.Length, PlaceKinds.Head)));
        Cleaning work = new(file);
        try
        {
            switch (kind)
            {
                case PictureKind.Jpeg:
                    JpegFile(work);
                    break;
                case PictureKind.Png:
                    Png(work);
                    break;
                case PictureKind.Webp:
                    Webp(work);
                    break;
                case PictureKind.Tiff:
                    if (IsRawName(name))
                    {
                        return Refused(Doubt.Raw);
                    }

                    new TiffBlock(work, 0, file.Length, 0, true).Clean();
                    break;
                case PictureKind.Gif or PictureKind.Bmp or PictureKind.Svg:
                    // A GIF's XMP is read by a GIF reader as sub-blocks (XMP Part 3, 1.1.2), so it cannot be emptied in place: such a file is refused.
                    if (HoldsPlace(file))
                    {
                        return Refused(Doubt.Said);
                    }

                    break;
                case PictureKind.NotAPicture:
                    if (IsRawName(name))
                    {
                        return Refused(Doubt.Raw);
                    }

                    if (IsEditorName(name))
                    {
                        return RefuseDocument("a picture editor's");
                    }

                    // Not a picture by its bytes: a file whose name says it is one may still hold a place nobody here can see.
                    if (IsPicture(FileRules.MediaTypeOf(name)))
                    {
                        return Refused(Doubt.Unknown);
                    }

                    break;
                case PictureKind.OtherMedia:
                    return Refused(Doubt.Unknown);
                case PictureKind.Raw:
                    // A raw picture by its bytes, whatever it is named.
                    return Refused(Doubt.Raw);
                case PictureKind.Photoshop or PictureKind.Gimp or PictureKind.OpenRaster:
                    // An editor's document by its bytes, whatever it is named.
                    return RefuseDocument(kind == PictureKind.OpenRaster ? "an OpenRaster" : "a " + PlaceKinds.Named(kind));
                default:
                    return Refuse(kind);
            }
        }
        catch (PlaceDoubtException doubt)
        {
            return Refused(doubt.Why);
        }
        catch (Exception problem) when (problem is not OutOfMemoryException)
        {
            // Nothing above is meant to throw anything but a doubt; if something does, the picture is not sent.
            return Refused(Doubt.Fault);
        }

        return new PlaceResult(work.Changed ? PlaceOutcome.Removed : PlaceOutcome.None) { Bytes = work.Bytes };
    }

    /// <summary>Whether a name is that of a camera's raw picture.</summary>
    /// <param name="name">The file's name.</param>
    /// <returns>True when its extension is one of the raw formats.</returns>
    public static bool IsRawName(string name)
    {
        ArgumentNullException.ThrowIfNull(name);
        string shown = FileRules.DisplayName(name);
        int dot = shown.LastIndexOf('.');
        return dot > 0 && RawExtensions.Contains(shown[(dot + 1)..]);
    }

    /// <summary>Whether a name is that of a picture editor's own document.</summary>
    /// <param name="name">The file's name.</param>
    /// <returns>True when its extension is one of the editors' documents.</returns>
    public static bool IsEditorName(string name)
    {
        ArgumentNullException.ThrowIfNull(name);
        string shown = FileRules.DisplayName(name);
        int dot = shown.LastIndexOf('.');
        return dot > 0 && EditorExtensions.Contains(shown[(dot + 1)..]);
    }

    // An editor's document is asked for as such, so "save it as a JPEG" is only half an answer: the words also say what the form must do.
    private static PlaceResult RefuseDocument(string whose) =>
        new(PlaceOutcome.Refused, "The place a photo was taken cannot be taken out of " + whose + " document here, and this field does not keep the place, so it was not added. Save it as a JPEG or PNG picture and choose that; a form that asks for the document itself has to keep the place.");

    private static PlaceResult Refuse(PictureKind kind)
    {
        string named = PlaceKinds.Named(kind);
        string article = named[0] is 'A' or 'E' or 'I' or 'O' or 'U' ? "an " : "a ";
        return new PlaceResult(PlaceOutcome.Refused, "The place it was taken cannot be taken out of " + article + named + " picture here, so it was not added. Save it as a JPEG or PNG picture and choose that.");
    }

    private static PlaceResult Refused(Doubt why) => new(PlaceOutcome.Refused, why switch
    {
        Doubt.Video => "This photo holds a short video (a motion photo), and the video can hold the place it was taken, so it was not added. Save the photo as a still, without its video, and choose that.",
        Doubt.After => "Something follows the picture in this file that cannot be read here and may hold the place it was taken, so it was not added. Save it again as a JPEG or PNG picture and choose that.",
        Doubt.Raw => "The place it was taken cannot be taken out of a camera's raw picture here, so it was not added. Save it as a JPEG or PNG picture and choose that.",
        Doubt.Said => "This picture says where it was taken in a way that cannot be taken out here, so it was not added. Save it as a JPEG or PNG picture and choose that.",
        Doubt.Unknown => "The place it was taken cannot be looked for in this file here, because its bytes are not those of a picture this client reads, so it was not added. Save it as a JPEG or PNG picture and choose that.",
        Doubt.Fault => "The place it was taken could not be looked for in this picture, because reading it failed here, so it was not added. Save it again as a JPEG or PNG picture and choose that.",
        _ => "The place it was taken cannot be looked for in this picture, because a part of it cannot be read here, so it was not added. Save it again as a JPEG or PNG picture and choose that.",
    });

    private static PlaceDoubtException Doubted(Doubt why = Doubt.Unreadable) => new(why);

    /// <summary>Why a picture is refused.</summary>
    private enum Doubt
    {
        // A part of it cannot be read: cut short, a structure that does not hold together, a packet that is not UTF-8.
        Unreadable,

        // A motion photo's video follows the picture.
        Video,

        // Something that is not a picture follows the picture.
        After,

        // A camera's raw picture.
        Raw,

        // A GIF, BMP or SVG that names a place.
        Said,

        // The name says picture and the bytes say nothing this code reads.
        Unknown,

        // Reading it failed in a way nothing here expects.
        Fault,
    }

    /// <summary>The one thing the reading of a picture throws: it cannot be sure, and says why.</summary>
    private sealed class PlaceDoubtException : Exception
    {
        public PlaceDoubtException(Doubt why)
            : base(why.ToString()) => Why = why;

        public PlaceDoubtException()
        {
        }

        public PlaceDoubtException(string message)
            : base(message)
        {
        }

        public PlaceDoubtException(string message, Exception inner)
            : base(message, inner)
        {
        }

        public Doubt Why { get; }
    }

    /// <summary>The file being cleaned, and whether anything in it changed.</summary>
    private sealed class Cleaning(byte[] bytes)
    {
        public byte[] Bytes { get; set; } = bytes;

        public bool Changed { get; set; }

        // Fills bytes with zeros; a change only when one of them was not zero already.
        public void Zero(int from, int end) => Fill(from, end, 0);

        // Fills bytes with spaces: a value of words that is to say nothing.
        public void Spaces(int from, int end) => Fill(from, end, (byte)' ');

        private void Fill(int from, int end, byte value)
        {
            Span<byte> span = Bytes.AsSpan(from, end - from);
            if (span.ContainsAnyExcept(value))
            {
                span.Fill(value);
                Changed = true;
            }
        }
    }

    // ---- bytes ------------------------------------------------------------------------------------------------

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
