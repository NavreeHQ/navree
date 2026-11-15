using Navree.Presence;
using Windows.Security.Credentials;

namespace Navree.Shell;

/// <summary>
/// Windows Hello behind the one interface a confirmation by the device's own means goes through. It says whether the device can do it at all.
/// Signing something with it, and the passkeys, are not written yet and have not been tried on a device with Hello enrolled: they answer as
/// not supported, and no build lists the features that would use them.
/// </summary>
public sealed class WindowsHelloAuthenticator : IDeviceAuthenticator
{
    /// <inheritdoc />
    public async Task<bool> IsAvailableAsync(CancellationToken cancel)
    {
        try
        {
            return await KeyCredentialManager.IsSupportedAsync();
        }
        catch (Exception)
        {
            return false;
        }
    }

    /// <inheritdoc />
    public Task<DeviceAnswer> ConfirmAsync(string reason, CancellationToken cancel) => Task.FromResult(DeviceAnswer.Unsupported);
}
