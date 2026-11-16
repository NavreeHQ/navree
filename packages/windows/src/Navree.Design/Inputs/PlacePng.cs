using System.Buffers.Binary;
using System.IO.Compression;
using System.Text;

namespace Navree.Design.Inputs;

/// <summary>
/// PNG (the PNG specification, third edition): chunks of a length, a type, data and a CRC over type and data (5.3). eXIf holds an Exif profile
/// (11.3.4.5) and is cleaned in place, its CRC made again. tEXt, zTXt and iTXt (11.3.3.2 to 11.3.3.4) are read: one with the XMP keyword
/// (<c>XML:com.adobe.xmp</c>, 11.3.3.1 and XMP Part 3, 1.1.5) goes when its packet holds a place, compressed or not (a compressed one is expanded
/// only up to <see cref="MostText"/> bytes; past that, or if it cannot be expanded, the file is refused); one whose keyword starts
/// <c>Raw profile type</c> holds a whole profile (EXIF, IPTC or XMP) written as hex text and always goes; one whose keyword names a place goes; the
/// others (a title, a description, a comment) are a person's words and stay. An ancillary chunk this code does not know goes (13.1: "can simply be
/// ignored"); a critical one it does not know is refused ("a fatal condition"). A chunk that goes is taken out of the file, which holds no offsets
/// between chunks, so the picture's own chunks keep every byte. A file with no IEND (11.2.4) or with anything but zeros after it (13.3: "data after
/// the IEND chunk which could contain anything") is refused.
/// </summary>
public static partial class PlaceRules
{
    /// <summary>The most bytes a compressed text chunk is expanded to before the file is refused.</summary>
    public const int MostText = 8 * 1024 * 1024;

    // The chunks of the PNG specification's list (4.8.2, Table 7) that hold no place: kept as they are.
    private static readonly HashSet<string> PngKept = new(StringComparer.Ordinal)
    {
        "IHDR", "PLTE", "IDAT", "tRNS", "cHRM", "gAMA", "iCCP", "sBIT", "sRGB", "cICP", "mDCV", "cLLI", "bKGD", "hIST", "pHYs", "sPLT", "tIME",
        "acTL", "fcTL", "fdAT",
    };

    private static void Png(Cleaning work)
    {
        byte[] file = work.Bytes;
        List<(int At, int Length)> going = [];
        int at = 8;
        bool ended = false;
        while (at < file.Length && !ended)
        {
            if (at + 12 > file.Length)
            {
                throw Doubted();
            }

            uint size = BinaryPrimitives.ReadUInt32BigEndian(file.AsSpan(at));
            int data = at + 8;
            if (size > int.MaxValue || data + (long)size + 4 > file.Length)
            {
                throw Doubted();
            }

            ReadOnlySpan<byte> type = file.AsSpan(at + 4, 4);
            foreach (byte letter in type)
            {
                if (!char.IsAsciiLetter((char)letter))
                {
                    throw Doubted();
                }
            }

            string kind = Encoding.ASCII.GetString(type);
            int end = data + (int)size;
            int next = end + 4;
            switch (kind)
            {
                case "IEND":
                    ended = true;
                    break;
                case "eXIf":
                    bool before = work.Changed;
                    work.Changed = false;
                    new TiffBlock(work, data, (int)size, 1, false).Clean();
                    if (work.Changed)
                    {
                        BinaryPrimitives.WriteUInt32BigEndian(file.AsSpan(end), Crc32(file.AsSpan(at + 4, 4 + (int)size)));
                    }

                    work.Changed |= before;
                    break;
                case "tEXt" or "zTXt" or "iTXt":
                    if (TextGoes(file, kind, data, end))
                    {
                        going.Add((at, next - at));
                    }

                    break;
                default:
                    if (PngKept.Contains(kind))
                    {
                        break;
                    }

                    if (char.IsAsciiLetterUpper(kind[0]))
                    {
                        throw Doubted();
                    }

                    going.Add((at, next - at));
                    break;
            }

            at = next;
        }

        if (!ended)
        {
            throw Doubted();
        }

        if (file.AsSpan(at).ContainsAnyExcept((byte)0))
        {
            throw Doubted(Doubt.After);
        }

        if (going.Count == 0)
        {
            return;
        }

        byte[] kept = new byte[file.Length - going.Sum(chunk => chunk.Length)];
        int from = 0;
        int to = 0;
        foreach ((int start, int length) in going)
        {
            file.AsSpan(from, start - from).CopyTo(kept.AsSpan(to));
            to += start - from;
            from = start + length;
        }

        file.AsSpan(from).CopyTo(kept.AsSpan(to));
        work.Bytes = kept;
        work.Changed = true;
    }

    // Whether a text chunk at file[data .. end] goes: its keyword is 1 to 79 bytes and a NUL; the rest is as its kind says.
    private static bool TextGoes(byte[] file, string kind, int data, int end)
    {
        int nul = Array.IndexOf(file, (byte)0, data, Math.Min(80, end - data));
        if (nul <= data)
        {
            throw Doubted();
        }

        string keyword = Encoding.Latin1.GetString(file, data, nul - data);
        if (keyword.StartsWith("Raw profile type", StringComparison.Ordinal))
        {
            return true;
        }

        if (keyword != "XML:com.adobe.xmp")
        {
            return keyword.Split([' ', '_', '-', ':', '.'], StringSplitOptions.RemoveEmptyEntries).Any(word => IsPlace(word.ToLowerInvariant()))
                || IsPlace(keyword.Replace(" ", string.Empty, StringComparison.Ordinal).ToLowerInvariant());
        }

        byte[] text;
        switch (kind)
        {
            case "tEXt":
                text = file[(nul + 1)..end];
                break;
            case "zTXt":
                if (nul + 2 > end || file[nul + 1] != 0)
                {
                    throw Doubted();
                }

                text = Inflate(file, nul + 2, end);
                break;
            default:
                // Compression flag, compression method, language tag and NUL, translated keyword and NUL, then the text.
                if (nul + 3 > end)
                {
                    throw Doubted();
                }

                int language = Array.IndexOf(file, (byte)0, nul + 3, end - nul - 3);
                int translated = language < 0 ? -1 : Array.IndexOf(file, (byte)0, language + 1, end - language - 1);
                if (translated < 0)
                {
                    throw Doubted();
                }

                text = (file[nul + 1], file[nul + 2]) switch
                {
                    (0, _) => file[(translated + 1)..end],
                    (1, 0) => Inflate(file, translated + 1, end),
                    _ => throw Doubted(),
                };
                break;
        }

        if (text.AsSpan().Contains((byte)0))
        {
            throw Doubted();
        }

        return HoldsPlace(text);
    }

    // Expands a zlib stream, refusing what would expand past MostText bytes and what is not a zlib stream.
    private static byte[] Inflate(byte[] file, int from, int end)
    {
        try
        {
            using MemoryStream source = new(file, from, end - from, false);
            using ZLibStream zlib = new(source, CompressionMode.Decompress);
            byte[] buffer = new byte[64 * 1024];
            int length = 0;
            while (true)
            {
                if (length == buffer.Length)
                {
                    if (buffer.Length >= MostText)
                    {
                        if (zlib.ReadByte() >= 0)
                        {
                            throw Doubted();
                        }

                        break;
                    }

                    Array.Resize(ref buffer, Math.Min(buffer.Length * 2, MostText));
                }

                int got = zlib.Read(buffer, length, buffer.Length - length);
                if (got == 0)
                {
                    break;
                }

                length += got;
            }

            return buffer[..length];
        }
        catch (InvalidDataException)
        {
            throw Doubted();
        }
    }
}
