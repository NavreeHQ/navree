using System.Buffers.Binary;
using System.Text;

namespace Navree.Design.Inputs;

/// <summary>What a file is, read from its first bytes: never from its name or from a type someone claimed for it.</summary>
public enum PictureKind
{
    /// <summary>None of the pictures below: a paper, a video, a sound, text.</summary>
    NotAPicture,

    /// <summary>JPEG (and JFIF, whatever the file is named).</summary>
    Jpeg,

    /// <summary>PNG.</summary>
    Png,

    /// <summary>WebP.</summary>
    Webp,

    /// <summary>TIFF, and the pictures built on it.</summary>
    Tiff,

    /// <summary>GIF.</summary>
    Gif,

    /// <summary>BMP.</summary>
    Bmp,

    /// <summary>SVG.</summary>
    Svg,

    /// <summary>HEIC: HEIF holding HEVC pictures.</summary>
    Heic,

    /// <summary>AVIF: HEIF holding AV1 pictures.</summary>
    Avif,

    /// <summary>The rest of the HEIF family.</summary>
    Heif,

    /// <summary>JPEG XL, bare or in its container.</summary>
    JpegXl,

    /// <summary>JPEG 2000.</summary>
    Jpeg2000,

    /// <summary>BigTIFF.</summary>
    BigTiff,

    /// <summary>A file of the ISO media family that names neither a picture nor a video this code knows.</summary>
    OtherMedia,

    /// <summary>A Photoshop document (PSD, PSB): it keeps the EXIF and XMP of the photo it was made from.</summary>
    Photoshop,

    /// <summary>A camera's raw picture whose first bytes say so, whatever the file is named.</summary>
    Raw,

    /// <summary>A GIMP document (XCF).</summary>
    Gimp,

    /// <summary>An OpenRaster document: a ZIP archive whose first file says so.</summary>
    OpenRaster,

    /// <summary>JPEG XR (HD Photo).</summary>
    JpegXr,
}

/// <summary>Reads what kind of file a file is from its first bytes.</summary>
public static class PlaceKinds
{
    /// <summary>How many of a file's first bytes are enough to say what it is.</summary>
    public const int Head = 4096;

    // The brands of the ISO media files of the HEIF family (the brands page of the MP4 registration authority, mp4ra.org): the pictures this client
    // cannot take a place out of. The AVIF brands are named apart so that the words can say AVIF.
    private static readonly HashSet<string> HeicBrands = new(StringComparer.Ordinal) { "heic", "heix", "heim", "heis", "hevc", "hevx", "hevm", "hevs" };

    private static readonly HashSet<string> AvifBrands = new(StringComparer.Ordinal) { "avif", "avis", "MA1B", "MA1A" };

    private static readonly HashSet<string> HeifBrands = new(StringComparer.Ordinal)
    {
        "mif1", "msf1", "mif2", "miaf", "1pic", "avci", "avcs", "evbi", "evbs", "evmi", "evms", "jpeg", "jpgs", "vvic", "vvis", "j2ki", "j2ks", "j2is",
        "pred", "tmap", "MiAB", "MiAC", "MiAn", "MiBu", "MiCm", "MiHA", "MiHB", "MiHE", "MiPr",
    };

    // Brands of plain video and sound in ISO media files (same page): such a file is not a picture.
    private static readonly HashSet<string> MovingBrands = new(StringComparer.Ordinal)
    {
        "isom", "iso2", "iso3", "iso4", "iso5", "iso6", "iso7", "iso8", "iso9", "mp41", "mp42", "qt  ", "avc1", "M4V ", "M4A ", "M4P ", "M4B ",
        "3gp4", "3gp5", "3gp6", "3gp7", "3gp8", "3gp9", "3g2a", "3g2b", "3g2c", "dash", "mmp4", "MSNV", "f4v ", "f4a ",
    };

    /// <summary>What kind of file this is, from its first bytes.</summary>
    /// <param name="head">The file's first bytes: <see cref="Head"/> of them are enough; fewer when the file is shorter.</param>
    /// <returns>The kind.</returns>
    public static PictureKind KindOf(ReadOnlySpan<byte> head)
    {
        if (head.StartsWith((ReadOnlySpan<byte>)[0xFF, 0xD8, 0xFF]))
        {
            return PictureKind.Jpeg;
        }

        if (head.StartsWith((ReadOnlySpan<byte>)[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]))
        {
            return PictureKind.Png;
        }

        if (head.StartsWith("RIFF"u8) && head.Length >= 12 && head[8..12].SequenceEqual("WEBP"u8))
        {
            return PictureKind.Webp;
        }

        // A Photoshop document and the raw pictures that say what they are in their first bytes are known before TIFF is, since some of
        // them begin as a TIFF does. This client takes no place out of either: they are refused unless the field keeps the place.
        if (head.StartsWith("8BPS"u8))
        {
            return PictureKind.Photoshop;
        }

        if (IsRaw(head))
        {
            return PictureKind.Raw;
        }

        // A GIMP document: "byte[9] "gimp xcf " File type identification" (the XCF format's own page, developer.gimp.org/core/standards/xcf).
        if (head.StartsWith("gimp xcf "u8))
        {
            return PictureKind.Gimp;
        }

        // JPEG XR: a file begins with a FIXED_FILE_HEADER_II_2BYTES field "equal to 0x4949, followed by a FIXED_FILE_HEADER_0XBC_BYTE field
        // equal to 0xBC" (the IANA registration of image/jxr). It begins as a little-endian TIFF does, with another number where 42 would be.
        if (head.StartsWith((ReadOnlySpan<byte>)[0x49, 0x49, 0xBC]))
        {
            return PictureKind.JpegXr;
        }

        if (IsOpenRaster(head))
        {
            return PictureKind.OpenRaster;
        }

        if (head.StartsWith("II*\0"u8) || head.StartsWith("MM\0*"u8))
        {
            return PictureKind.Tiff;
        }

        if (head.StartsWith("II+\0"u8) || head.StartsWith("MM\0+"u8))
        {
            return PictureKind.BigTiff;
        }

        // JPEG XL: a bare codestream starts FF 0A; the container starts with its signature box (the IANA registration of image/jxl).
        if (head.StartsWith((ReadOnlySpan<byte>)[0xFF, 0x0A]) || head.StartsWith((ReadOnlySpan<byte>)[0, 0, 0, 0x0C, 0x4A, 0x58, 0x4C, 0x20, 0x0D, 0x0A, 0x87, 0x0A]))
        {
            return PictureKind.JpegXl;
        }

        // JPEG 2000: every file of the family starts with this signature box (RFC 3745).
        if (head.StartsWith((ReadOnlySpan<byte>)[0, 0, 0, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A]))
        {
            return PictureKind.Jpeg2000;
        }

        if (head.Length >= 12 && head[4..8].SequenceEqual("ftyp"u8))
        {
            return Brands(head);
        }

        if (head.StartsWith("GIF87a"u8) || head.StartsWith("GIF89a"u8))
        {
            return PictureKind.Gif;
        }

        if (head.StartsWith("BM"u8))
        {
            return PictureKind.Bmp;
        }

        return LooksLikeSvg(head) ? PictureKind.Svg : PictureKind.NotAPicture;
    }

    /// <summary>The media type of a kind of picture, or null when it is not a picture.</summary>
    /// <param name="kind">The kind.</param>
    /// <returns>The media type.</returns>
    public static string? MediaTypeOf(PictureKind kind) => kind switch
    {
        PictureKind.Jpeg => "image/jpeg",
        PictureKind.Png => "image/png",
        PictureKind.Webp => "image/webp",
        PictureKind.Tiff or PictureKind.BigTiff => "image/tiff",
        PictureKind.Gif => "image/gif",
        PictureKind.Bmp => "image/bmp",
        PictureKind.Svg => "image/svg+xml",
        PictureKind.Heic => "image/heic",
        PictureKind.Avif => "image/avif",
        PictureKind.Heif => "image/heif",
        PictureKind.JpegXl => "image/jxl",
        PictureKind.Jpeg2000 => "image/jp2",
        PictureKind.Photoshop => "image/vnd.adobe.photoshop",
        PictureKind.Gimp => "image/x-xcf",
        PictureKind.OpenRaster => "image/openraster",
        PictureKind.JpegXr => "image/jxr",
        _ => null,
    };

    /// <summary>The kind's name in the words said at a field.</summary>
    /// <param name="kind">The kind.</param>
    /// <returns>A name such as <c>HEIC</c>.</returns>
    public static string Named(PictureKind kind) => kind switch
    {
        PictureKind.JpegXl => "JPEG XL",
        PictureKind.Jpeg2000 => "JPEG 2000",
        PictureKind.BigTiff => "BigTIFF",
        PictureKind.OtherMedia => "ISO media",
        PictureKind.Photoshop => "Photoshop",
        PictureKind.Gimp => "GIMP",
        PictureKind.OpenRaster => "OpenRaster",
        PictureKind.JpegXr => "JPEG XR",
        PictureKind.Raw => "raw",
        _ => kind.ToString().ToUpperInvariant(),
    };

    // An OpenRaster document is a ZIP archive: "The first file in the archive must be called `mimetype`", "It must be STORED without
    // compression", and it "must contain the string `image/openraster`" (the OpenRaster file layout specification, openraster.org). In a
    // ZIP archive's first file header the name's length is at byte 26, the length of what follows the name at byte 28, and the name at byte
    // 30, with the file's content after them.
    private static bool IsOpenRaster(ReadOnlySpan<byte> head)
    {
        if (head.Length < 38 || !head.StartsWith("PK\u0003\u0004"u8) || BinaryPrimitives.ReadUInt16LittleEndian(head[26..]) != 8 || !head[30..38].SequenceEqual("mimetype"u8))
        {
            return false;
        }

        int content = 38 + BinaryPrimitives.ReadUInt16LittleEndian(head[28..]);
        return content <= head.Length && head[content..].StartsWith("image/openraster"u8);
    }

    // A camera's raw picture by its first bytes. From ExifTool's list of file signatures (lib/Image/ExifTool.pm, %magicNumber): Fujifilm RAF
    // 'FUJIFILM', Sigma X3F 'FOVb', Minolta MRW '\0MR[MI]', Canon CRW '(II|MM).{4}HEAP(CCDR|JPGM)'. Not in that list, and written here from
    // the formats' own headers: Olympus ORF, which begins as a TIFF does with another number in place of 42 ("IIRO", "IIRS", "MMOR"); Panasonic
    // RW2, likewise ("II" then 0x55 0x00); and Canon CR2, a TIFF with "CR" at its ninth byte. A raw picture that is a plain TIFF by its bytes
    // (DNG, NEF, ARW and others) is known by its name only, and read as the TIFF it is when it has another name.
    private static bool IsRaw(ReadOnlySpan<byte> head)
    {
        if (head.StartsWith("FUJIFILM"u8) || head.StartsWith("FOVb"u8) || head.StartsWith("\0MRM"u8) || head.StartsWith("\0MRI"u8))
        {
            return true;
        }

        if (head.StartsWith("IIRO"u8) || head.StartsWith("IIRS"u8) || head.StartsWith("MMOR"u8) || head.StartsWith((ReadOnlySpan<byte>)[0x49, 0x49, 0x55, 0x00]))
        {
            return true;
        }

        bool ordered = head.StartsWith("II"u8) || head.StartsWith("MM"u8);
        if (ordered && head.Length >= 14 && head[6..10].SequenceEqual("HEAP"u8) && (head[10..14].SequenceEqual("CCDR"u8) || head[10..14].SequenceEqual("JPGM"u8)))
        {
            return true;
        }

        return (head.StartsWith("II*\0"u8) || head.StartsWith("MM\0*"u8)) && head.Length >= 10 && head[8] == (byte)'C' && head[9] == (byte)'R';
    }

    // The brands of an ISO media file: its major brand and the compatible ones, read from its first box (the 'ftyp' box) as far as the head goes.
    private static PictureKind Brands(ReadOnlySpan<byte> head)
    {
        uint size = BinaryPrimitives.ReadUInt32BigEndian(head);
        int end = size < 16 ? head.Length : (int)Math.Min(size, (uint)head.Length);
        List<string> brands = [Encoding.ASCII.GetString(head[8..12])];
        for (int at = 16; at + 4 <= end; at += 4)
        {
            brands.Add(Encoding.ASCII.GetString(head[at..(at + 4)]));
        }

        if (brands.Any(HeicBrands.Contains))
        {
            return PictureKind.Heic;
        }

        if (brands.Any(AvifBrands.Contains))
        {
            return PictureKind.Avif;
        }

        if (brands.Any(brand => brand == "jxl "))
        {
            return PictureKind.JpegXl;
        }

        if (brands.Any(brand => brand is "jp2 " or "jpx " or "jpm "))
        {
            return PictureKind.Jpeg2000;
        }

        if (brands.Any(HeifBrands.Contains))
        {
            return PictureKind.Heif;
        }

        return brands.Where(brand => brand != "\0\0\0\0").All(MovingBrands.Contains) ? PictureKind.NotAPicture : PictureKind.OtherMedia;
    }

    // An SVG is XML text whose first element is svg: the head, past a byte order mark, white space, the XML declaration and comments, holds "<svg".
    private static bool LooksLikeSvg(ReadOnlySpan<byte> head)
    {
        ReadOnlySpan<byte> rest = head.StartsWith((ReadOnlySpan<byte>)[0xEF, 0xBB, 0xBF]) ? head[3..] : head;
        rest = rest.TrimStart(" \t\r\n"u8);
        return rest.StartsWith("<"u8) && IndexOfIgnoringCase(rest, "<svg"u8) >= 0;
    }

    private static int IndexOfIgnoringCase(ReadOnlySpan<byte> text, ReadOnlySpan<byte> lower)
    {
        for (int at = 0; at + lower.Length <= text.Length; at++)
        {
            int matched = 0;
            while (matched < lower.Length && (byte)(text[at + matched] | 0x20) == lower[matched])
            {
                matched++;
            }

            if (matched == lower.Length)
            {
                return at;
            }
        }

        return -1;
    }
}
