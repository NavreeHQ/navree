using System.Runtime.InteropServices;

// Windows' own libraries are looked for in the system folder and nowhere else.
[assembly: DefaultDllImportSearchPaths(DllImportSearchPath.System32)]

namespace Navree.Platform;

/// <summary>
/// Windows' data protection for the person signed in to Windows: what is protected can be opened only by this
/// person, on this device. Nothing about it roams or syncs; a copy of the file on another device or for another
/// person is bytes that mean nothing. It exists on Windows only.
/// </summary>
public static unsafe partial class Dpapi
{
    private const uint UiForbidden = 0x1;

    /// <summary>Protects bytes for the current person on this device.</summary>
    /// <param name="plain">The bytes.</param>
    /// <param name="entropy">Bytes that must be given again to open them: the app's name, so another app cannot.</param>
    /// <returns>The protected bytes.</returns>
    /// <exception cref="InvalidOperationException">The system refused.</exception>
    public static byte[] Protect(ReadOnlySpan<byte> plain, ReadOnlySpan<byte> entropy)
    {
        fixed (byte* input = plain)
        fixed (byte* extra = entropy)
        {
            DataBlob inBlob = new() { Size = (uint)plain.Length, Data = input };
            DataBlob extraBlob = new() { Size = (uint)entropy.Length, Data = extra };
            DataBlob outBlob = default;
            if (!CryptProtectData(&inBlob, null, &extraBlob, 0, 0, UiForbidden, &outBlob))
            {
                throw new InvalidOperationException("Windows would not protect the data (error " + Marshal.GetLastPInvokeError() + ").");
            }

            return Take(outBlob);
        }
    }

    /// <summary>Opens bytes <see cref="Protect"/> protected, for the same person on the same device.</summary>
    /// <param name="protectedBytes">The protected bytes.</param>
    /// <param name="entropy">The bytes given to <see cref="Protect"/>.</param>
    /// <returns>The bytes.</returns>
    /// <exception cref="InvalidOperationException">They cannot be opened here, by this person, with this entropy.</exception>
    public static byte[] Unprotect(ReadOnlySpan<byte> protectedBytes, ReadOnlySpan<byte> entropy)
    {
        fixed (byte* input = protectedBytes)
        fixed (byte* extra = entropy)
        {
            DataBlob inBlob = new() { Size = (uint)protectedBytes.Length, Data = input };
            DataBlob extraBlob = new() { Size = (uint)entropy.Length, Data = extra };
            DataBlob outBlob = default;
            if (!CryptUnprotectData(&inBlob, null, &extraBlob, 0, 0, UiForbidden, &outBlob))
            {
                throw new InvalidOperationException("Windows would not open the data (error " + Marshal.GetLastPInvokeError() + ").");
            }

            return Take(outBlob);
        }
    }

    // Copies what the system gave into memory of ours and gives it back to the system, which allocated it.
    private static byte[] Take(DataBlob blob)
    {
        try
        {
            return new ReadOnlySpan<byte>(blob.Data, checked((int)blob.Size)).ToArray();
        }
        finally
        {
            _ = LocalFree(blob.Data);
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct DataBlob
    {
        public uint Size;
        public byte* Data;
    }

    [LibraryImport("crypt32.dll", EntryPoint = "CryptProtectData", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool CryptProtectData(DataBlob* data, char* description, DataBlob* entropy, nint reserved, nint prompt, uint flags, DataBlob* output);

    [LibraryImport("crypt32.dll", EntryPoint = "CryptUnprotectData", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool CryptUnprotectData(DataBlob* data, char* description, DataBlob* entropy, nint reserved, nint prompt, uint flags, DataBlob* output);

    [LibraryImport("kernel32.dll")]
    private static partial nint LocalFree(void* memory);
}
