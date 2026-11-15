using System.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation.Peers;

namespace Navree.Demo;

/// <summary>What assistive technology is given: the tree of automation peers of an element, with the name and the kind each exposes.</summary>
internal static class AutomationDump
{
    /// <summary>Writes the tree as indented lines: the kind, then the name a screen reader says.</summary>
    internal static string Of(FrameworkElement root, int depth = 16)
    {
        StringBuilder text = new();
        AutomationPeer? peer = FrameworkElementAutomationPeer.CreatePeerForElement(root);
        if (peer is not null)
        {
            Write(text, peer, 0, depth);
        }

        return text.ToString();
    }

    private static void Write(StringBuilder text, AutomationPeer peer, int level, int depth)
    {
        string name = peer.GetName();
        AutomationControlType kind = peer.GetAutomationControlType();
        bool interesting = name.Length > 0 || kind is not (AutomationControlType.Custom or AutomationControlType.Pane or AutomationControlType.Group);
        int next = level;
        if (interesting)
        {
            text.Append(' ', level * 2).Append(kind).Append(": ").Append(name.Length == 0 ? "(no name)" : name).AppendLine();
            next = level + 1;
        }

        if (level >= depth)
        {
            return;
        }

        IList<AutomationPeer>? children = peer.GetChildren();
        if (children is null)
        {
            return;
        }

        foreach (AutomationPeer child in children)
        {
            Write(text, child, next, depth);
        }
    }
}
