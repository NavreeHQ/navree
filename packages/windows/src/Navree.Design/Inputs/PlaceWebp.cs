using System.Buffers.Binary;
using System.Text;

namespace Navree.Design.Inputs;

/// <summary>
/// WebP (Google's WebP container specification): a RIFF header whose File Size counts the bytes from offset 8, then padded chunks of a FourCC, a
/// little-endian size and data. 'EXIF' holds Exif metadata (some writers put the six bytes "Exif\0\0" first, as in a JPEG) and is cleaned in place;
/// 'XMP ' holds an XMP packet. 'VP8 ', 'VP8L', 'VP8X', 'ALPH', 'ANIM' and 'ICCP' are the picture and how it is shown and are kept; 'ANMF' frames
/// hold an alpha and a bitstream sub-chunk, kept, and possibly unknown chunks. An unknown chunk, at the end of the file or of a frame ("Readers
/// SHOULD ignore these chunks"), is emptied. Data after File Size ("SHOULD NOT contain any data after") is refused unless it is zeros.
/// </summary>
public static partial class PlaceRules
{
    private static void Webp(Cleaning work)
    {
        byte[] file = work.Bytes;
        long end = 8L + BinaryPrimitives.ReadUInt32LittleEndian(file.AsSpan(4));
        if (end > file.Length)
        {
            throw Doubted();
        }

        WebpChunks(work, 12, (int)end, false);
        if (file.AsSpan((int)end).ContainsAnyExcept((byte)0))
        {
            throw Doubted(Doubt.After);
        }
    }

    private static void WebpChunks(Cleaning work, int at, int end, bool inFrame)
    {
        byte[] file = work.Bytes;
        while (at < end)
        {
            if (at + 8 > end)
            {
                throw Doubted();
            }

            string kind = Encoding.ASCII.GetString(file, at, 4);
            uint size = BinaryPrimitives.ReadUInt32LittleEndian(file.AsSpan(at + 4));
            long data = at + 8L;
            long stop = data + size;
            if (stop + (size & 1) > end)
            {
                throw Doubted();
            }

            int start = (int)data;
            int finish = (int)stop;
            switch (kind)
            {
                case "VP8 " or "VP8L" or "ALPH":
                    break;
                case "VP8X" or "ANIM" or "ICCP" when !inFrame:
                    break;
                case "ANMF" when !inFrame:
                    // A frame: 16 bytes of its place and timing, then its own chunks.
                    if (finish - start < 16)
                    {
                        throw Doubted();
                    }

                    WebpChunks(work, start + 16, finish, true);
                    break;
                case "EXIF" when !inFrame:
                    int skip = file.AsSpan(start, finish - start).StartsWith("Exif\0\0"u8) ? 6 : 0;
                    new TiffBlock(work, start + skip, finish - start - skip, 1, false).Clean();
                    break;
                case "XMP " when !inFrame:
                    Xmp(work, start, finish);
                    break;
                default:
                    work.Zero(start, finish);
                    break;
            }

            at = finish + (int)(size & 1);
        }
    }
}
