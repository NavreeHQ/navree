using System.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;

namespace Navree.Demo;

/// <summary>What the window shows in words: the text of every control in it, as a person reading the window would find it.</summary>
internal static class DrawnText
{
    /// <summary>All the text in the tree under the element.</summary>
    /// <param name="root">The element, usually the window's content.</param>
    /// <returns>The words, one control's after another's.</returns>
    internal static string Of(UIElement? root)
    {
        StringBuilder words = new();
        if (root is not null)
        {
            Visit(root, words);
        }

        return words.ToString();
    }

    private static void Visit(DependencyObject node, StringBuilder words)
    {
        switch (node)
        {
            case TextBlock block:
                words.Append(block.Text).Append('\n');
                break;
            case TextBox box:
                words.Append(box.Text).Append('\n');
                break;
            case RichTextBlock rich:
                foreach (Block part in rich.Blocks)
                {
                    if (part is Paragraph paragraph)
                    {
                        foreach (Inline inline in paragraph.Inlines)
                        {
                            if (inline is Run run)
                            {
                                words.Append(run.Text);
                            }
                        }

                        words.Append('\n');
                    }
                }

                break;
            default:
                break;
        }

        for (int child = 0; child < VisualTreeHelper.GetChildrenCount(node); child++)
        {
            Visit(VisualTreeHelper.GetChild(node, child), words);
        }
    }
}
