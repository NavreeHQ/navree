namespace Navree.Presence;

/// <summary>Where a new window of the app goes and how small any may be, as plain rules.</summary>
public static class PlacementRules
{
    /// <summary>How far a new window is from the one before it, across and down: the title bar's height, as Windows' own cascade is.</summary>
    public const int Step = 32;

    /// <summary>
    /// The place of a new window: a step down and across from the window before it, inside the work area, and back at its
    /// top left corner when the next step would not fit.
    /// </summary>
    /// <param name="previous">The window before it, or null when it is the first.</param>
    /// <param name="work">The part of the screen the taskbar leaves.</param>
    /// <param name="width">The new window's width.</param>
    /// <param name="height">Its height.</param>
    /// <returns>Its left and top.</returns>
    public static (int Left, int Top) Cascade(Box? previous, Box work, int width, int height)
    {
        int firstLeft = work.Left + Math.Max(0, (work.Width - width) / 2);
        int firstTop = work.Top + Math.Max(0, (work.Height - height) / 2);
        if (previous is not Box before)
        {
            return (firstLeft, firstTop);
        }

        int left = before.Left + Step;
        int top = before.Top + Step;
        return left + width > work.Right || top + height > work.Bottom ? (work.Left, work.Top) : (left, top);
    }

    /// <summary>The size a new window opens at: the same as the window it is opened from, never smaller than the smallest a window may be.</summary>
    /// <param name="from">The window it is opened from, in pixels.</param>
    /// <param name="smallest">The smallest width, in pixels.</param>
    /// <param name="smallestHeight">The smallest height, in pixels.</param>
    /// <returns>Its width and height.</returns>
    public static (int Width, int Height) OpenSize(Box from, int smallest, int smallestHeight) =>
        (Math.Max(from.Width, smallest), Math.Max(from.Height, smallestHeight));
}

/// <summary>What a confirmation by the device's own means (Windows Hello) came to.</summary>
public enum DeviceAnswer
{
    /// <summary>The person was verified.</summary>
    Verified,

    /// <summary>This build does not do it.</summary>
    Unsupported,

    /// <summary>The person said no or closed the prompt.</summary>
    Cancelled,

    /// <summary>It could not be done: nothing is enrolled, or the system refused.</summary>
    Unavailable,
}

/// <summary>
/// Confirming a person with the device's own means (Windows Hello, passkeys). It is written behind this one interface so a build can
/// have it or not; no build here has been proven on a device with one enrolled, so none lists the features in its capabilities.
/// </summary>
public interface IDeviceAuthenticator
{
    /// <summary>Whether this device can do it at all.</summary>
    /// <param name="cancel">Ends the wait.</param>
    /// <returns>Whether it can.</returns>
    Task<bool> IsAvailableAsync(CancellationToken cancel);

    /// <summary>Asks the person to confirm.</summary>
    /// <param name="reason">What the person is told it is for.</param>
    /// <param name="cancel">Ends the wait.</param>
    /// <returns>What it came to.</returns>
    Task<DeviceAnswer> ConfirmAsync(string reason, CancellationToken cancel);
}

/// <summary>The authenticator of a build that has none: nothing is available and nothing is verified.</summary>
public sealed class NoAuthenticator : IDeviceAuthenticator
{
    /// <inheritdoc />
    public Task<bool> IsAvailableAsync(CancellationToken cancel) => Task.FromResult(false);

    /// <inheritdoc />
    public Task<DeviceAnswer> ConfirmAsync(string reason, CancellationToken cancel) => Task.FromResult(DeviceAnswer.Unsupported);
}
