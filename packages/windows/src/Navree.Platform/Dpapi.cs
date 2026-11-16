using System.Runtime.InteropServices;

// Windows' own libraries are looked for in the system folder and nowhere else.
[assembly: DefaultDllImportSearchPaths(DllImportSearchPath.System32)]

namespace Navree.Platform;

/// <summary>
/// Windows' data protection (<c>CryptProtectData</c>) for the Windows user the app runs as. Microsoft's page for it says that typically only
/// a user with the same logon credentials can decrypt the data, usually only on the computer where it was protected, and that a user with a
/// roaming profile can decrypt it from another computer on the network. It does not keep programs apart: any program that runs as the same
/// Windows user can open what this one protected, given the entropy, which is a constant of the app and no secret. It exists on Windows only.
/// </summary>
public static unsafe partial class Dpapi
{
    private const uint UiForbidden = 0x1;

    /// <summary>Protects bytes for the Windows user the app runs as.</summary>
    /// <param name="plain">The bytes.</param>
    /// <param name="entropy">
    /// Bytes that must be given again to open them. The app's name: it keeps one app's store from being read as another's by mistake, and is no
    /// protection against a program that reads it from this one.
    /// </param>
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

    /// <summary>Opens bytes <see cref="Protect"/> protected, as the same Windows user.</summary>
    /// <param name="protectedBytes">The protected bytes.</param>
    /// <param name="entropy">The bytes given to <see cref="Protect"/>.</param>
    /// <returns>The bytes.</returns>
    /// <exception cref="InvalidOperationException">They cannot be opened here, as this Windows user, with this entropy.</exception>
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
