using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Shell;

/// <summary>
/// Draws a screen of the protocol with no core behind it, so each drawn kind can be shown in every state and at every width, in the
/// theme the app runs in. What a person does to a part is handed to <c>send</c> and goes nowhere else.
/// </summary>
public static class KindsGallery
{
    /// <summary>Reads a screen from the JSON the server sends: an answer with a <c>screen</c> in it, or the screen itself.</summary>
    /// <param name="json">The JSON.</param>
    /// <returns>The screen.</returns>
    public static Models.Screen Read(string json)
    {
        using JsonDocument document = Reading.Open(json);
        JsonElement root = document.RootElement;
        return root.TryGetProperty("screen", out JsonElement screen) ? Models.ReadScreen(screen) : Models.ReadScreen(root);
    }

    /// <summary>Draws a screen's title and body as the app draws a screen's page.</summary>
    /// <param name="screen">The screen.</param>
    /// <param name="width">How wide the window is, in effective pixels.</param>
    /// <param name="send">Told of what a person does to a part.</param>
    /// <returns>The page.</returns>
    public static FrameworkElement Draw(Models.Screen screen, double width, Action<string>? send = null)
    {
        WidthClass klass = WindowRules.ClassOf(width);
        ScreenView view = new() { Id = 1, Content = new NativeContent(screen) };
        RenderContext context = new() { Screen = view, Send = send ?? (_ => { }), Width = klass };
        NodeRenderer renderer = new(context);
        double side = klass == WidthClass.Compact ? 16 : 32;
        StackPanel page = new() { Spacing = 16, Padding = new Thickness(side, 16, side, 32), Width = width };
        TextBlock title = Look.Label(screen.Title, klass == WidthClass.Compact ? "SubtitleTextBlockStyle" : "TitleTextBlockStyle");
        AutomationProperties.SetHeadingLevel(title, Microsoft.UI.Xaml.Automation.Peers.AutomationHeadingLevel.Level1);
        page.Children.Add(title);
        page.Children.Add(renderer.Render(screen.Body));
        return page;
    }
}
