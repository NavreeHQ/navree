using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Navree.Presence;

/// <summary>The calls of the system that WinUI has no control for, as they are declared in its own headers.</summary>
internal static unsafe partial class Native
{
    internal const uint WmNull = 0x0000;
    internal const uint WmContextMenu = 0x007B;
    internal const uint WmSetIcon = 0x0080;
    internal const uint WmApp = 0x8000;
    internal const uint NinSelect = 0x0400;
    internal const uint NinKeySelect = 0x0401;

    internal const uint NimAdd = 0;
    internal const uint NimModify = 1;
    internal const uint NimDelete = 2;
    internal const uint NimSetVersion = 4;
    internal const uint NifMessage = 0x1;
    internal const uint NifIcon = 0x2;
    internal const uint NifTip = 0x4;
    internal const uint NifShowTip = 0x80;
    internal const uint NotifyIconVersion4 = 4;

    internal const uint MfString = 0x0;
    internal const uint MfSeparator = 0x800;
    internal const uint TpmReturnCommand = 0x100;
    internal const uint TpmRightButton = 0x2;
    internal const uint TpmBottomAlign = 0x20;
    internal const uint TpmLayoutRtl = 0x8000;

    internal const int GwlExStyle = -20;
    internal const int WsExLayoutRtl = 0x00400000;

    [StructLayout(LayoutKind.Sequential)]
    internal struct Rect
    {
        internal int Left;
        internal int Top;
        internal int Right;
        internal int Bottom;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct WndClassEx
    {
        internal uint Size;
        internal uint Style;
        internal nint WindowProc;
        internal int ClassExtra;
        internal int WindowExtra;
        internal nint Instance;
        internal nint Icon;
        internal nint Cursor;
        internal nint Background;
        internal nint MenuName;
        internal nint ClassName;
        internal nint SmallIcon;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct NotifyIconData
    {
        internal uint Size;
        internal nint Window;
        internal uint Id;
        internal uint Flags;
        internal uint CallbackMessage;
        internal nint Icon;
        internal fixed char Tip[128];
        internal uint State;
        internal uint StateMask;
        internal fixed char Info[256];
        internal uint VersionOrTimeout;
        internal fixed char InfoTitle[64];
        internal uint InfoFlags;
        internal Guid Item;
        internal nint BalloonIcon;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct NotifyIconIdentifier
    {
        internal uint Size;
        internal nint Window;
        internal uint Id;
        internal Guid Item;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct IconInfo
    {
        internal int IsIcon;
        internal uint HotspotX;
        internal uint HotspotY;
        internal nint Mask;
        internal nint Colour;
    }

    [LibraryImport("shell32.dll", EntryPoint = "Shell_NotifyIconW")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static partial bool ShellNotifyIcon(uint message, ref NotifyIconData data);

    [LibraryImport("shell32.dll", EntryPoint = "Shell_NotifyIconGetRect")]
    internal static partial int ShellNotifyIconGetRect(ref NotifyIconIdentifier identifier, out Rect rect);

    [LibraryImport("user32.dll", EntryPoint = "RegisterClassExW", SetLastError = true)]
    internal static partial ushort RegisterClassEx(ref WndClassEx classInfo);

    [LibraryImport("user32.dll", EntryPoint = "CreateWindowExW", SetLastError = true, StringMarshalling = StringMarshalling.Utf16)]
    internal static partial nint CreateWindowEx(uint exStyle, string className, string windowName, uint style, int x, int y, int width, int height, nint parent, nint menu, nint instance, nint param);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static partial bool DestroyWindow(nint window);

    [LibraryImport("user32.dll", EntryPoint = "DefWindowProcW")]
    internal static partial nint DefWindowProc(nint window, uint message, nuint wParam, nint lParam);

    [LibraryImport("user32.dll", EntryPoint = "RegisterWindowMessageW", StringMarshalling = StringMarshalling.Utf16)]
    internal static partial uint RegisterWindowMessage(string name);

    [LibraryImport("kernel32.dll", EntryPoint = "GetModuleHandleW")]
    internal static partial nint GetModuleHandle(nint name);

    [LibraryImport("user32.dll", EntryPoint = "CreatePopupMenu")]
    internal static partial nint CreatePopupMenu();

    [LibraryImport("user32.dll", EntryPoint = "AppendMenuW", StringMarshalling = StringMarshalling.Utf16)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static partial bool AppendMenu(nint menu, uint flags, nuint id, string? text);

    [LibraryImport("user32.dll")]
    internal static partial int TrackPopupMenuEx(nint menu, uint flags, int x, int y, nint window, nint parameters);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static partial bool DestroyMenu(nint menu);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static partial bool SetForegroundWindow(nint window);

    [LibraryImport("user32.dll", EntryPoint = "PostMessageW")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static partial bool PostMessage(nint window, uint message, nuint wParam, nint lParam);

    [LibraryImport("user32.dll", EntryPoint = "SendMessageW")]
    internal static partial nint SendMessage(nint window, uint message, nuint wParam, nint lParam);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static partial bool ShowWindow(nint window, int command);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static partial bool IsIconic(nint window);

    [LibraryImport("user32.dll", EntryPoint = "GetWindowLongW")]
    internal static partial int GetWindowLong(nint window, int index);

    [LibraryImport("user32.dll", EntryPoint = "SetWindowLongW")]
    internal static partial int SetWindowLong(nint window, int index, int value);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static partial bool SetWindowPos(nint window, nint after, int x, int y, int width, int height, uint flags);

    [LibraryImport("user32.dll")]
    internal static partial int GetSystemMetrics(int index);

    [LibraryImport("user32.dll")]
    internal static partial uint GetDpiForWindow(nint window);

    [LibraryImport("gdi32.dll")]
    internal static partial nint CreateBitmap(int width, int height, uint planes, uint bitsPerPixel, byte* bits);

    [LibraryImport("gdi32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static partial bool DeleteObject(nint handle);

    [LibraryImport("user32.dll")]
    internal static partial nint CreateIconIndirect(ref IconInfo info);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static partial bool DestroyIcon(nint icon);
}

/// <summary>An icon the app draws itself, as the system's own handle to one.</summary>
public static unsafe class IconHandle
{
    /// <summary>The side of the small icon the system wants at the current scale, in pixels.</summary>
    /// <returns>The side.</returns>
    public static int SmallSide() => Math.Max(16, Native.GetSystemMetrics(49));

    /// <summary>Makes an icon from pixels. The caller gives it back with <see cref="Destroy"/>.</summary>
    /// <param name="bgra">Straight BGRA pixels, <c>side * side * 4</c> bytes.</param>
    /// <param name="side">The side, in pixels.</param>
    /// <returns>The handle, or 0 when the system would not make one.</returns>
    public static nint Create(byte[] bgra, int side)
    {
        ArgumentNullException.ThrowIfNull(bgra);
        if (bgra.Length != side * side * 4)
        {
            throw new ArgumentException("the pixels are not a square of that side", nameof(bgra));
        }

        byte[] mask = new byte[((side + 15) / 16) * 2 * side];
        nint colour;
        nint monochrome;
        fixed (byte* pixels = bgra)
        {
            colour = Native.CreateBitmap(side, side, 1, 32, pixels);
        }

        fixed (byte* bits = mask)
        {
            monochrome = Native.CreateBitmap(side, side, 1, 1, bits);
        }

        if (colour == 0 || monochrome == 0)
        {
            _ = colour == 0 || Native.DeleteObject(colour);
            _ = monochrome == 0 || Native.DeleteObject(monochrome);
            return 0;
        }

        Native.IconInfo info = new() { IsIcon = 1, Mask = monochrome, Colour = colour };
        nint icon = Native.CreateIconIndirect(ref info);
        _ = Native.DeleteObject(colour);
        _ = Native.DeleteObject(monochrome);
        return icon;
    }

    /// <summary>Gives an icon back to the system.</summary>
    /// <param name="icon">The handle.</param>
    public static void Destroy(nint icon)
    {
        if (icon != 0)
        {
            _ = Native.DestroyIcon(icon);
        }
    }
}

/// <summary>
/// The app's icon in the notification area, over the system's call for it (WinUI has no control for one). It owns a hidden window
/// that the system sends the icon's events to: a left click or the keyboard's choice, and the context menu's request. The
/// icon is put back when Explorer starts again and the notification area is made anew.
/// </summary>
public sealed unsafe class TrayIcon : IDisposable
{
    private const string ClassName = "Navree.Presence.TrayWindow";
    private static readonly Dictionary<nint, TrayIcon> Instances = [];
    private static bool registered;
    private static uint taskbarCreated;
    private nint window;
    private nint icon;
    private string tip = string.Empty;
    private bool added;

    /// <summary>Raised when the icon is chosen with the left button or the keyboard; with the place the system says it was at.</summary>
    public event Action<int, int>? Selected;

    /// <summary>Raised when the icon's menu is asked for, with a right click or the menu key; with the place.</summary>
    public event Action<int, int>? ContextRequested;

    /// <summary>Whether the icon is in the notification area.</summary>
    public bool IsAdded => added;

    /// <summary>The hidden window the icon's events go to; a menu is shown from it.</summary>
    public nint Owner => window;

    /// <summary>Puts the icon in the notification area, or changes it when it is there.</summary>
    /// <param name="tooltip">What the tooltip says.</param>
    /// <param name="iconHandle">The icon; the caller keeps it alive while it is shown.</param>
    /// <returns>Whether the system took it.</returns>
    public bool Show(string tooltip, nint iconHandle)
    {
        ArgumentNullException.ThrowIfNull(tooltip);
        EnsureWindow();
        tip = tooltip;
        icon = iconHandle;
        if (added)
        {
            Native.NotifyIconData data = Data();
            return Native.ShellNotifyIcon(Native.NimModify, ref data);
        }

        return Add();
    }

    /// <summary>Takes the icon out of the notification area.</summary>
    public void Hide()
    {
        if (!added || window == 0)
        {
            return;
        }

        Native.NotifyIconData data = Data();
        _ = Native.ShellNotifyIcon(Native.NimDelete, ref data);
        added = false;
    }

    /// <summary>Where the icon is on the screen.</summary>
    /// <returns>The rectangle, or null when the system cannot say.</returns>
    public Box? Rectangle()
    {
        if (!added)
        {
            return null;
        }

        Native.NotifyIconIdentifier identifier = new() { Size = (uint)sizeof(Native.NotifyIconIdentifier), Window = window, Id = 1 };
        return Native.ShellNotifyIconGetRect(ref identifier, out Native.Rect rect) == 0
            ? new Box(rect.Left, rect.Top, rect.Right, rect.Bottom)
            : null;
    }

    /// <summary>
    /// Shows a menu at a place and waits for the choice, as the system's own icons do: the menu is the system's, and the app's hidden
    /// window is made the foreground one first so the menu closes when the person clicks away.
    /// </summary>
    /// <param name="entries">The entries.</param>
    /// <param name="x">The place across.</param>
    /// <param name="y">The place down.</param>
    /// <param name="rightToLeft">Whether the menu is laid out from the right.</param>
    /// <returns>The entry chosen, or null.</returns>
    public TrayEntry? ShowMenu(IReadOnlyList<TrayEntry> entries, int x, int y, bool rightToLeft)
    {
        if (window == 0 || entries.Count == 0)
        {
            return null;
        }

        nint menu = Native.CreatePopupMenu();
        if (menu == 0)
        {
            return null;
        }

        try
        {
            for (int at = 0; at < entries.Count; at++)
            {
                TrayEntry entry = entries[at];
                _ = entry.Command == TrayCommand.Separator
                    ? Native.AppendMenu(menu, Native.MfSeparator, 0, null)
                    : Native.AppendMenu(menu, Native.MfString, (nuint)(at + 1), entry.Label);
            }

            _ = Native.SetForegroundWindow(window);
            uint flags = Native.TpmReturnCommand | Native.TpmRightButton | Native.TpmBottomAlign | (rightToLeft ? Native.TpmLayoutRtl : 0);
            int chosen = Native.TrackPopupMenuEx(menu, flags, x, y, window, 0);
            _ = Native.PostMessage(window, Native.WmNull, 0, 0);
            return chosen > 0 && chosen <= entries.Count ? entries[chosen - 1] : null;
        }
        finally
        {
            _ = Native.DestroyMenu(menu);
        }
    }

    /// <inheritdoc />
    public void Dispose()
    {
        Hide();
        if (window != 0)
        {
            _ = Instances.Remove(window);
            _ = Native.DestroyWindow(window);
            window = 0;
        }
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
    private static nint WindowProc(nint hwnd, uint message, nuint wParam, nint lParam)
    {
        try
        {
            if (Instances.TryGetValue(hwnd, out TrayIcon? tray))
            {
                tray.Handle(message, wParam, lParam);
            }
        }
        catch (Exception)
        {
            // A window procedure that throws would take the app with it; the icon's events are best effort.
        }

        return Native.DefWindowProc(hwnd, message, wParam, lParam);
    }

    private void Handle(uint message, nuint wParam, nint lParam)
    {
        if (message == Native.WmApp + 1)
        {
            uint code = (uint)(lParam & 0xFFFF);
            int x = (short)(wParam & 0xFFFF);
            int y = (short)((wParam >> 16) & 0xFFFF);
            if (code is Native.NinSelect or Native.NinKeySelect)
            {
                Selected?.Invoke(x, y);
            }
            else if (code == Native.WmContextMenu)
            {
                ContextRequested?.Invoke(x, y);
            }
        }
        else if (taskbarCreated != 0 && message == taskbarCreated && added)
        {
            added = false;
            _ = Add();
        }
    }

    private bool Add()
    {
        Native.NotifyIconData data = Data();
        if (!Native.ShellNotifyIcon(Native.NimAdd, ref data))
        {
            return false;
        }

        data.VersionOrTimeout = Native.NotifyIconVersion4;
        _ = Native.ShellNotifyIcon(Native.NimSetVersion, ref data);
        added = true;
        return true;
    }

    private Native.NotifyIconData Data()
    {
        Native.NotifyIconData data = default;
        data.Size = (uint)sizeof(Native.NotifyIconData);
        data.Window = window;
        data.Id = 1;
        data.Flags = Native.NifMessage | Native.NifIcon | Native.NifTip | Native.NifShowTip;
        data.CallbackMessage = Native.WmApp + 1;
        data.Icon = icon;
        int length = Math.Min(tip.Length, 127);
        for (int at = 0; at < length; at++)
        {
            data.Tip[at] = tip[at];
        }

        return data;
    }

    private void EnsureWindow()
    {
        if (window != 0)
        {
            return;
        }

        nint instance = Native.GetModuleHandle(0);
        if (!registered)
        {
            Native.WndClassEx classInfo = new()
            {
                Size = (uint)sizeof(Native.WndClassEx),
                WindowProc = (nint)(delegate* unmanaged[Stdcall]<nint, uint, nuint, nint, nint>)&WindowProc,
                Instance = instance,
                ClassName = Marshal.StringToHGlobalUni(ClassName),
            };
            _ = Native.RegisterClassEx(ref classInfo);
            taskbarCreated = Native.RegisterWindowMessage("TaskbarCreated");
            registered = true;
        }

        // A window that is never shown, and is not a message-only window: those do not get the message that says Explorer started again.
        window = Native.CreateWindowEx(0x80, ClassName, "Navree notification area", 0x80000000, 0, 0, 0, 0, 0, 0, instance, 0);
        if (window != 0)
        {
            Instances[window] = this;
        }
    }
}

/// <summary>The window's own frame, where the toolkit has no property for it.</summary>
public static class WindowFrame
{
    /// <summary>
    /// Lays the window's frame out from the right (the caption buttons at the left, the title at the right), as Windows does for a
    /// window of a right-to-left language. The toolkit mirrors what is inside the window; the frame is the window's own.
    /// </summary>
    /// <param name="window">The window's handle.</param>
    /// <param name="rightToLeft">Whether to lay it out from the right.</param>
    public static void SetRightToLeft(nint window, bool rightToLeft)
    {
        int style = Native.GetWindowLong(window, Native.GwlExStyle);
        int wanted = rightToLeft ? style | Native.WsExLayoutRtl : style & ~Native.WsExLayoutRtl;
        if (wanted == style)
        {
            return;
        }

        _ = Native.SetWindowLong(window, Native.GwlExStyle, wanted);
        _ = Native.SetWindowPos(window, 0, 0, 0, 0, 0, 0x37);
    }

    /// <summary>Gives the window the icon the taskbar and Alt+Tab show for it.</summary>
    /// <param name="window">The window's handle.</param>
    /// <param name="icon">The icon, which the caller keeps alive.</param>
    public static void SetIcon(nint window, nint icon)
    {
        _ = Native.SendMessage(window, Native.WmSetIcon, 0, icon);
        _ = Native.SendMessage(window, Native.WmSetIcon, 1, icon);
    }

    /// <summary>How many device pixels make one effective pixel on the screen the window is on (1 at 96 dpi).</summary>
    /// <param name="window">The window's handle.</param>
    /// <returns>The scale.</returns>
    public static double ScaleOf(nint window)
    {
        uint dpi = Native.GetDpiForWindow(window);
        return dpi == 0 ? 1.0 : dpi / 96.0;
    }

    /// <summary>Brings the window to the front, restoring it when it is minimised.</summary>
    /// <param name="window">The window's handle.</param>
    public static void Raise(nint window)
    {
        if (Native.IsIconic(window))
        {
            _ = Native.ShowWindow(window, 9);
        }

        _ = Native.SetForegroundWindow(window);
    }
}

/// <summary>
/// A count on the taskbar button for a build with no package identity: a small icon over the button's own, which the system draws.
/// (A package has the system's own badge, which this is not.)
/// </summary>
public sealed class TaskbarOverlay : IDisposable
{
    private ITaskbarList3? list;
    private nint current;

    /// <summary>Shows the count, or takes the overlay away.</summary>
    /// <param name="window">The window whose button it is on.</param>
    /// <param name="text">The characters to write on it, or null for none.</param>
    /// <param name="description">What a screen reader is told of it.</param>
    /// <returns>Whether the system took it.</returns>
    public bool Set(nint window, string? text, string description)
    {
        try
        {
            list ??= (ITaskbarList3)new TaskbarListClass();
            _ = list.HrInit();
            nint next = 0;
            if (text is not null)
            {
                int side = IconHandle.SmallSide();
                next = IconHandle.Create(IconArt.Overlay(text, side), side);
            }

            int answer = list.SetOverlayIcon(window, next, text is null ? string.Empty : description);
            IconHandle.Destroy(current);
            current = next;
            return answer == 0;
        }
        catch (Exception problem) when (problem is COMException or InvalidCastException)
        {
            return false;
        }
    }

    /// <inheritdoc />
    public void Dispose()
    {
        IconHandle.Destroy(current);
        current = 0;
    }

    [ComImport]
    [Guid("56FDF344-FD6D-11D0-958A-006097C9A090")]
    private class TaskbarListClass
    {
    }

    [ComImport]
    [Guid("EA1AFB91-9E28-4B86-90E9-9E9F8A5EEFAF")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface ITaskbarList3
    {
        [PreserveSig]
        int HrInit();

        [PreserveSig]
        int AddTab(nint window);

        [PreserveSig]
        int DeleteTab(nint window);

        [PreserveSig]
        int ActivateTab(nint window);

        [PreserveSig]
        int SetActiveAlt(nint window);

        [PreserveSig]
        int MarkFullscreenWindow(nint window, [MarshalAs(UnmanagedType.Bool)] bool fullscreen);

        [PreserveSig]
        int SetProgressValue(nint window, ulong completed, ulong total);

        [PreserveSig]
        int SetProgressState(nint window, int flags);

        [PreserveSig]
        int RegisterTab(nint tab, nint owner);

        [PreserveSig]
        int UnregisterTab(nint tab);

        [PreserveSig]
        int SetTabOrder(nint tab, nint before);

        [PreserveSig]
        int SetTabActive(nint tab, nint owner, uint reserved);

        [PreserveSig]
        int ThumbBarAddButtons(nint window, uint count, nint buttons);

        [PreserveSig]
        int ThumbBarUpdateButtons(nint window, uint count, nint buttons);

        [PreserveSig]
        int ThumbBarSetImageList(nint window, nint images);

        [PreserveSig]
        int SetOverlayIcon(nint window, nint icon, [MarshalAs(UnmanagedType.LPWStr)] string description);

        [PreserveSig]
        int SetThumbnailTooltip(nint window, [MarshalAs(UnmanagedType.LPWStr)] string tip);

        [PreserveSig]
        int SetThumbnailClip(nint window, nint clip);
    }
}
