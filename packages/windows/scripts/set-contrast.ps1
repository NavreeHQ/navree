# Turns the system's high contrast colours on or off for the session, so that a picture can be taken
# in them, and says whether the system took it.
#
#   powershell -File set-contrast.ps1 -On      (High Contrast Black)
#   powershell -File set-contrast.ps1 -Off
param(
    [switch]$On,
    [switch]$Off,
    [string]$Scheme = 'High Contrast Black'
)

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class Contrast
{
    [StructLayout(LayoutKind.Sequential)]
    private struct HighContrastInfo
    {
        public int Size;
        public int Flags;
        public IntPtr Scheme;
    }

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool SystemParametersInfo(int action, int size, ref HighContrastInfo info, int flags);

    public static string Set(bool on, string scheme)
    {
        HighContrastInfo info = new HighContrastInfo();
        info.Size = Marshal.SizeOf(typeof(HighContrastInfo));
        info.Flags = on ? 3 : 2;
        info.Scheme = Marshal.StringToHGlobalUni(scheme);
        bool done = SystemParametersInfo(0x43, info.Size, ref info, 3);
        string result = done ? "the system took it" : "the system refused: " + Marshal.GetLastWin32Error();
        Marshal.FreeHGlobal(info.Scheme);
        return result;
    }

    public static bool IsOn()
    {
        HighContrastInfo info = new HighContrastInfo();
        info.Size = Marshal.SizeOf(typeof(HighContrastInfo));
        bool read = SystemParametersInfo(0x42, info.Size, ref info, 0);
        return read && (info.Flags & 1) == 1;
    }
}
'@

if ($On) {
    Write-Host ('high contrast on: ' + [Contrast]::Set($true, $Scheme))
} elseif ($Off) {
    Write-Host ('high contrast off: ' + [Contrast]::Set($false, $Scheme))
}
Start-Sleep -Seconds 3
Write-Host ('high contrast is on: ' + [Contrast]::IsOn())
