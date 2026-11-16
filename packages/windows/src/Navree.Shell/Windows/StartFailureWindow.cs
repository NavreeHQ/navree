using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;

namespace Navree.Shell;

/// <summary>
/// A window that says, in words, why the app could not start (its core could not be had: which file, where it was looked for), with a button
/// that closes the app. It needs nothing of the core, so an app shows it when making its <see cref="ShellHost"/> failed.
/// </summary>
public sealed class StartFailureWindow : Window
{
    /// <summary>Makes the window; <see cref="Window.Activate"/> shows it.</summary>
    /// <param name="appName">The app's name, for the window's title.</param>
    /// <param name="words">Why the app could not start.</param>
    public StartFailureWindow(string appName, string words)
    {
        ArgumentException.ThrowIfNullOrEmpty(appName);
        ArgumentException.ThrowIfNullOrEmpty(words);
        Title = appName;
        TextBlock heading = new() { Text = appName + " could not start", Style = (Style)Application.Current.Resources["SubtitleTextBlockStyle"], TextWrapping = TextWrapping.Wrap };
        AutomationProperties.SetHeadingLevel(heading, Microsoft.UI.Xaml.Automation.Peers.AutomationHeadingLevel.Level1);
        TextBlock said = new() { Text = words, TextWrapping = TextWrapping.Wrap, IsTextSelectionEnabled = true };
        Button close = new() { Content = "Close", HorizontalAlignment = HorizontalAlignment.Right };
        close.Click += (_, _) => Application.Current.Exit();
        StackPanel body = new() { Spacing = 16, Padding = new Thickness(24), MaxWidth = 560 };
        body.Children.Add(heading);
        body.Children.Add(said);
        body.Children.Add(close);
        Content = new ScrollViewer { Content = body };
        AppWindow.Resize(new Windows.Graphics.SizeInt32(640, 320));
    }
}
