using Microsoft.UI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Navree.Protocol;

namespace Navree.Shell;

/// <summary>
/// What the system's own look gives, looked up by the names its resources have: the brushes and the type
/// ramp of the theme the app runs in, so light, dark and the contrast themes are the system's and never ours.
/// </summary>
internal static class Look
{
    /// <summary>A brush of the system's theme, by its resource name; a plain one when the system has none by that name.</summary>
    internal static Brush Resource(string key)
    {
        if (Application.Current.Resources.TryGetValue(key, out object? value) && value is Brush found)
        {
            return found;
        }

        return new SolidColorBrush(Colors.Gray);
    }

    /// <summary>A style of the system's type ramp, by its resource name.</summary>
    internal static Style? Ramp(string key) =>
        Application.Current.Resources.TryGetValue(key, out object? value) ? value as Style : null;

    /// <summary>The brush a tone writes its text in.</summary>
    internal static Brush ToneBrush(Models.Tone? tone) =>
        tone switch
        {
            Models.Tone.Accent => Resource("AccentTextFillColorPrimaryBrush"),
            Models.Tone.Success => Resource("SystemFillColorSuccessBrush"),
            Models.Tone.Warning => Resource("SystemFillColorCautionBrush"),
            Models.Tone.Danger => Resource("SystemFillColorCriticalBrush"),
            Models.Tone.Info => Resource("SystemFillColorAttentionBrush"),
            _ => Resource("TextFillColorPrimaryBrush"),
        };

    /// <summary>The severity an info bar shows for a tone.</summary>
    internal static InfoBarSeverity Severity(Models.Tone tone) =>
        tone switch
        {
            Models.Tone.Success => InfoBarSeverity.Success,
            Models.Tone.Warning => InfoBarSeverity.Warning,
            Models.Tone.Danger => InfoBarSeverity.Error,
            _ => InfoBarSeverity.Informational,
        };

    /// <summary>The system's secondary text brush, for what is said in passing.</summary>
    internal static Brush Muted => Resource("TextFillColorSecondaryBrush");

    /// <summary>
    /// A text that begins with a digit or a sign and holds nothing of a right-to-left script (a phone number, an amount) is
    /// marked as left to right, so a right-to-left page does not turn it round; the mark is invisible and says nothing to a screen reader.
    /// </summary>
    internal static string Directed(string text)
    {
        if (text.Length == 0 || char.IsLetter(text[0]))
        {
            return text;
        }

        foreach (char letter in text)
        {
            if (letter is >= '\u0590' and <= '\u08FF' or >= '\uFB1D' and <= '\uFDFF' or >= '\uFE70' and <= '\uFEFF')
            {
                return text;
            }
        }

        return "\u200E" + text;
    }

    /// <summary>A text, in a style of the type ramp.</summary>
    internal static TextBlock Label(string text, string? ramp = null, bool muted = false, Models.Tone? tone = null)
    {
        TextBlock block = new() { Text = Directed(text), TextWrapping = TextWrapping.Wrap, TextReadingOrder = TextReadingOrder.DetectFromContent };
        if (ramp is not null)
        {
            Style? found = Ramp(ramp);
            if (found is not null)
            {
                block.Style = found;
            }
        }

        if (tone is not null && tone != Models.Tone.Neutral)
        {
            block.Foreground = ToneBrush(tone);
        }
        else if (muted)
        {
            block.Foreground = Muted;
        }

        return block;
    }

    /// <summary>A weight of the type ramp, by the protocol's word.</summary>
    internal static Windows.UI.Text.FontWeight WeightOf(Models.Weight weight) =>
        weight switch
        {
            Models.Weight.Medium => FontWeights.Medium,
            Models.Weight.Semibold => FontWeights.SemiBold,
            Models.Weight.Bold => FontWeights.Bold,
            _ => FontWeights.Normal,
        };

    /// <summary>One of the system's own icons, in the font they are drawn in.</summary>
    internal static FontIcon Glyph(string glyph, double size = 16) => new() { Glyph = glyph, FontSize = size };
}
