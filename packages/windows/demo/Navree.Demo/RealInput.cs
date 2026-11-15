using System.Runtime.InteropServices;

namespace Navree.Demo;

/// <summary>A real pointer and real keys, sent to the system as a hand would, so what the drawings do with them is seen and not assumed.</summary>
internal static class RealInput
{
    private const uint MouseMove = 0x0001;
    private const uint KeyUp = 0x0002;

    [StructLayout(LayoutKind.Sequential)]
    private struct Spot
    {
        public int X;
        public int Y;
    }

    [DllImport("user32.dll")]
    private static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll")]
    private static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);

    [DllImport("user32.dll")]
    private static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);

    [DllImport("user32.dll")]
    private static extern bool ClientToScreen(IntPtr window, ref Spot spot);

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr window);

    /// <summary>Brings the window forward, so input reaches it.</summary>
    internal static void Forward(IntPtr window) => SetForegroundWindow(window);

    /// <summary>Moves the pointer to a place in the window content, in pixels from its top left, and nudges it so the window sees it move.</summary>
    internal static void PointerTo(IntPtr window, double x, double y)
    {
        Spot origin = default;
        ClientToScreen(window, ref origin);
        SetCursorPos(origin.X + (int)Math.Round(x), origin.Y + (int)Math.Round(y));
        mouse_event(MouseMove, 1, 0, 0, UIntPtr.Zero);
        mouse_event(MouseMove, -1, 0, 0, UIntPtr.Zero);
    }

    /// <summary>Presses and lets go of a key (a virtual key code).</summary>
    internal static void Press(byte key)
    {
        keybd_event(key, 0, 0, UIntPtr.Zero);
        keybd_event(key, 0, KeyUp, UIntPtr.Zero);
    }
}
