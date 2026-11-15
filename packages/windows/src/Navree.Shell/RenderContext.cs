using Microsoft.UI.Xaml;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Shell;

/// <summary>A sheet, dialog or popover of a screen's tree, found while it was drawn: shown over the screen when the core says it is open.</summary>
/// <param name="Node">The overlay's node.</param>
/// <param name="Id">Its id.</param>
internal sealed record OverlayRequest(Models.Node Node, string Id);

/// <summary>What drawing one screen needs to know, and what it gives back besides the element: the overlays it found and the fields it drew.</summary>
internal sealed class RenderContext
{
    /// <summary>The screen being drawn.</summary>
    internal required ScreenView Screen { get; init; }

    /// <summary>Tells the core something happened.</summary>
    internal required Action<string> Send { get; init; }

    /// <summary>How wide the window is.</summary>
    internal WidthClass Width { get; init; } = WidthClass.Expanded;

    /// <summary>Loads a picture's bytes by a whole address the core made, or null.</summary>
    internal Func<string, Task<byte[]?>>? Pictures { get; init; }

    /// <summary>The sheets, dialogs and popovers found in the tree.</summary>
    internal List<OverlayRequest> Overlays { get; } = [];

    /// <summary>The fields drawn, by form and field id, so their state can be shown again without drawing the screen again.</summary>
    internal Dictionary<(string Form, string Field), FieldHandle> Fields { get; } = [];

    /// <summary>The element that raises assistive technology's notices.</summary>
    internal FrameworkElement? Announcer { get; init; }
}

/// <summary>A field that is on the screen, and how to show its state.</summary>
internal sealed class FieldHandle
{
    /// <summary>Shows what the field holds and what is wrong with it.</summary>
    internal required Action<FieldView?> Show { get; init; }
}
