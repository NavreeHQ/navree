using System.Runtime.InteropServices;

namespace Navree.Presence;

/// <summary>
/// Waits for a task on a thread that runs windows, the way the system asks of one: a wait that lets the thread answer the messages
/// and calls that are sent to it, so the call it waits on can finish. A plain wait on such a thread can stop both.
/// </summary>
public static unsafe partial class PumpingWait
{
    /// <summary>Waits until the task ends, however it ends.</summary>
    /// <param name="work">The task.</param>
    public static void For(Task work)
    {
        ArgumentNullException.ThrowIfNull(work);
        nint done = CreateEvent(0, 1, 0, 0);
        if (done == 0)
        {
            work.Wait();
            return;
        }

        try
        {
            _ = work.ContinueWith(_ => SetEvent(done), TaskScheduler.Default);
            nint* handles = stackalloc nint[1];
            handles[0] = done;
            _ = CoWaitForMultipleObjects(0, 0xFFFFFFFF, 1, handles, out _);
        }
        finally
        {
            _ = CloseHandle(done);
        }
    }

    [LibraryImport("kernel32.dll", EntryPoint = "CreateEventW")]
    private static partial nint CreateEvent(nint attributes, int manualReset, int initialState, nint name);

    [LibraryImport("kernel32.dll")]
    private static partial int SetEvent(nint handle);

    [LibraryImport("kernel32.dll")]
    private static partial int CloseHandle(nint handle);

    [LibraryImport("ole32.dll")]
    private static partial int CoWaitForMultipleObjects(uint flags, uint milliseconds, uint count, nint* handles, out uint index);
}

/// <summary>A picture of a part of the screen, as the system shows it: the windows, the taskbar, the notification area.</summary>
public static unsafe partial class ScreenCapture
{
    /// <summary>Copies a rectangle of the screen.</summary>
    /// <param name="area">The rectangle, in screen pixels.</param>
    /// <returns>Straight BGRA pixels, <c>width * height * 4</c> bytes, top row first; null when the system would not give it.</returns>
    public static byte[]? Grab(Box area)
    {
        int width = area.Width;
        int height = area.Height;
        if (width <= 0 || height <= 0 || width > 16384 || height > 16384)
        {
            return null;
        }

        nint screen = GetDC(0);
        if (screen == 0)
        {
            return null;
        }

        nint memory = CreateCompatibleDC(screen);
        nint bitmap = memory == 0 ? 0 : CreateCompatibleBitmap(screen, width, height);
        try
        {
            if (memory == 0 || bitmap == 0)
            {
                return null;
            }

            nint old = SelectObject(memory, bitmap);
            bool copied = BitBlt(memory, 0, 0, width, height, screen, area.Left, area.Top, 0x40CC0020) != 0;
            _ = SelectObject(memory, old);
            if (!copied)
            {
                return null;
            }

            byte[] pixels = new byte[width * height * 4];
            BitmapHeader header = new()
            {
                Size = (uint)sizeof(BitmapHeader),
                Width = width,
                Height = -height,
                Planes = 1,
                BitCount = 32,
            };
            fixed (byte* bits = pixels)
            {
                int lines = GetDIBits(memory, bitmap, 0, (uint)height, bits, ref header, 0);
                if (lines != height)
                {
                    return null;
                }
            }

            // The screen has no alpha: what it gives in that byte is nothing, so the picture is made opaque.
            for (int at = 3; at < pixels.Length; at += 4)
            {
                pixels[at] = 0xFF;
            }

            return pixels;
        }
        finally
        {
            if (bitmap != 0)
            {
                _ = DeleteObject(bitmap);
            }

            if (memory != 0)
            {
                _ = DeleteDC(memory);
            }

            _ = ReleaseDC(0, screen);
        }
    }

    /// <summary>The size of the screen all monitors make together, from its top left.</summary>
    /// <returns>The rectangle.</returns>
    public static Box Screen() => new(
        Native.GetSystemMetrics(76),
        Native.GetSystemMetrics(77),
        Native.GetSystemMetrics(76) + Native.GetSystemMetrics(78),
        Native.GetSystemMetrics(77) + Native.GetSystemMetrics(79));

    [StructLayout(LayoutKind.Sequential)]
    private struct BitmapHeader
    {
        internal uint Size;
        internal int Width;
        internal int Height;
        internal ushort Planes;
        internal ushort BitCount;
        internal uint Compression;
        internal uint SizeImage;
        internal int XPelsPerMeter;
        internal int YPelsPerMeter;
        internal uint ColoursUsed;
        internal uint ColoursImportant;
    }

    [LibraryImport("user32.dll")]
    private static partial nint GetDC(nint window);

    [LibraryImport("user32.dll")]
    private static partial int ReleaseDC(nint window, nint context);

    [LibraryImport("gdi32.dll")]
    private static partial nint CreateCompatibleDC(nint context);

    [LibraryImport("gdi32.dll")]
    private static partial nint CreateCompatibleBitmap(nint context, int width, int height);

    [LibraryImport("gdi32.dll")]
    private static partial nint SelectObject(nint context, nint item);

    [LibraryImport("gdi32.dll")]
    private static partial int BitBlt(nint target, int x, int y, int width, int height, nint source, int sourceX, int sourceY, uint operation);

    [LibraryImport("gdi32.dll")]
    private static partial int GetDIBits(nint context, nint bitmap, uint start, uint lines, byte* bits, ref BitmapHeader info, uint usage);

    [LibraryImport("gdi32.dll")]
    private static partial int DeleteObject(nint item);

    [LibraryImport("gdi32.dll")]
    private static partial int DeleteDC(nint context);
}
