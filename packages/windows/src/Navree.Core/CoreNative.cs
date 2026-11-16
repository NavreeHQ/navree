using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

// The core's library is looked for beside this assembly and nowhere else.
[assembly: DefaultDllImportSearchPaths(DllImportSearchPath.AssemblyDirectory)]

namespace Navree.Core;

/// <summary>Bytes the core owns: <c>Length</c> bytes of UTF-8 JSON at <c>Pointer</c>.</summary>
[StructLayout(LayoutKind.Sequential)]
internal unsafe struct NavreeBytes
{
    /// <summary>Where the bytes are; null for an empty answer.</summary>
    public byte* Pointer;

    /// <summary>How many bytes there are.</summary>
    public nuint Length;
}

/// <summary>
/// The core's C interface, <c>navree_core.h</c>: five functions that take and
/// give UTF-8 JSON as a pointer and a length. Nothing outside this file calls
/// them; <see cref="NavreeCore"/> is the way in.
/// </summary>
internal static unsafe partial class CoreNative
{
    private const string Library = "navree_native_core";

    /// <summary>Makes a core from a configuration.</summary>
    /// <exception cref="ArgumentException">The configuration does not read.</exception>
    internal static CoreHandle Create(ReadOnlySpan<byte> config)
    {
        nint raw;
        fixed (byte* bytes = config)
        {
            raw = CoreNew(bytes, (nuint)config.Length);
        }

        if (raw == 0)
        {
            throw new ArgumentException("The core's configuration does not read.", nameof(config));
        }

        return new CoreHandle(raw);
    }

    /// <summary>Tells the core one event and takes what it answers.</summary>
    internal static byte[] Send(CoreHandle core, ReadOnlySpan<byte> eventJson)
    {
        fixed (byte* bytes = eventJson)
        {
            return Take(CoreSend(core, bytes, (nuint)eventJson.Length));
        }
    }

    /// <summary>Reads a part of the view state; an empty query reads all of it.</summary>
    internal static byte[] View(CoreHandle core, ReadOnlySpan<byte> query)
    {
        fixed (byte* bytes = query)
        {
            return Take(CoreView(core, bytes, (nuint)query.Length));
        }
    }

    /// <summary>Ends a core. Called once, by the handle.</summary>
    internal static void Free(nint core) => CoreFree(core);

    // Copies what the core gave into memory of ours and gives it back to the
    // core's own allocator, which is the only one that may free it.
    private static byte[] Take(NavreeBytes bytes)
    {
        try
        {
            return bytes.Pointer == null
                ? Array.Empty<byte>()
                : new ReadOnlySpan<byte>(bytes.Pointer, checked((int)bytes.Length)).ToArray();
        }
        finally
        {
            BytesFree(bytes);
        }
    }

    [LibraryImport(Library, EntryPoint = "navree_core_new")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    private static partial nint CoreNew(byte* config, nuint length);

    [LibraryImport(Library, EntryPoint = "navree_core_send")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    private static partial NavreeBytes CoreSend(CoreHandle core, byte* eventJson, nuint length);

    [LibraryImport(Library, EntryPoint = "navree_core_view")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    private static partial NavreeBytes CoreView(CoreHandle core, byte* query, nuint length);

    [LibraryImport(Library, EntryPoint = "navree_bytes_free")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    private static partial void BytesFree(NavreeBytes bytes);

    [LibraryImport(Library, EntryPoint = "navree_core_free")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    private static partial void CoreFree(nint core);
}
