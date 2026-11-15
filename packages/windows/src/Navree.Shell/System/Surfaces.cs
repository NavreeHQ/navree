using System.Runtime.InteropServices;
using Microsoft.UI.Dispatching;
using Microsoft.Windows.AppNotifications;
using Microsoft.Windows.AppNotifications.Builder;
using Microsoft.Windows.BadgeNotifications;
using Navree.Presence;
using Navree.Protocol;
using Windows.ApplicationModel;
using Windows.ApplicationModel.DataTransfer;
using Windows.UI.StartScreen;

namespace Navree.Shell;

/// <summary>
/// The count on the taskbar button. With package identity it is the system's own badge; without, a small icon the app draws over the
/// button's own. A call that needs identity is made only with it.
/// </summary>
internal sealed class BadgeSurface(nint window, bool hasIdentity) : IDisposable
{
    private readonly TaskbarOverlay overlay = new();

    /// <summary>Shows the count, or takes it away.</summary>
    internal bool Show(string app, long? count)
    {
        try
        {
            if (hasIdentity)
            {
                if (BadgeRules.Shown(count) is null)
                {
                    BadgeNotificationManager.Current.ClearBadge();
                }
                else
                {
                    BadgeNotificationManager.Current.SetBadgeAsCount(BadgeRules.ForSystem(count));
                }

                return true;
            }

            return overlay.Set(window, BadgeRules.OverlayText(count), BadgeRules.Description(app, count));
        }
        catch (Exception problem) when (problem is COMException or InvalidOperationException or UnauthorizedAccessException)
        {
            return false;
        }
    }

    public void Dispose() => overlay.Dispose();
}

/// <summary>The app's own notifications: shown now, taken back by their id.</summary>
internal static class NotificationSurface
{
    private const int TagLongest = 64;

    internal static bool Show(PendingNotification item)
    {
        try
        {
            AppNotificationBuilder builder = new AppNotificationBuilder().AddText(item.Title);
            if (!string.IsNullOrEmpty(item.Body))
            {
                _ = builder.AddText(item.Body);
            }

            if (item.Href is not null)
            {
                _ = builder.AddArgument("href", item.Href);
            }

            _ = builder.AddArgument("id", item.Id);
            AppNotification notification = builder.BuildNotification();
            notification.Tag = Tag(item.Id);
            if (!string.IsNullOrEmpty(item.Thread))
            {
                notification.Group = Tag(item.Thread);
            }

            AppNotificationManager.Default.Show(notification);
            return true;
        }
        catch (Exception problem) when (problem is COMException or InvalidOperationException or UnauthorizedAccessException)
        {
            return false;
        }
    }

    internal static void Remove(string id)
    {
        try
        {
            _ = AppNotificationManager.Default.RemoveByTagAsync(Tag(id));
        }
        catch (Exception problem) when (problem is COMException or InvalidOperationException)
        {
            // Nothing to take back that the system knows of.
        }
    }

    internal static void RemoveAll()
    {
        try
        {
            _ = AppNotificationManager.Default.RemoveAllAsync();
        }
        catch (Exception problem) when (problem is COMException or InvalidOperationException)
        {
            // Nothing shown that the system knows of.
        }
    }

    private static string Tag(string text) => text.Length <= TagLongest ? text : text[..TagLongest];
}

/// <summary>The taskbar button's jump list: the app's destinations as its tasks. It needs package identity.</summary>
internal static class JumpListSurface
{
    internal static async Task<bool> UpdateAsync(IReadOnlyList<PlaceDestination> destinations, int limit)
    {
        try
        {
            JumpList list = await JumpList.LoadCurrentAsync();
            list.Items.Clear();
            foreach (PlaceDestination destination in destinations.Take(Math.Max(0, limit)))
            {
                string? arguments = LaunchArguments.ForJumpList(destination.Href);
                if (arguments is null || string.IsNullOrWhiteSpace(destination.Label))
                {
                    continue;
                }

                JumpListItem item = JumpListItem.CreateWithArguments(arguments, destination.Label);
                item.Description = "Open " + destination.Label;
                list.Items.Add(item);
            }

            await list.SaveAsync();
            return true;
        }
        catch (Exception problem) when (problem is COMException or InvalidOperationException or UnauthorizedAccessException)
        {
            return false;
        }
    }
}

/// <summary>Opening at sign-in: the package's startup task with identity, this person's list of programs to run without.</summary>
internal sealed class StartupSurface(bool hasIdentity, string taskId, string name)
{
    private readonly RunKey runKey = new(name);

    internal async Task<StartupState> ReadAsync()
    {
        try
        {
            if (hasIdentity)
            {
                StartupTask task = await StartupTask.GetAsync(taskId);
                return StartupRules.FromTask((int)task.State);
            }

            return runKey.State(Environment.ProcessPath ?? string.Empty);
        }
        catch (Exception problem) when (problem is COMException or InvalidOperationException or ArgumentException)
        {
            return StartupState.Off;
        }
    }

    internal async Task<StartupState> SetAsync(bool on)
    {
        try
        {
            if (hasIdentity)
            {
                StartupTask task = await StartupTask.GetAsync(taskId);
                if (on)
                {
                    StartupTaskState state = await task.RequestEnableAsync();
                    return StartupRules.FromTask((int)state);
                }

                task.Disable();
                return StartupState.Off;
            }

            _ = runKey.Set(Environment.ProcessPath ?? string.Empty, on);
            return runKey.State(Environment.ProcessPath ?? string.Empty);
        }
        catch (Exception problem) when (problem is COMException or InvalidOperationException or ArgumentException)
        {
            return await ReadAsync();
        }
    }
}

/// <summary>The window's own interface for the share sheet, which the toolkit has no call for.</summary>
[ComImport]
[Guid("3A3DCD6C-3EAB-43DC-BCDE-45671CE800C8")]
[InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface IDataTransferManagerInterop
{
    nint GetForWindow([In] nint window, [In] ref Guid riid);

    void ShowShareUIForWindow(nint window);
}

/// <summary>The system's share sheet for a window: packaged or not, by the window's handle.</summary>
internal sealed class ShareSurface(nint window)
{
    private static readonly Guid ManagerId = new(0xa5caee9b, 0x8708, 0x49d1, 0x8d, 0x36, 0x67, 0xd2, 0x5a, 0x8d, 0xa0, 0x0c);
    private DataTransferManager? manager;
    private Action<DataPackage>? fill;
    private TaskCompletionSource<bool?>? pending;

    /// <summary>Opens the sheet for what is shared; completes with whether the person shared it, or null when the sheet could not be shown.</summary>
    internal async Task<bool?> ShowAsync(string title, string? text, Uri? link)
    {
        if (pending is not null)
        {
            return null;
        }

        TaskCompletionSource<bool?> done = new(TaskCreationOptions.RunContinuationsAsynchronously);
        pending = done;
        fill = package =>
        {
            package.Properties.Title = title;
            if (!string.IsNullOrEmpty(text))
            {
                package.SetText(text);
            }

            if (link is not null)
            {
                package.SetWebLink(link);
            }

            package.ShareCompleted += (_, _) => _ = done.TrySetResult(true);
            if (OperatingSystem.IsWindowsVersionAtLeast(10, 0, 19041))
            {
                package.ShareCanceled += (_, _) => _ = done.TrySetResult(false);
            }

        };
        try
        {
            IDataTransferManagerInterop interop = DataTransferManager.As<IDataTransferManagerInterop>();
            if (manager is null)
            {
                Guid id = ManagerId;
                manager = WinRT.MarshalInterface<DataTransferManager>.FromAbi(interop.GetForWindow(window, ref id));
                manager.DataRequested += (_, args) => fill?.Invoke(args.Request.Data);
            }

            interop.ShowShareUIForWindow(window);
        }
        catch (Exception problem) when (problem is COMException or InvalidCastException or InvalidOperationException)
        {
            pending = null;
            return null;
        }

        // A sheet that is dismissed without a word from the system is a share that did not happen, not one the core waits on forever.
        _ = await Task.WhenAny(done.Task, Task.Delay(TimeSpan.FromMinutes(5)));
        _ = done.TrySetResult(false);
        pending = null;
        return await done.Task;
    }
}
