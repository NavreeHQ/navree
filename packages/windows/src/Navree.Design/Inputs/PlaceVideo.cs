using System.Buffers.Binary;
using System.Text;

namespace Navree.Design.Inputs;

/// <summary>
/// ISO base media file format and QuickTime movies (ISO/IEC 14496-12, 3GPP TS 26.244, Apple QuickTime File Format Specification, and Adobe XMP
/// Specification Part 3, 1.2.2): a sequence of size-and-type boxes, with media samples in <c>mdat</c> and movie structure and metadata in
/// <c>moov</c>, <c>udta</c>, <c>meta</c> and <c>uuid</c>. Location atoms (<c>©xyz</c>, <c>loci</c>, <c>gps </c>, GoPro <c>GPMF</c>, DJI <c>djmd</c>
/// and <c>dbgi</c>, Canon EXIF <c>uuid</c>, XMP boxes holding a place, QuickTime <c>mdta</c> location keys and their <c>ilst</c> entries, and
/// iTunes <c>mdir</c> location items) are removed by rebuilding the enclosing metadata and movie boxes and shifting <c>stco</c> and <c>co64</c>
/// chunk offsets by the exact byte delta of each <c>mdat</c>. A video with nothing of a place is returned untouched byte for byte. A video whose
/// location sits in a timed telemetry or subtitle track inside <c>mdat</c>, alongside <c>iloc</c>/<c>sidx</c> offsets, or in a malformed container
/// is refused rather than sent with a doubt.
/// </summary>
public static partial class PlaceRules
{
    // Adobe XMP UUID box in ISO base media files (XMP Specification Part 3, 1.2.2.1).
    private static ReadOnlySpan<byte> XmpBoxUuid =>
    [
        0xBE, 0x7A, 0xCF, 0xCB, 0x97, 0xA9, 0x42, 0xE8,
        0x9C, 0x71, 0x99, 0x94, 0x91, 0xE3, 0xAF, 0xAC,
    ];

    // Canon EOS movie EXIF/GPS UUID box stored inside moov.
    private static ReadOnlySpan<byte> CanonExifUuid =>
    [
        0x85, 0xC0, 0xB6, 0x87, 0x82, 0x0F, 0x11, 0xE0,
        0x81, 0x11, 0xF4, 0xCE, 0x46, 0x2B, 0x6A, 0x48,
    ];

    // User-data atoms that always hold location or camera/drone telemetry.
    private static readonly HashSet<string> UdtaLocationBoxes = new(StringComparer.Ordinal)
    {
        "\u00A9xyz", "loci", "LOCI", "xyz ", "\u00A9loc", "gps ", "GPS ", "GPMF", "djmd", "dbgi", "CNTH", "CNCV", "CNMN", "WLOC",
    };

    // Standard non-location user-data atoms kept when their payload holds no coordinates, embedded pictures or XMP place properties.
    private static readonly HashSet<string> UdtaSafeBoxes = new(StringComparer.Ordinal)
    {
        "\u00A9nam", "\u00A9cmt", "\u00A9day", "\u00A9ART", "\u00A9too", "\u00A9alb", "\u00A9wrt", "\u00A9cpy", "\u00A9gen", "\u00A9grp",
        "\u00A9lyr", "\u00A9mak", "\u00A9mod", "\u00A9swr", "\u00A9enc", "name", "titl", "dscp", "cprt", "auth", "perf", "gnre", "yrrc",
        "albm", "hnti", "hinf", "AllF", "SelO", "LOOP", "ptv ", "chpl", "wide", "smta",
    };

    // Boxes inside moov / trak / mdia / minf / stbl that hold timing, codec descriptors and sample tables (never location metadata).
    private static readonly HashSet<string> SafeMovieLeafBoxes = new(StringComparer.Ordinal)
    {
        "mvhd", "iods", "mvex", "ipmc", "pssh", "tkhd", "tref", "edts", "tapt", "load", "imap", "matt", "kmat", "clip", "crgn", "mdhd",
        "elng", "hdlr", "vmhd", "smhd", "hmhd", "nmhd", "gmhd", "dinf", "stsd", "stts", "ctts", "cslg", "stsc", "stsz", "stz2", "stco",
        "co64", "stss", "stsh", "padb", "stdp", "sdtp", "sbgp", "sgpd", "subs", "saiz", "wide",
    };

    private readonly record struct BoxHeader(int Start, int Data, int End, string Kind, byte[]? Uuid);

    private readonly record struct MdatRange(int Index, int OldStart, int OldEnd);

    private static void IsoBmffVideo(Cleaning work)
    {
        byte[] cleaned = CleanIsoBmff(work.Bytes, out bool changed);
        if (!changed)
        {
            return;
        }

        // Verify that the rewritten container parses cleanly and holds nothing more of a place.
        _ = CleanIsoBmff(cleaned, out bool stillDirty);
        if (stillDirty)
        {
            throw Doubted(Doubt.VideoSaid);
        }

        work.Bytes = cleaned;
        work.Changed = true;
    }

    private static byte[] CleanIsoBmff(byte[] file, out bool changed)
    {
        changed = false;
        if (file.Length < 8)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        List<BoxHeader> boxes = ReadBoxes(file, 0, file.Length, topLevel: true, allowZeroTerminator: false, allowRawTypes: false);
        if (boxes.Count == 0)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        List<byte[]> slices = new(boxes.Count);
        List<MdatRange> mdats = [];
        List<int> moovIndices = [];
        bool hasAbsoluteOffsets = false;
        bool hasSubtitleTrack = false;

        for (int index = 0; index < boxes.Count; index++)
        {
            BoxHeader box = boxes[index];
            switch (box.Kind)
            {
                case "ftyp" or "styp" or "wide" or "pnot":
                    slices.Add(Slice(file, box));
                    break;
                case "mdat":
                    mdats.Add(new MdatRange(index, box.Start, box.End));
                    slices.Add(Slice(file, box));
                    break;
                case "free" or "skip":
                    slices.Add(CleanFreeOrSkipBox(file, box, out bool freeChanged));
                    changed |= freeChanged;
                    break;
                case "moov":
                    byte[] cleanedMoov = CleanMoovBox(file, box, depth: 1, ref hasSubtitleTrack, out bool moovChanged);
                    moovIndices.Add(index);
                    slices.Add(cleanedMoov);
                    changed |= moovChanged;
                    break;
                case "moof":
                    byte[] cleanedMoof = CleanMoofBox(file, box, ref hasAbsoluteOffsets, out bool moofChanged);
                    slices.Add(cleanedMoof);
                    changed |= moofChanged;
                    break;
                case "mfra" or "sidx":
                    hasAbsoluteOffsets = true;
                    slices.Add(Slice(file, box));
                    break;
                case "meta":
                    byte[] cleanedMeta = CleanMetaBox(file, box, depth: 1, out bool metaChanged);
                    slices.Add(cleanedMeta);
                    changed |= metaChanged;
                    break;
                case "udta":
                    byte[] cleanedUdta = CleanUdtaBox(file, box, depth: 1, out bool udtaChanged);
                    slices.Add(cleanedUdta);
                    changed |= udtaChanged;
                    break;
                case "uuid":
                    byte[] cleanedUuid = CleanUuidBox(file, box, out bool uuidChanged);
                    slices.Add(cleanedUuid);
                    changed |= uuidChanged;
                    break;
                case "XMP_":
                    byte[] cleanedXmp = CleanRawXmpBox(file, box, out bool xmpChanged);
                    slices.Add(cleanedXmp);
                    changed |= xmpChanged;
                    break;
                case "emsg" or "prft":
                    if (PayloadMentionsPlace(file.AsSpan(box.Data, box.End - box.Data)))
                    {
                        slices.Add([]);
                        changed = true;
                    }
                    else
                    {
                        slices.Add(Slice(file, box));
                    }

                    break;
                default:
                    throw Doubted(Doubt.VideoUnreadable);
            }
        }

        if (hasSubtitleTrack)
        {
            foreach (MdatRange mdat in mdats)
            {
                int payloadStart = boxes[mdat.Index].Data;
                ReadOnlySpan<byte> payload = file.AsSpan(payloadStart, mdat.OldEnd - payloadStart);
                if (PayloadMentionsPlace(payload) || ContainsGpsCallout(payload))
                {
                    throw Doubted(Doubt.VideoTrack);
                }
            }
        }

        if (!changed)
        {
            return file;
        }

        int[] newStarts = new int[slices.Count];
        long totalLength = 0;
        for (int i = 0; i < slices.Count; i++)
        {
            if (totalLength > int.MaxValue)
            {
                throw Doubted(Doubt.VideoSaid);
            }

            newStarts[i] = (int)totalLength;
            totalLength += slices[i].Length;
        }

        if (totalLength > int.MaxValue)
        {
            throw Doubted(Doubt.VideoSaid);
        }

        bool mdatMoved = mdats.Any(mdat => newStarts[mdat.Index] != mdat.OldStart);
        if (mdatMoved)
        {
            if (hasAbsoluteOffsets)
            {
                throw Doubted(Doubt.VideoSaid);
            }

            foreach (int moovIndex in moovIndices)
            {
                AdjustChunkOffsetsInMoov(slices[moovIndex], mdats, newStarts);
            }
        }

        byte[] output = new byte[(int)totalLength];
        int cursor = 0;
        foreach (byte[] part in slices)
        {
            if (part.Length > 0)
            {
                part.CopyTo(output.AsSpan(cursor));
                cursor += part.Length;
            }
        }

        return output;
    }

    private static byte[] CleanMoovBox(byte[] file, BoxHeader moov, int depth, ref bool hasSubtitleTrack, out bool changed)
    {
        if (depth > 8)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        changed = false;
        List<BoxHeader> children = ReadBoxes(file, moov.Data, moov.End, topLevel: false, allowZeroTerminator: true, allowRawTypes: false);
        List<byte[]> parts = new(children.Count);

        foreach (BoxHeader child in children)
        {
            switch (child.Kind)
            {
                case "trak":
                    byte[] trak = CleanTrakBox(file, child, depth + 1, ref hasSubtitleTrack, out bool trakChanged);
                    parts.Add(trak);
                    changed |= trakChanged;
                    break;
                case "udta":
                    byte[] udta = CleanUdtaBox(file, child, depth + 1, out bool udtaChanged);
                    parts.Add(udta);
                    changed |= udtaChanged;
                    break;
                case "meta":
                    byte[] meta = CleanMetaBox(file, child, depth + 1, out bool metaChanged);
                    parts.Add(meta);
                    changed |= metaChanged;
                    break;
                case "uuid":
                    byte[] uuid = CleanUuidBox(file, child, out bool uuidChanged);
                    parts.Add(uuid);
                    changed |= uuidChanged;
                    break;
                case "XMP_":
                    byte[] xmp = CleanRawXmpBox(file, child, out bool xmpChanged);
                    parts.Add(xmp);
                    changed |= xmpChanged;
                    break;
                case "free" or "skip":
                    parts.Add(CleanFreeOrSkipBox(file, child, out bool moovFreeChanged));
                    changed |= moovFreeChanged;
                    break;
                default:
                    if (SafeMovieLeafBoxes.Contains(child.Kind) || child.Kind is "cmov" or "prfl" or "ctab" or "colr")
                    {
                        if (child.Kind == "cmov")
                        {
                            // Compressed movie headers can hide metadata inside compressed atoms.
                            throw Doubted(Doubt.VideoUnreadable);
                        }

                        parts.Add(Slice(file, child));
                    }
                    else
                    {
                        // Unrecognised box directly under moov (for example proprietary camera metadata): remove it.
                        parts.Add([]);
                        changed = true;
                    }

                    break;
            }
        }

        return changed ? BuildContainerBox("moov", parts) : Slice(file, moov);
    }

    private static byte[] CleanTrakBox(byte[] file, BoxHeader trak, int depth, ref bool hasSubtitleTrack, out bool changed)
    {
        changed = false;
        List<BoxHeader> children = ReadBoxes(file, trak.Data, trak.End, topLevel: false, allowZeroTerminator: true, allowRawTypes: false);
        List<byte[]> parts = new(children.Count);

        foreach (BoxHeader child in children)
        {
            switch (child.Kind)
            {
                case "mdia":
                    byte[] mdia = CleanMdiaBox(file, child, depth + 1, ref hasSubtitleTrack, out bool mdiaChanged);
                    parts.Add(mdia);
                    changed |= mdiaChanged;
                    break;
                case "udta":
                    byte[] udta = CleanUdtaBox(file, child, depth + 1, out bool udtaChanged);
                    parts.Add(udta);
                    changed |= udtaChanged;
                    break;
                case "meta":
                    byte[] meta = CleanMetaBox(file, child, depth + 1, out bool metaChanged);
                    parts.Add(meta);
                    changed |= metaChanged;
                    break;
                case "uuid":
                    byte[] uuid = CleanUuidBox(file, child, out bool uuidChanged);
                    parts.Add(uuid);
                    changed |= uuidChanged;
                    break;
                case "XMP_":
                    byte[] xmp = CleanRawXmpBox(file, child, out bool xmpChanged);
                    parts.Add(xmp);
                    changed |= xmpChanged;
                    break;
                case "free" or "skip":
                    parts.Add(CleanFreeOrSkipBox(file, child, out bool trakFreeChanged));
                    changed |= trakFreeChanged;
                    break;
                default:
                    if (SafeMovieLeafBoxes.Contains(child.Kind))
                    {
                        parts.Add(Slice(file, child));
                    }
                    else
                    {
                        parts.Add([]);
                        changed = true;
                    }

                    break;
            }
        }

        return changed ? BuildContainerBox("trak", parts) : Slice(file, trak);
    }

    private static byte[] CleanMdiaBox(byte[] file, BoxHeader mdia, int depth, ref bool hasSubtitleTrack, out bool changed)
    {
        changed = false;
        List<BoxHeader> children = ReadBoxes(file, mdia.Data, mdia.End, topLevel: false, allowZeroTerminator: false, allowRawTypes: false);
        string? handler = null;
        string handlerName = string.Empty;
        byte[] handlerNameBytes = [];
        BoxHeader? minfBox = null;

        foreach (BoxHeader child in children)
        {
            if (child.Kind == "hdlr")
            {
                (handler, handlerNameBytes, handlerName) = ReadHandlerInfo(file, child);
            }
            else if (child.Kind == "minf")
            {
                minfBox = child;
            }
        }

        if (handler is null)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        if (IsVideoPlaceKey(string.Empty, handlerName) || PayloadMentionsPlace(handlerNameBytes) || ContainsGpsCallout(handlerNameBytes))
        {
            throw Doubted(Doubt.VideoTrack);
        }

        // Only media, checked subtitle and checked metadata handlers may pass; timed telemetry tracks (gpmd, camm, rtmd, djmd, dbgi, fdsc, klv , etc.) are refused.
        switch (handler)
        {
            case "vide" or "soun" or "tmcd" or "clcp" or "auxv":
                break;
            case "text" or "sbtl" or "subt" or "subp":
                hasSubtitleTrack = true;
                break;
            case "meta":
                break;
            default:
                throw Doubted(Doubt.VideoTrack);
        }

        if (minfBox is BoxHeader presentMinf)
        {
            InspectMinfSampleEntries(file, presentMinf, handler);
        }
        else if (handler == "meta")
        {
            throw Doubted(Doubt.VideoTrack);
        }

        List<byte[]> parts = new(children.Count);
        foreach (BoxHeader child in children)
        {
            switch (child.Kind)
            {
                case "minf":
                    byte[] minf = CleanMinfBox(file, child, depth + 1, out bool minfChanged);
                    parts.Add(minf);
                    changed |= minfChanged;
                    break;
                case "udta":
                    byte[] udta = CleanUdtaBox(file, child, depth + 1, out bool udtaChanged);
                    parts.Add(udta);
                    changed |= udtaChanged;
                    break;
                case "meta":
                    byte[] meta = CleanMetaBox(file, child, depth + 1, out bool metaChanged);
                    parts.Add(meta);
                    changed |= metaChanged;
                    break;
                case "uuid":
                    byte[] uuid = CleanUuidBox(file, child, out bool uuidChanged);
                    parts.Add(uuid);
                    changed |= uuidChanged;
                    break;
                case "free" or "skip":
                    parts.Add(CleanFreeOrSkipBox(file, child, out bool mdiaFreeChanged));
                    changed |= mdiaFreeChanged;
                    break;
                default:
                    if (SafeMovieLeafBoxes.Contains(child.Kind))
                    {
                        parts.Add(Slice(file, child));
                    }
                    else
                    {
                        parts.Add([]);
                        changed = true;
                    }

                    break;
            }
        }

        return changed ? BuildContainerBox("mdia", parts) : Slice(file, mdia);
    }

    private static byte[] CleanMinfBox(byte[] file, BoxHeader minf, int depth, out bool changed)
    {
        changed = false;
        List<BoxHeader> children = ReadBoxes(file, minf.Data, minf.End, topLevel: false, allowZeroTerminator: false, allowRawTypes: false);
        List<byte[]> parts = new(children.Count);

        foreach (BoxHeader child in children)
        {
            switch (child.Kind)
            {
                case "stbl":
                    byte[] stbl = CleanStblBox(file, child, depth + 1, out bool stblChanged);
                    parts.Add(stbl);
                    changed |= stblChanged;
                    break;
                case "udta":
                    parts.Add(CleanUdtaBox(file, child, depth + 1, out bool udtaChanged));
                    changed |= udtaChanged;
                    break;
                case "meta":
                    parts.Add(CleanMetaBox(file, child, depth + 1, out bool metaChanged));
                    changed |= metaChanged;
                    break;
                case "uuid":
                    parts.Add(CleanUuidBox(file, child, out bool uuidChanged));
                    changed |= uuidChanged;
                    break;
                case "free" or "skip":
                    parts.Add(CleanFreeOrSkipBox(file, child, out bool minfFreeChanged));
                    changed |= minfFreeChanged;
                    break;
                default:
                    if (SafeMovieLeafBoxes.Contains(child.Kind))
                    {
                        parts.Add(Slice(file, child));
                    }
                    else
                    {
                        parts.Add([]);
                        changed = true;
                    }

                    break;
            }
        }

        return changed ? BuildContainerBox("minf", parts) : Slice(file, minf);
    }

    private static byte[] CleanStblBox(byte[] file, BoxHeader stbl, int depth, out bool changed)
    {
        changed = false;
        List<BoxHeader> children = ReadBoxes(file, stbl.Data, stbl.End, topLevel: false, allowZeroTerminator: false, allowRawTypes: false);
        List<byte[]> parts = new(children.Count);

        foreach (BoxHeader child in children)
        {
            switch (child.Kind)
            {
                case "udta":
                    parts.Add(CleanUdtaBox(file, child, depth + 1, out bool udtaChanged));
                    changed |= udtaChanged;
                    break;
                case "meta":
                    parts.Add(CleanMetaBox(file, child, depth + 1, out bool metaChanged));
                    changed |= metaChanged;
                    break;
                case "uuid":
                    parts.Add(CleanUuidBox(file, child, out bool uuidChanged));
                    changed |= uuidChanged;
                    break;
                case "saio":
                    ValidateSaio(file, child);
                    parts.Add(Slice(file, child));
                    break;
                case "free" or "skip":
                    parts.Add(CleanFreeOrSkipBox(file, child, out bool stblFreeChanged));
                    changed |= stblFreeChanged;
                    break;
                default:
                    if (SafeMovieLeafBoxes.Contains(child.Kind))
                    {
                        parts.Add(Slice(file, child));
                    }
                    else
                    {
                        parts.Add([]);
                        changed = true;
                    }

                    break;
            }
        }

        return changed ? BuildContainerBox("stbl", parts) : Slice(file, stbl);
    }

    private static void InspectMinfSampleEntries(byte[] file, BoxHeader minf, string handler)
    {
        bool sawStsd = false;
        List<BoxHeader> minfChildren = ReadBoxes(file, minf.Data, minf.End, topLevel: false, allowZeroTerminator: false, allowRawTypes: false);
        foreach (BoxHeader child in minfChildren)
        {
            if (child.Kind != "stbl")
            {
                continue;
            }

            List<BoxHeader> stblChildren = ReadBoxes(file, child.Data, child.End, topLevel: false, allowZeroTerminator: false, allowRawTypes: false);
            foreach (BoxHeader table in stblChildren)
            {
                if (table.Kind == "stsd")
                {
                    sawStsd = true;
                    InspectStsdBox(file, table, handler);
                }
            }
        }

        if (handler == "meta" && !sawStsd)
        {
            throw Doubted(Doubt.VideoTrack);
        }
    }

    private static void InspectStsdBox(byte[] file, BoxHeader stsd, string handler)
    {
        if (stsd.End - stsd.Data < 8)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        uint count = BinaryPrimitives.ReadUInt32BigEndian(file.AsSpan(stsd.Data + 4, 4));
        if (count == 0 && handler == "meta")
        {
            throw Doubted(Doubt.VideoTrack);
        }

        int at = stsd.Data + 8;
        for (uint i = 0; i < count; i++)
        {
            if (at >= stsd.End)
            {
                throw Doubted(Doubt.VideoUnreadable);
            }

            BoxHeader entry = ReadSingleBox(file, at, stsd.End, topLevel: false, allowRawTypes: true);
            at = entry.End;

            if (entry.Kind is "gpmd" or "camm" or "rtmd" or "djmd" or "dbgi" or "fdsc" or "klv " or "mett" or "metx" or "urim")
            {
                throw Doubted(Doubt.VideoTrack);
            }

            if (entry.Kind == "mebx")
            {
                // QuickTime timed metadata sample entry: 8 reserved/data-reference bytes, then child boxes including 'keys'.
                if (entry.End - entry.Data < 8)
                {
                    throw Doubted(Doubt.VideoUnreadable);
                }

                bool sawKeys = false;
                List<BoxHeader> mebxChildren = ReadBoxes(file, entry.Data + 8, entry.End, topLevel: false, allowZeroTerminator: true, allowRawTypes: false);
                foreach (BoxHeader mebxChild in mebxChildren)
                {
                    if (mebxChild.Kind == "keys")
                    {
                        sawKeys = true;
                        List<(string Namespace, string Value, byte[] Raw)> keys = ParseMdtaKeys(file, mebxChild);
                        if (keys.Any(k => IsVideoPlaceKey(k.Namespace, k.Value)))
                        {
                            throw Doubted(Doubt.VideoTrack);
                        }
                    }
                }

                if (!sawKeys)
                {
                    throw Doubted(Doubt.VideoTrack);
                }
            }
            else if (handler == "meta")
            {
                // An unknown timed metadata sample entry on a metadata track cannot be verified to hold no location.
                throw Doubted(Doubt.VideoTrack);
            }
        }
    }

    private static byte[] CleanMoofBox(byte[] file, BoxHeader moof, ref bool hasAbsoluteOffsets, out bool changed)
    {
        changed = false;
        List<BoxHeader> children = ReadBoxes(file, moof.Data, moof.End, topLevel: false, allowZeroTerminator: false, allowRawTypes: false);
        List<byte[]> parts = new(children.Count);

        foreach (BoxHeader child in children)
        {
            switch (child.Kind)
            {
                case "traf":
                    List<BoxHeader> trafChildren = ReadBoxes(file, child.Data, child.End, topLevel: false, allowZeroTerminator: false, allowRawTypes: false);
                    bool trafChanged = false;
                    List<byte[]> trafParts = new(trafChildren.Count);
                    foreach (BoxHeader trafChild in trafChildren)
                    {
                        if (trafChild.Kind == "tfhd" && trafChild.End - trafChild.Data >= 4)
                        {
                            uint flags = BinaryPrimitives.ReadUInt32BigEndian(file.AsSpan(trafChild.Data, 4)) & 0x00FFFFFFu;
                            if ((flags & 0x000001u) != 0)
                            {
                                hasAbsoluteOffsets = true;
                            }

                            trafParts.Add(Slice(file, trafChild));
                        }
                        else if (trafChild.Kind == "udta")
                        {
                            trafParts.Add(CleanUdtaBox(file, trafChild, depth: 2, out bool uChanged));
                            trafChanged |= uChanged;
                        }
                        else if (trafChild.Kind == "meta")
                        {
                            trafParts.Add(CleanMetaBox(file, trafChild, depth: 2, out bool mChanged));
                            trafChanged |= mChanged;
                        }
                        else if (trafChild.Kind == "uuid")
                        {
                            trafParts.Add(CleanUuidBox(file, trafChild, out bool idChanged));
                            trafChanged |= idChanged;
                        }
                        else if (trafChild.Kind is "free" or "skip")
                        {
                            trafParts.Add(CleanFreeOrSkipBox(file, trafChild, out bool trafFreeChanged));
                            trafChanged |= trafFreeChanged;
                        }
                        else
                        {
                            trafParts.Add(Slice(file, trafChild));
                        }
                    }

                    parts.Add(trafChanged ? BuildContainerBox("traf", trafParts) : Slice(file, child));
                    changed |= trafChanged;
                    break;
                case "udta":
                    parts.Add(CleanUdtaBox(file, child, depth: 2, out bool udtaChanged));
                    changed |= udtaChanged;
                    break;
                case "meta":
                    parts.Add(CleanMetaBox(file, child, depth: 2, out bool metaChanged));
                    changed |= metaChanged;
                    break;
                case "uuid":
                    parts.Add(CleanUuidBox(file, child, out bool uuidChanged));
                    changed |= uuidChanged;
                    break;
                case "free" or "skip":
                    parts.Add(CleanFreeOrSkipBox(file, child, out bool moofFreeChanged));
                    changed |= moofFreeChanged;
                    break;
                default:
                    parts.Add(Slice(file, child));
                    break;
            }
        }

        return changed ? BuildContainerBox("moof", parts) : Slice(file, moof);
    }

    private static byte[] CleanUdtaBox(byte[] file, BoxHeader udta, int depth, out bool changed)
    {
        if (depth > 8)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        changed = false;
        List<BoxHeader> children = ReadBoxes(file, udta.Data, udta.End, topLevel: false, allowZeroTerminator: true, allowRawTypes: false);
        List<byte[]> kept = new(children.Count);

        foreach (BoxHeader child in children)
        {
            if (UdtaLocationBoxes.Contains(child.Kind))
            {
                changed = true;
                continue;
            }

            if (child.Kind is "free" or "skip")
            {
                kept.Add(CleanFreeOrSkipBox(file, child, out bool udtaFreeChanged));
                changed |= udtaFreeChanged;
                continue;
            }

            if (child.Kind == "meta")
            {
                byte[] meta = CleanMetaBox(file, child, depth + 1, out bool metaChanged);
                if (meta.Length > 0)
                {
                    kept.Add(meta);
                }

                changed |= metaChanged;
                continue;
            }

            if (child.Kind == "XMP_")
            {
                byte[] xmp = CleanRawXmpBox(file, child, out bool xmpChanged);
                if (xmp.Length > 0)
                {
                    kept.Add(xmp);
                }

                changed |= xmpChanged;
                continue;
            }

            if (child.Kind == "uuid")
            {
                byte[] uuid = CleanUuidBox(file, child, out bool uuidChanged);
                if (uuid.Length > 0)
                {
                    kept.Add(uuid);
                }

                changed |= uuidChanged;
                continue;
            }

            ReadOnlySpan<byte> payload = file.AsSpan(child.Data, child.End - child.Data);
            ReadOnlySpan<byte> head = payload[..Math.Min(payload.Length, PlaceKinds.Head)];
            bool embeddedMedia = PlaceKinds.KindOf(head) != PictureKind.NotAPicture || PlaceKinds.VideoOf(payload) != VideoKind.NotAVideo;
            if (UdtaSafeBoxes.Contains(child.Kind) && !embeddedMedia && !PayloadMentionsPlace(payload))
            {
                kept.Add(Slice(file, child));
            }
            else
            {
                // Either a safe text box whose value holds coordinates/place properties/embedded media, or an unrecognised vendor user-data atom.
                changed = true;
            }
        }

        if (!changed)
        {
            return Slice(file, udta);
        }

        return kept.Count == 0 ? [] : BuildContainerBox("udta", kept);
    }

    private static byte[] CleanMetaBox(byte[] file, BoxHeader meta, int depth, out bool changed)
    {
        if (depth > 8)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        changed = false;
        ReadOnlySpan<byte> body = file.AsSpan(meta.Data, meta.End - meta.Data);
        if (body.Length == 0 || (body.Length == 4 && BinaryPrimitives.ReadUInt32BigEndian(body) == 0))
        {
            return Slice(file, meta);
        }

        // ISO FullBox 'meta' starts with 4-byte version/flags (0x00000000); QuickTime 'meta' starts immediately with a child atom whose size >= 8 or 1.
        int prefix = body.Length >= 4 && BinaryPrimitives.ReadUInt32BigEndian(body[..4]) == 0 ? 4 : 0;
        List<BoxHeader> children = ReadBoxes(file, meta.Data + prefix, meta.End, topLevel: false, allowZeroTerminator: true, allowRawTypes: false);

        string? handler = null;
        BoxHeader? keysBox = null;
        BoxHeader? ilstBox = null;

        foreach (BoxHeader child in children)
        {
            if (child.Kind is "iloc" or "iinf" or "iref" or "pitm" or "idat" or "iprp" or "bxml")
            {
                throw Doubted(Doubt.VideoSaid);
            }

            if (child.Kind == "hdlr")
            {
                handler = ReadHandlerType(file, child);
            }
            else if (child.Kind == "keys")
            {
                keysBox = child;
            }
            else if (child.Kind == "ilst")
            {
                ilstBox = child;
            }
        }

        if (keysBox is not null || handler == "mdta")
        {
            return CleanMdtaMetaBox(file, meta, prefix, children, keysBox, ilstBox, out changed);
        }

        return CleanMdirOrIsoMetaBox(file, meta, prefix, children, out changed);
    }

    private static byte[] CleanMdtaMetaBox(
        byte[] file,
        BoxHeader meta,
        int prefix,
        List<BoxHeader> children,
        BoxHeader? keysBox,
        BoxHeader? ilstBox,
        out bool changed)
    {
        changed = false;
        if (keysBox is not BoxHeader keysHeader)
        {
            if (ilstBox is not null)
            {
                throw Doubted(Doubt.VideoUnreadable);
            }

            return CleanMdirOrIsoMetaBox(file, meta, prefix, children, out changed);
        }

        List<(string Namespace, string Value, byte[] Raw)> keys = ParseMdtaKeys(file, keysHeader);
        bool[] dropKey = new bool[keys.Count + 1];
        for (int i = 0; i < keys.Count; i++)
        {
            if (IsVideoPlaceKey(keys[i].Namespace, keys[i].Value))
            {
                dropKey[i + 1] = true;
            }
        }

        List<BoxHeader> ilstItems = ilstBox is BoxHeader ilstHeader
            ? ReadBoxes(file, ilstHeader.Data, ilstHeader.End, topLevel: false, allowZeroTerminator: true, allowRawTypes: true)
            : [];

        List<(BoxHeader Item, uint OldIndex, byte[] Body, bool BodyChanged)> preparedItems = new(ilstItems.Count);
        foreach (BoxHeader item in ilstItems)
        {
            uint keyIndex = BinaryPrimitives.ReadUInt32BigEndian(file.AsSpan(item.Start + 4, 4));
            if (keyIndex == 0 || keyIndex > (uint)keys.Count)
            {
                throw Doubted(Doubt.VideoUnreadable);
            }

            if (dropKey[keyIndex])
            {
                continue;
            }

            byte[] cleanedBody = CleanIlstItemBody(file, item, out bool bodyChanged, out bool dropWholeItem);
            if (dropWholeItem)
            {
                dropKey[keyIndex] = true;
                continue;
            }

            preparedItems.Add((item, keyIndex, cleanedBody, bodyChanged));
        }

        int[] remap = new int[keys.Count + 1];
        List<byte[]> keptKeys = [];
        for (int i = 0; i < keys.Count; i++)
        {
            if (!dropKey[i + 1])
            {
                keptKeys.Add(keys[i].Raw);
                remap[i + 1] = keptKeys.Count;
            }
            else
            {
                changed = true;
            }
        }

        List<byte[]> parts = [];
        if (prefix == 4)
        {
            parts.Add(new byte[4]);
        }

        bool hasMeaningfulPayload = false;
        foreach (BoxHeader child in children)
        {
            switch (child.Kind)
            {
                case "hdlr" or "ctry" or "lang":
                    parts.Add(Slice(file, child));
                    break;
                case "free" or "skip":
                    parts.Add(CleanFreeOrSkipBox(file, child, out bool mdtaFreeChanged));
                    changed |= mdtaFreeChanged;
                    break;
                case "keys":
                    if (keptKeys.Count > 0)
                    {
                        parts.Add(BuildMdtaKeysBox(keptKeys));
                    }

                    break;
                case "ilst":
                    List<byte[]> keptItems = [];
                    foreach ((BoxHeader item, uint oldIndex, byte[] body, bool bodyChanged) in preparedItems)
                    {
                        int newIndex = remap[oldIndex];
                        if (newIndex == 0)
                        {
                            changed = true;
                            continue;
                        }

                        if (!bodyChanged && oldIndex == (uint)newIndex && item.Data - item.Start == 8)
                        {
                            keptItems.Add(Slice(file, item));
                        }
                        else
                        {
                            changed = true;
                            byte[] rebuilt = new byte[8 + body.Length];
                            BinaryPrimitives.WriteUInt32BigEndian(rebuilt.AsSpan(0, 4), (uint)rebuilt.Length);
                            BinaryPrimitives.WriteUInt32BigEndian(rebuilt.AsSpan(4, 4), (uint)newIndex);
                            body.CopyTo(rebuilt.AsSpan(8));
                            keptItems.Add(rebuilt);
                        }

                        hasMeaningfulPayload = true;
                    }

                    if (keptItems.Count > 0)
                    {
                        parts.Add(BuildContainerBox("ilst", keptItems));
                    }

                    break;
                case "xml ":
                    byte[] xml = CleanFullXmlBox(file, child, out bool xmlChanged);
                    changed |= xmlChanged;
                    if (xml.Length > 0)
                    {
                        parts.Add(xml);
                        hasMeaningfulPayload = true;
                    }

                    break;
                case "XMP_":
                    byte[] xmp = CleanRawXmpBox(file, child, out bool xmpChanged);
                    changed |= xmpChanged;
                    if (xmp.Length > 0)
                    {
                        parts.Add(xmp);
                        hasMeaningfulPayload = true;
                    }

                    break;
                case "uuid":
                    byte[] uuid = CleanUuidBox(file, child, out bool uuidChanged);
                    changed |= uuidChanged;
                    if (uuid.Length > 0)
                    {
                        parts.Add(uuid);
                        hasMeaningfulPayload = true;
                    }

                    break;
                default:
                    changed = true;
                    break;
            }
        }

        if (!changed)
        {
            return Slice(file, meta);
        }

        return !hasMeaningfulPayload ? [] : BuildContainerBox("meta", parts);
    }

    private static byte[] CleanMdirOrIsoMetaBox(byte[] file, BoxHeader meta, int prefix, List<BoxHeader> children, out bool changed)
    {
        changed = false;
        List<byte[]> parts = [];
        if (prefix == 4)
        {
            parts.Add(new byte[4]);
        }

        bool hasMeaningfulPayload = false;
        foreach (BoxHeader child in children)
        {
            switch (child.Kind)
            {
                case "hdlr":
                    parts.Add(Slice(file, child));
                    break;
                case "free" or "skip":
                    parts.Add(CleanFreeOrSkipBox(file, child, out bool mdirFreeChanged));
                    changed |= mdirFreeChanged;
                    break;
                case "ilst":
                    List<BoxHeader> items = ReadBoxes(file, child.Data, child.End, topLevel: false, allowZeroTerminator: true, allowRawTypes: true);
                    List<byte[]> keptItems = [];
                    foreach (BoxHeader item in items)
                    {
                        if (UdtaLocationBoxes.Contains(item.Kind) || IsVideoPlaceAtomType(item.Kind))
                        {
                            changed = true;
                            continue;
                        }

                        if (item.Kind == "----" && ReverseDnsHeaderNamesPlace(file, item))
                        {
                            changed = true;
                            continue;
                        }

                        byte[] cleanedBody = CleanIlstItemBody(file, item, out bool bodyChanged, out bool dropWholeItem);
                        if (dropWholeItem)
                        {
                            changed = true;
                            continue;
                        }

                        if (!bodyChanged)
                        {
                            keptItems.Add(Slice(file, item));
                        }
                        else
                        {
                            changed = true;
                            keptItems.Add(BuildBox(item.Kind, cleanedBody));
                        }

                        hasMeaningfulPayload = true;
                    }

                    if (keptItems.Count > 0)
                    {
                        parts.Add(BuildContainerBox("ilst", keptItems));
                    }

                    break;
                case "xml ":
                    byte[] xml = CleanFullXmlBox(file, child, out bool xmlChanged);
                    changed |= xmlChanged;
                    if (xml.Length > 0)
                    {
                        parts.Add(xml);
                        hasMeaningfulPayload = true;
                    }

                    break;
                case "XMP_":
                    byte[] xmp = CleanRawXmpBox(file, child, out bool xmpChanged);
                    changed |= xmpChanged;
                    if (xmp.Length > 0)
                    {
                        parts.Add(xmp);
                        hasMeaningfulPayload = true;
                    }

                    break;
                case "uuid":
                    byte[] uuid = CleanUuidBox(file, child, out bool uuidChanged);
                    changed |= uuidChanged;
                    if (uuid.Length > 0)
                    {
                        parts.Add(uuid);
                        hasMeaningfulPayload = true;
                    }

                    break;
                default:
                    changed = true;
                    break;
            }
        }

        if (!changed)
        {
            return Slice(file, meta);
        }

        return !hasMeaningfulPayload ? [] : BuildContainerBox("meta", parts);
    }

    // Cleans the sub-boxes of an 'ilst' item (in QuickTime 'mdta', iTunes 'mdir', 'covr', or '----'): strips embedded pictures, drops refused
    // embedded media, and marks the whole item to be dropped when a text/binary value holds location metadata or all 'data' sub-boxes were dropped.
    private static byte[] CleanIlstItemBody(byte[] file, BoxHeader item, out bool changed, out bool dropWholeItem)
    {
        changed = false;
        dropWholeItem = false;
        ReadOnlySpan<byte> body = file.AsSpan(item.Data, item.End - item.Data);
        if (body.Length < 8)
        {
            if (PayloadMentionsPlace(body))
            {
                dropWholeItem = true;
                return [];
            }

            return body.ToArray();
        }

        List<BoxHeader> subs = [];
        int scan = item.Data;
        bool structured = true;
        while (scan < item.End)
        {
            if (item.End - scan == 4 && BinaryPrimitives.ReadUInt32BigEndian(file.AsSpan(scan, 4)) == 0)
            {
                break;
            }

            if (scan + 8 > item.End)
            {
                structured = false;
                break;
            }

            uint subSize = BinaryPrimitives.ReadUInt32BigEndian(file.AsSpan(scan, 4));
            if (subSize < 8 || scan + (long)subSize > item.End || !IsBoxType(file.AsSpan(scan + 4, 4)))
            {
                structured = false;
                break;
            }

            subs.Add(new BoxHeader(scan, scan + 8, scan + (int)subSize, Encoding.Latin1.GetString(file.AsSpan(scan + 4, 4)), null));
            scan += (int)subSize;
        }

        if (!structured || subs.Count == 0)
        {
            if (PayloadMentionsPlace(body))
            {
                dropWholeItem = true;
                return [];
            }

            return body.ToArray();
        }

        List<byte[]> kept = new(subs.Count);
        bool sawData = false;
        int keptDataCount = 0;

        foreach (BoxHeader sub in subs)
        {
            if (sub.Kind is "free" or "skip")
            {
                kept.Add(CleanFreeOrSkipBox(file, sub, out bool subFreeChanged));
                changed |= subFreeChanged;
                continue;
            }

            if (sub.Kind != "data")
            {
                ReadOnlySpan<byte> subPayload = file.AsSpan(sub.Data, sub.End - sub.Data);
                if (sub.Kind is not ("mean" or "name") && PayloadMentionsPlace(subPayload))
                {
                    dropWholeItem = true;
                    return [];
                }

                kept.Add(Slice(file, sub));
                continue;
            }

            sawData = true;
            if (sub.End - sub.Data < 8)
            {
                if (PayloadMentionsPlace(file.AsSpan(sub.Data, sub.End - sub.Data)))
                {
                    dropWholeItem = true;
                    return [];
                }

                kept.Add(Slice(file, sub));
                keptDataCount++;
                continue;
            }

            uint dataType = BinaryPrimitives.ReadUInt32BigEndian(file.AsSpan(sub.Data, 4)) & 0x00FFFFFFu;
            ReadOnlySpan<byte> rawValue = file.AsSpan(sub.Data + 8, sub.End - sub.Data - 8);
            ReadOnlySpan<byte> head = rawValue[..Math.Min(rawValue.Length, PlaceKinds.Head)];
            bool isEmbeddedMedia = dataType is 13 or 14 or 27
                || PlaceKinds.KindOf(head) != PictureKind.NotAPicture
                || PlaceKinds.VideoOf(rawValue) != VideoKind.NotAVideo;

            if (isEmbeddedMedia)
            {
                string mediaName = dataType == 14 ? "embedded.png" : dataType == 27 ? "embedded.bmp" : "embedded.jpg";
                PlaceResult mediaResult = TakeOut(rawValue.ToArray(), mediaName);
                if (mediaResult.Outcome == PlaceOutcome.Refused || mediaResult.Bytes is not { } cleanMedia)
                {
                    changed = true;
                    continue;
                }

                if (mediaResult.Outcome == PlaceOutcome.Removed)
                {
                    byte[] rebuiltData = new byte[8 + cleanMedia.Length];
                    file.AsSpan(sub.Data, 8).CopyTo(rebuiltData);
                    cleanMedia.CopyTo(rebuiltData.AsSpan(8));
                    kept.Add(BuildBox("data", rebuiltData));
                    changed = true;
                }
                else
                {
                    kept.Add(Slice(file, sub));
                }

                keptDataCount++;
                continue;
            }

            if (DataValueMentionsPlace(rawValue, dataType))
            {
                dropWholeItem = true;
                return [];
            }

            kept.Add(Slice(file, sub));
            keptDataCount++;
        }

        if (sawData && keptDataCount == 0)
        {
            dropWholeItem = true;
            return [];
        }

        if (!changed)
        {
            return body.ToArray();
        }

        int totalLen = kept.Sum(part => part.Length);
        byte[] output = new byte[totalLen];
        int cursor = 0;
        foreach (byte[] part in kept)
        {
            part.CopyTo(output.AsSpan(cursor));
            cursor += part.Length;
        }

        return output;
    }

    private static List<(string Namespace, string Value, byte[] Raw)> ParseMdtaKeys(byte[] file, BoxHeader keysBox)
    {
        ReadOnlySpan<byte> span = file.AsSpan(keysBox.Data, keysBox.End - keysBox.Data);
        if (span.Length < 8 || span[0] != 0)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        uint count = BinaryPrimitives.ReadUInt32BigEndian(span.Slice(4, 4));
        if (count > 100_000)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        List<(string Namespace, string Value, byte[] Raw)> keys = new((int)count);
        int at = 8;
        for (uint i = 0; i < count; i++)
        {
            if (at + 8 > span.Length)
            {
                throw Doubted(Doubt.VideoUnreadable);
            }

            uint keySize = BinaryPrimitives.ReadUInt32BigEndian(span.Slice(at, 4));
            if (keySize < 8 || at + (long)keySize > span.Length)
            {
                throw Doubted(Doubt.VideoUnreadable);
            }

            string ns = Encoding.Latin1.GetString(span.Slice(at + 4, 4));
            string value = Encoding.UTF8.GetString(span.Slice(at + 8, (int)keySize - 8));
            byte[] raw = span.Slice(at, (int)keySize).ToArray();
            keys.Add((ns, value, raw));
            at += (int)keySize;
        }

        if (at != span.Length)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        return keys;
    }

    private static byte[] BuildMdtaKeysBox(List<byte[]> keptKeys)
    {
        int payloadLen = 8 + keptKeys.Sum(k => k.Length);
        byte[] payload = new byte[payloadLen];
        BinaryPrimitives.WriteUInt32BigEndian(payload.AsSpan(4, 4), (uint)keptKeys.Count);
        int at = 8;
        foreach (byte[] key in keptKeys)
        {
            key.CopyTo(payload.AsSpan(at));
            at += key.Length;
        }

        return BuildBox("keys", payload);
    }

    private static bool ReverseDnsHeaderNamesPlace(byte[] file, BoxHeader item)
    {
        List<BoxHeader> subs = ReadBoxes(file, item.Data, item.End, topLevel: false, allowZeroTerminator: false, allowRawTypes: false);
        string mean = string.Empty;
        string name = string.Empty;
        foreach (BoxHeader sub in subs)
        {
            if (sub.Kind == "mean" && sub.End - sub.Data >= 4)
            {
                mean = Encoding.UTF8.GetString(file.AsSpan(sub.Data + 4, sub.End - sub.Data - 4));
            }
            else if (sub.Kind == "name" && sub.End - sub.Data >= 4)
            {
                name = Encoding.UTF8.GetString(file.AsSpan(sub.Data + 4, sub.End - sub.Data - 4));
            }
        }

        return IsVideoPlaceKey(mean, name);
    }

    private static byte[] CleanFullXmlBox(byte[] file, BoxHeader xmlBox, out bool changed)
    {
        if (xmlBox.End - xmlBox.Data < 4)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        ReadOnlySpan<byte> packet = file.AsSpan(xmlBox.Data + 4, xmlBox.End - xmlBox.Data - 4).TrimEnd((byte)0);
        if (packet.Contains((byte)0))
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        if (HoldsPlace(packet))
        {
            changed = true;
            return [];
        }

        changed = false;
        return Slice(file, xmlBox);
    }

    private static byte[] CleanRawXmpBox(byte[] file, BoxHeader xmpBox, out bool changed)
    {
        ReadOnlySpan<byte> packet = file.AsSpan(xmpBox.Data, xmpBox.End - xmpBox.Data).TrimEnd((byte)0);
        if (packet.Contains((byte)0))
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        if (HoldsPlace(packet))
        {
            changed = true;
            return [];
        }

        changed = false;
        return Slice(file, xmpBox);
    }

    private static byte[] CleanUuidBox(byte[] file, BoxHeader box, out bool changed)
    {
        ReadOnlySpan<byte> uuid = box.Uuid;
        ReadOnlySpan<byte> payload = file.AsSpan(box.Data, box.End - box.Data);

        if (uuid.SequenceEqual(XmpBoxUuid))
        {
            ReadOnlySpan<byte> trimmed = payload.TrimEnd((byte)0);
            if (trimmed.Contains((byte)0))
            {
                throw Doubted(Doubt.VideoUnreadable);
            }

            if (HoldsPlace(trimmed))
            {
                changed = true;
                return [];
            }

            changed = false;
            return Slice(file, box);
        }

        if (uuid.SequenceEqual(CanonExifUuid) || PayloadMentionsPlace(payload) || LooksLikeEmbeddedExif(payload))
        {
            changed = true;
            return [];
        }

        // Keep clean UTF-8 XML extension boxes (such as spherical projection metadata) that hold no place; drop unknown binary user-extension boxes.
        ReadOnlySpan<byte> text = payload.TrimStart(" \t\r\n"u8).TrimEnd((byte)0);
        if (text.StartsWith("<"u8) && !text.Contains((byte)0) && IsValidUtf8(text) && !HoldsPlace(text))
        {
            changed = false;
            return Slice(file, box);
        }

        changed = true;
        return [];
    }

    private static void AdjustChunkOffsetsInMoov(byte[] moovBytes, List<MdatRange> mdats, int[] newStarts)
    {
        if (moovBytes.Length == 0)
        {
            return;
        }

        BoxHeader root = ReadSingleBox(moovBytes, 0, moovBytes.Length, topLevel: true, allowRawTypes: false);
        AdjustOffsetsRecursive(moovBytes, root.Data, root.End, mdats, newStarts);
    }

    private static void AdjustOffsetsRecursive(byte[] moovBytes, int start, int end, List<MdatRange> mdats, int[] newStarts)
    {
        List<BoxHeader> children = ReadBoxes(moovBytes, start, end, topLevel: false, allowZeroTerminator: true, allowRawTypes: false);
        foreach (BoxHeader child in children)
        {
            switch (child.Kind)
            {
                case "trak" or "mdia" or "minf" or "stbl":
                    AdjustOffsetsRecursive(moovBytes, child.Data, child.End, mdats, newStarts);
                    break;
                case "stco":
                    AdjustStco(moovBytes, child, mdats, newStarts);
                    break;
                case "co64":
                    AdjustCo64(moovBytes, child, mdats, newStarts);
                    break;
                case "saio":
                    AdjustSaio(moovBytes, child, mdats, newStarts);
                    break;
                default:
                    break;
            }
        }
    }

    private static void ValidateSaio(byte[] file, BoxHeader saio)
    {
        int length = saio.End - saio.Data;
        if (length < 8)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        byte version = file[saio.Data];
        if (version > 1)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        uint flags = BinaryPrimitives.ReadUInt32BigEndian(file.AsSpan(saio.Data, 4)) & 0x00FFFFFFu;
        int pos = saio.Data + 4 + ((flags & 1u) != 0 ? 8 : 0);
        if (pos + 4 > saio.End)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        uint count = BinaryPrimitives.ReadUInt32BigEndian(file.AsSpan(pos, 4));
        pos += 4;
        long entrySize = version == 0 ? 4L : 8L;
        if (pos + (entrySize * count) != saio.End)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }
    }

    private static void AdjustSaio(byte[] moovBytes, BoxHeader saio, List<MdatRange> mdats, int[] newStarts)
    {
        ValidateSaio(moovBytes, saio);
        byte version = moovBytes[saio.Data];
        uint flags = BinaryPrimitives.ReadUInt32BigEndian(moovBytes.AsSpan(saio.Data, 4)) & 0x00FFFFFFu;
        int pos = saio.Data + 4 + ((flags & 1u) != 0 ? 8 : 0);
        uint count = BinaryPrimitives.ReadUInt32BigEndian(moovBytes.AsSpan(pos, 4));
        pos += 4;

        if (version == 0)
        {
            for (uint i = 0; i < count; i++, pos += 4)
            {
                long oldOffset = BinaryPrimitives.ReadUInt32BigEndian(moovBytes.AsSpan(pos, 4));
                long shifted = ShiftMdatOffset(oldOffset, mdats, newStarts);
                if (shifted is < 0 or > uint.MaxValue)
                {
                    throw Doubted(Doubt.VideoSaid);
                }

                BinaryPrimitives.WriteUInt32BigEndian(moovBytes.AsSpan(pos, 4), (uint)shifted);
            }
        }
        else
        {
            for (uint i = 0; i < count; i++, pos += 8)
            {
                ulong raw = BinaryPrimitives.ReadUInt64BigEndian(moovBytes.AsSpan(pos, 8));
                if (raw > long.MaxValue)
                {
                    throw Doubted(Doubt.VideoSaid);
                }

                long shifted = ShiftMdatOffset((long)raw, mdats, newStarts);
                if (shifted < 0)
                {
                    throw Doubted(Doubt.VideoSaid);
                }

                BinaryPrimitives.WriteUInt64BigEndian(moovBytes.AsSpan(pos, 8), (ulong)shifted);
            }
        }
    }

    private static void AdjustStco(byte[] moovBytes, BoxHeader stco, List<MdatRange> mdats, int[] newStarts)
    {
        int length = stco.End - stco.Data;
        if (length < 8 || moovBytes[stco.Data] != 0)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        uint count = BinaryPrimitives.ReadUInt32BigEndian(moovBytes.AsSpan(stco.Data + 4, 4));
        if (8L + (4L * count) != length)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        int at = stco.Data + 8;
        for (uint i = 0; i < count; i++, at += 4)
        {
            long oldOffset = BinaryPrimitives.ReadUInt32BigEndian(moovBytes.AsSpan(at, 4));
            long shifted = ShiftMdatOffset(oldOffset, mdats, newStarts);
            if (shifted is < 0 or > uint.MaxValue)
            {
                throw Doubted(Doubt.VideoSaid);
            }

            BinaryPrimitives.WriteUInt32BigEndian(moovBytes.AsSpan(at, 4), (uint)shifted);
        }
    }

    private static void AdjustCo64(byte[] moovBytes, BoxHeader co64, List<MdatRange> mdats, int[] newStarts)
    {
        int length = co64.End - co64.Data;
        if (length < 8 || moovBytes[co64.Data] != 0)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        uint count = BinaryPrimitives.ReadUInt32BigEndian(moovBytes.AsSpan(co64.Data + 4, 4));
        if (8L + (8L * count) != length)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        int at = co64.Data + 8;
        for (uint i = 0; i < count; i++, at += 8)
        {
            ulong raw = BinaryPrimitives.ReadUInt64BigEndian(moovBytes.AsSpan(at, 8));
            if (raw > long.MaxValue)
            {
                throw Doubted(Doubt.VideoSaid);
            }

            long shifted = ShiftMdatOffset((long)raw, mdats, newStarts);
            if (shifted < 0)
            {
                throw Doubted(Doubt.VideoSaid);
            }

            BinaryPrimitives.WriteUInt64BigEndian(moovBytes.AsSpan(at, 8), (ulong)shifted);
        }
    }

    private static long ShiftMdatOffset(long offset, List<MdatRange> mdats, int[] newStarts)
    {
        foreach (MdatRange mdat in mdats)
        {
            if (offset >= mdat.OldStart && offset <= mdat.OldEnd)
            {
                long delta = (long)newStarts[mdat.Index] - mdat.OldStart;
                return offset + delta;
            }
        }

        throw Doubted(Doubt.VideoSaid);
    }

    private static byte[] CleanFreeOrSkipBox(byte[] file, BoxHeader box, out bool changed)
    {
        ReadOnlySpan<byte> payload = file.AsSpan(box.Data, box.End - box.Data);
        if (!payload.ContainsAnyExcept((byte)0))
        {
            changed = false;
            return Slice(file, box);
        }

        changed = true;
        byte[] zeroed = new byte[box.End - box.Start];
        file.AsSpan(box.Start, box.Data - box.Start).CopyTo(zeroed);
        return zeroed;
    }

    private static string ReadHandlerType(byte[] file, BoxHeader hdlr) => ReadHandlerInfo(file, hdlr).Subtype;

    private static (string Subtype, byte[] NameBytes, string Name) ReadHandlerInfo(byte[] file, BoxHeader hdlr)
    {
        if (hdlr.End - hdlr.Data < 12)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        string subtype = Encoding.Latin1.GetString(file.AsSpan(hdlr.Data + 8, 4));
        if (hdlr.End - hdlr.Data <= 24)
        {
            return (subtype, [], string.Empty);
        }

        ReadOnlySpan<byte> rawName = file.AsSpan(hdlr.Data + 24, hdlr.End - hdlr.Data - 24);
        if (rawName.Length >= 2 && rawName[0] == rawName.Length - 1)
        {
            rawName = rawName[1..];
        }

        rawName = rawName.TrimEnd((byte)0);
        return (subtype, rawName.ToArray(), Encoding.UTF8.GetString(rawName));
    }

    private static bool IsVideoPlaceKey(string ns, string name)
    {
        string combined = (ns + "." + name).ToLowerInvariant();
        if (combined.Contains("location", StringComparison.Ordinal)
            || combined.Contains("iso6709", StringComparison.Ordinal)
            || combined.Contains("gps", StringComparison.Ordinal)
            || combined.Contains("latitude", StringComparison.Ordinal)
            || combined.Contains("longitude", StringComparison.Ordinal)
            || combined.Contains("altitude", StringComparison.Ordinal)
            || combined.Contains("coord", StringComparison.Ordinal))
        {
            return true;
        }

        foreach (string part in name.Split(['.', '/', '_', '-', ':', ' '], StringSplitOptions.RemoveEmptyEntries))
        {
            if (IsPlace(part.ToLowerInvariant()))
            {
                return true;
            }
        }

        return false;
    }

    private static bool IsVideoPlaceAtomType(string kind)
    {
        string trimmed = (kind.Length > 0 && kind[0] == '\u00A9' ? kind[1..] : kind).Trim().ToLowerInvariant();
        return trimmed is "xyz" or "loc" or "loci" or "gps" || IsPlace(trimmed);
    }

    private static bool DataValueMentionsPlace(ReadOnlySpan<byte> payload, uint dataType)
    {
        if (PayloadMentionsPlace(payload))
        {
            return true;
        }

        if (dataType is 2 or 4 or 5 && payload.Length >= 2)
        {
            byte[] utf8 = DecodeUtf16ToUtf8(payload, bigEndian: true);
            if (LooksLikeIso6709(utf8) || HoldsPlace(utf8) || ContainsGpsCallout(utf8))
            {
                return true;
            }
        }

        return false;
    }

    private static bool PayloadMentionsPlace(ReadOnlySpan<byte> payload) =>
        LooksLikeIso6709(payload) || LooksLikeEmbeddedExif(payload) || HoldsPlace(payload) || Utf16MentionsPlace(payload);

    private static bool Utf16MentionsPlace(ReadOnlySpan<byte> payload)
    {
        // Check UTF-16 BOMs at common box offsets (0 for raw strings, 4 after QuickTime string header, 6 after 3GPP version/flags+language).
        foreach (int offset in (ReadOnlySpan<int>)[0, 4, 6])
        {
            if (offset + 6 <= payload.Length)
            {
                ReadOnlySpan<byte> slice = payload[offset..];
                if (slice[0] == 0xFE && slice[1] == 0xFF)
                {
                    byte[] utf8 = DecodeUtf16ToUtf8(slice[2..], bigEndian: true);
                    if (LooksLikeIso6709(utf8) || HoldsPlace(utf8) || ContainsGpsCallout(utf8))
                    {
                        return true;
                    }
                }
                else if (slice[0] == 0xFF && slice[1] == 0xFE)
                {
                    byte[] utf8 = DecodeUtf16ToUtf8(slice[2..], bigEndian: false);
                    if (LooksLikeIso6709(utf8) || HoldsPlace(utf8) || ContainsGpsCallout(utf8))
                    {
                        return true;
                    }
                }
            }
        }

        // Also detect BOM-less UTF-16BE / UTF-16LE ASCII runs (for example '+30.9010+075.8573/' with interleaved NUL bytes).
        if (payload.Length >= 16 && payload.Length <= 65536)
        {
            foreach (int offset in (ReadOnlySpan<int>)[0, 4, 6])
            {
                if (offset + 16 > payload.Length)
                {
                    continue;
                }

                ReadOnlySpan<byte> slice = payload[offset..];
                if (LooksLikeInterleavedAscii(slice, nulEven: true))
                {
                    byte[] utf8 = DecodeUtf16ToUtf8(slice, bigEndian: true);
                    if (LooksLikeIso6709(utf8) || HoldsPlace(utf8) || ContainsGpsCallout(utf8))
                    {
                        return true;
                    }
                }
                else if (LooksLikeInterleavedAscii(slice, nulEven: false))
                {
                    byte[] utf8 = DecodeUtf16ToUtf8(slice, bigEndian: false);
                    if (LooksLikeIso6709(utf8) || HoldsPlace(utf8) || ContainsGpsCallout(utf8))
                    {
                        return true;
                    }
                }
            }
        }

        return false;
    }

    private static bool LooksLikeInterleavedAscii(ReadOnlySpan<byte> bytes, bool nulEven)
    {
        int pairs = Math.Min(bytes.Length / 2, 16);
        if (pairs < 6)
        {
            return false;
        }

        int asciiPairs = 0;
        for (int i = 0; i < pairs; i++)
        {
            byte nulByte = bytes[(i * 2) + (nulEven ? 0 : 1)];
            byte charByte = bytes[(i * 2) + (nulEven ? 1 : 0)];
            if (nulByte == 0 && charByte is >= 0x20 and <= 0x7E)
            {
                asciiPairs++;
            }
        }

        return asciiPairs >= 6;
    }

    private static byte[] DecodeUtf16ToUtf8(ReadOnlySpan<byte> bytes, bool bigEndian)
    {
        int evenLen = bytes.Length & ~1;
        if (evenLen == 0)
        {
            return [];
        }

        Encoding utf16 = bigEndian ? Encoding.BigEndianUnicode : Encoding.Unicode;
        return Encoding.UTF8.GetBytes(utf16.GetString(bytes[..evenLen]));
    }

    private static bool LooksLikeEmbeddedExif(ReadOnlySpan<byte> payload) =>
        payload.IndexOf("Exif\0\0"u8) >= 0
        || payload.IndexOf("II*\0"u8) >= 0
        || payload.IndexOf("MM\0*"u8) >= 0
        || payload.IndexOf("GPMF"u8) >= 0;

    private static bool ContainsGpsCallout(ReadOnlySpan<byte> payload) =>
        IndexOfAsciiIgnoreCase(payload, "gps("u8) >= 0
        || IndexOfAsciiIgnoreCase(payload, "latitude"u8) >= 0
        || IndexOfAsciiIgnoreCase(payload, "longitude"u8) >= 0
        || IndexOfAsciiIgnoreCase(payload, "$gprmc"u8) >= 0
        || IndexOfAsciiIgnoreCase(payload, "$gpgga"u8) >= 0
        || IndexOfAsciiIgnoreCase(payload, "$gnrmc"u8) >= 0
        || IndexOfAsciiIgnoreCase(payload, "$gngga"u8) >= 0
        || IndexOfAsciiIgnoreCase(payload, "$gpgll"u8) >= 0;

    // Recognises ISO 6709 coordinate strings such as "+30.9010+075.8573/" or "-33.8688+151.2093+012.3/" (including after a 4-byte QuickTime string header).
    private static bool LooksLikeIso6709(ReadOnlySpan<byte> bytes)
    {
        for (int start = 0; start + 8 <= bytes.Length; start++)
        {
            if (bytes[start] is not ((byte)'+' or (byte)'-'))
            {
                continue;
            }

            if (start > 0 && IsAsciiDigitOrLetter(bytes[start - 1]))
            {
                continue;
            }

            int pos = start + 1;
            int latInt = CountDigits(bytes, pos);
            if (latInt is not (2 or 4 or 6))
            {
                continue;
            }

            int latDeg = ((bytes[pos] - '0') * 10) + (bytes[pos + 1] - '0');
            if (latDeg > 90)
            {
                continue;
            }

            pos += latInt;
            if (pos < bytes.Length && bytes[pos] == (byte)'.')
            {
                pos++;
                int frac = CountDigits(bytes, pos);
                if (frac == 0)
                {
                    continue;
                }

                pos += frac;
            }

            if (pos >= bytes.Length || bytes[pos] is not ((byte)'+' or (byte)'-'))
            {
                continue;
            }

            pos++;
            int lonInt = CountDigits(bytes, pos);
            if (lonInt is not (3 or 5 or 7))
            {
                continue;
            }

            int lonDeg = ((bytes[pos] - '0') * 100) + ((bytes[pos + 1] - '0') * 10) + (bytes[pos + 2] - '0');
            if (lonDeg > 180)
            {
                continue;
            }

            pos += lonInt;
            if (pos < bytes.Length && bytes[pos] == (byte)'.')
            {
                pos++;
                int frac = CountDigits(bytes, pos);
                if (frac == 0)
                {
                    continue;
                }

                pos += frac;
            }

            if (pos == bytes.Length || bytes[pos] is (byte)'/' or (byte)'+' or (byte)'-' or (byte)'C' or 0 or (byte)' ' or (byte)'\r' or (byte)'\n')
            {
                return true;
            }
        }

        return false;
    }

    private static int CountDigits(ReadOnlySpan<byte> bytes, int from)
    {
        int count = 0;
        while (from + count < bytes.Length && bytes[from + count] is >= (byte)'0' and <= (byte)'9')
        {
            count++;
        }

        return count;
    }

    private static bool IsAsciiDigitOrLetter(byte value) =>
        value is (>= (byte)'0' and <= (byte)'9') or (>= (byte)'A' and <= (byte)'Z') or (>= (byte)'a' and <= (byte)'z');

    private static int IndexOfAsciiIgnoreCase(ReadOnlySpan<byte> haystack, ReadOnlySpan<byte> lowerNeedle)
    {
        for (int i = 0; i + lowerNeedle.Length <= haystack.Length; i++)
        {
            int j = 0;
            while (j < lowerNeedle.Length && (byte)(haystack[i + j] | 0x20) == lowerNeedle[j])
            {
                j++;
            }

            if (j == lowerNeedle.Length)
            {
                return i;
            }
        }

        return -1;
    }

    private static bool IsValidUtf8(ReadOnlySpan<byte> bytes)
    {
        try
        {
            _ = new UTF8Encoding(encoderShouldEmitUTF8Identifier: false, throwOnInvalidBytes: true).GetCharCount(bytes);
            return true;
        }
        catch (DecoderFallbackException)
        {
            return false;
        }
    }

    private static List<BoxHeader> ReadBoxes(byte[] file, int start, int end, bool topLevel, bool allowZeroTerminator, bool allowRawTypes)
    {
        List<BoxHeader> list = [];
        int at = start;
        while (at < end)
        {
            if (allowZeroTerminator && end - at == 4 && BinaryPrimitives.ReadUInt32BigEndian(file.AsSpan(at, 4)) == 0)
            {
                break;
            }

            BoxHeader box = ReadSingleBox(file, at, end, topLevel, allowRawTypes);
            list.Add(box);
            at = box.End;
        }

        return list;
    }

    private static BoxHeader ReadSingleBox(byte[] file, int at, int end, bool topLevel, bool allowRawTypes)
    {
        if (at + 8 > end)
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        uint size32 = BinaryPrimitives.ReadUInt32BigEndian(file.AsSpan(at, 4));
        ReadOnlySpan<byte> typeBytes = file.AsSpan(at + 4, 4);
        if (!allowRawTypes && !IsBoxType(typeBytes))
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        string kind = Encoding.Latin1.GetString(typeBytes);
        int headerLen;
        long boxEnd;
        if (size32 == 1)
        {
            if (at + 16 > end)
            {
                throw Doubted(Doubt.VideoUnreadable);
            }

            ulong size64 = BinaryPrimitives.ReadUInt64BigEndian(file.AsSpan(at + 8, 8));
            if (size64 < 16 || size64 > (ulong)(end - at))
            {
                throw Doubted(Doubt.VideoUnreadable);
            }

            headerLen = 16;
            boxEnd = at + (long)size64;
        }
        else if (size32 == 0)
        {
            if (!topLevel)
            {
                throw Doubted(Doubt.VideoUnreadable);
            }

            headerLen = 8;
            boxEnd = end;
        }
        else if (size32 >= 8)
        {
            headerLen = 8;
            boxEnd = at + (long)size32;
            if (boxEnd > end)
            {
                throw Doubted(Doubt.VideoUnreadable);
            }
        }
        else
        {
            throw Doubted(Doubt.VideoUnreadable);
        }

        byte[]? uuid = null;
        int data = at + headerLen;
        if (kind == "uuid")
        {
            if (data + 16 > boxEnd)
            {
                throw Doubted(Doubt.VideoUnreadable);
            }

            uuid = file.AsSpan(data, 16).ToArray();
            data += 16;
        }

        return new BoxHeader(at, data, (int)boxEnd, kind, uuid);
    }

    private static bool IsBoxType(ReadOnlySpan<byte> typeBytes)
    {
        for (int i = 0; i < 4; i++)
        {
            byte b = typeBytes[i];
            if (b != 0xA9 && (b < 0x20 || b > 0x7E))
            {
                return false;
            }
        }

        return true;
    }

    private static byte[] Slice(byte[] file, BoxHeader box) => file.AsSpan(box.Start, box.End - box.Start).ToArray();

    private static byte[] BuildBox(string kind, ReadOnlySpan<byte> payload)
    {
        long total = 8L + payload.Length;
        if (total > int.MaxValue)
        {
            throw Doubted(Doubt.VideoSaid);
        }

        byte[] box = new byte[(int)total];
        BinaryPrimitives.WriteUInt32BigEndian(box.AsSpan(0, 4), (uint)total);
        Encoding.Latin1.GetBytes(kind).CopyTo(box.AsSpan(4, 4));
        payload.CopyTo(box.AsSpan(8));
        return box;
    }

    private static byte[] BuildContainerBox(string kind, List<byte[]> children)
    {
        long payloadLen = 0;
        foreach (byte[] child in children)
        {
            payloadLen += child.Length;
        }

        long total = 8L + payloadLen;
        if (total > int.MaxValue)
        {
            throw Doubted(Doubt.VideoSaid);
        }

        byte[] box = new byte[(int)total];
        BinaryPrimitives.WriteUInt32BigEndian(box.AsSpan(0, 4), (uint)total);
        Encoding.Latin1.GetBytes(kind).CopyTo(box.AsSpan(4, 4));
        int cursor = 8;
        foreach (byte[] child in children)
        {
            if (child.Length > 0)
            {
                child.CopyTo(box.AsSpan(cursor));
                cursor += child.Length;
            }
        }

        return box;
    }
}
