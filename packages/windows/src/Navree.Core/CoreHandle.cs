using Microsoft.Win32.SafeHandles;

namespace Navree.Core;

/// <summary>
/// The core as the system sees it: a pointer that is given back to the core,
/// once, when the last user of the handle has let go.
/// </summary>
internal sealed class CoreHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    /// <summary>A handle that holds nothing yet.</summary>
    public CoreHandle()
        : base(ownsHandle: true)
    {
    }

    /// <summary>A handle that owns the core at <paramref name="raw"/>.</summary>
    public CoreHandle(nint raw)
        : this()
    {
        SetHandle(raw);
    }

    /// <inheritdoc />
    protected override bool ReleaseHandle()
    {
        CoreNative.Free(handle);
        return true;
    }
}
