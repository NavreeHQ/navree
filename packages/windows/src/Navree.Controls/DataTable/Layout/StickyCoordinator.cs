using System.Numerics;
using Microsoft.UI.Composition;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Hosting;
using Navree.Design.Tables;

namespace Navree.Controls;

/// <summary>How the parts that stay in place are moved against the scroll.</summary>
internal enum StickyMode
{
    /// <summary>By expression animations on the compositor, with the same numbers written to the elements at rest.</summary>
    Compositor,

    /// <summary>By expression animations only while the scroll view is not idle; at rest by numbers written to the elements.</summary>
    Static,

    /// <summary>Only by numbers written to the elements from the scroll view's change events.</summary>
    UiThread,
}

/// <summary>What a part that stays in place is.</summary>
internal enum StickyRole
{
    /// <summary>Kept at the start edge.</summary>
    Start,

    /// <summary>Kept at the end edge.</summary>
    End,

    /// <summary>The header row, which follows the scroll.</summary>
    Header,

    /// <summary>The scrolling cells of a row, cut at the frozen edges.</summary>
    ScrollCells,

    /// <summary>The line at the start edge, seen once the rows have scrolled.</summary>
    EdgeStart,

    /// <summary>The line at the end edge, seen until the rows have scrolled to their end.</summary>
    EdgeEnd,
}

/// <summary>
/// Keeps the header, the frozen columns and the open details in place while the rows scroll
/// sideways: they are moved on the compositor by expressions over the scroll view's own position,
/// and at rest the same numbers are written to the elements, which is what pointer hits and
/// accessibility rectangles read.
/// </summary>
internal sealed class StickyCoordinator
{
    private readonly ScrollView scroll;
    private readonly Dictionary<UIElement, StickyRole> parts = [];
    private Compositor? compositor;
    private CompositionPropertySet? layoutProperties;
    private ExpressionAnimation? startAnimation;
    private ExpressionAnimation? endAnimation;
    private ExpressionAnimation? headerAnimation;
    private ExpressionAnimation? clipLeftAnimation;
    private ExpressionAnimation? clipRightAnimation;
    private ExpressionAnimation? edgeStartAnimation;
    private ExpressionAnimation? edgeEndAnimation;
    private bool rtl;
    private double fs;
    private double fe;
    private bool animating;

    /// <summary>Makes a coordinator for a scroll view.</summary>
    /// <param name="scroll">The scroll view the rows are in.</param>
    internal StickyCoordinator(ScrollView scroll)
    {
        this.scroll = scroll;
    }

    /// <summary>How the parts are moved.</summary>
    internal StickyMode Mode { get; set; } = StickyMode.Compositor;

    /// <summary>What went wrong, in words, for a run to print.</summary>
    internal List<string> Problems { get; } = [];

    /// <summary>The number of parts being kept in place.</summary>
    internal int Count => parts.Count;

    /// <summary>Joins the scroll view's changes. Call when the scroll view is loaded.</summary>
    /// <param name="rightToLeft">Whether the flow is right to left.</param>
    internal void Attach(bool rightToLeft)
    {
        compositor = ElementCompositionPreview.GetElementVisual(scroll).Compositor;
        layoutProperties = compositor.CreatePropertySet();
        layoutProperties.InsertScalar("Fs", 0f);
        layoutProperties.InsertScalar("Fe", 0f);
        scroll.ViewChanged += (_, _) => Mirror();
        scroll.StateChanged += (_, _) => OnStateChanged();
        rtl = rightToLeft;
        BuildAnimations();
        animating = Mode == StickyMode.Compositor;
        foreach ((UIElement element, StickyRole role) in parts)
        {
            Begin(element, role);
        }
    }

    /// <summary>Tells the coordinator the widths kept at the start and at the end.</summary>
    /// <param name="startWidth">The width kept at the start.</param>
    /// <param name="endWidth">The width kept at the end.</param>
    internal void SetFrozenWidths(double startWidth, double endWidth)
    {
        fs = startWidth;
        fe = endWidth;
        layoutProperties?.InsertScalar("Fs", (float)startWidth);
        layoutProperties?.InsertScalar("Fe", (float)endWidth);
        Mirror();
    }

    /// <summary>Starts keeping a part in place.</summary>
    /// <param name="element">The part.</param>
    /// <param name="role">What it is.</param>
    internal void Register(UIElement element, StickyRole role)
    {
        parts[element] = role;
        if (role is StickyRole.Start or StickyRole.End or StickyRole.Header)
        {
            ElementCompositionPreview.SetIsTranslationEnabled(element, true);
        }

        if (compositor is not null)
        {
            Begin(element, role);
        }
    }

    /// <summary>Stops keeping a part in place.</summary>
    /// <param name="element">The part.</param>
    internal void Unregister(UIElement element)
    {
        if (parts.Remove(element, out StickyRole role) && compositor is not null)
        {
            End(element, role);
        }
    }

    /// <summary>Writes the numbers to the elements as the scroll view stands now.</summary>
    internal void Mirror()
    {
        double reach = Math.Max(0, scroll.ScrollableWidth);
        (double min, double max) = rtl ? (-reach, 0) : (0, reach);
        double position = rtl ? -scroll.HorizontalOffset : scroll.HorizontalOffset;
        foreach ((UIElement element, StickyRole role) in parts)
        {
            switch (role)
            {
                case StickyRole.Start:
                    element.Translation = new Vector3((float)StickyMath.StartTranslation(position), 0, 0);
                    break;
                case StickyRole.End:
                    element.Translation = new Vector3((float)StickyMath.EndTranslation(position, min, max, rtl), 0, 0);
                    break;
                case StickyRole.Header:
                    element.Translation = new Vector3((float)StickyMath.HeaderTranslation(position), 0, 0);
                    break;
                case StickyRole.ScrollCells:
                    if (ElementCompositionPreview.GetElementVisual(element).Clip is InsetClip clip)
                    {
                        (double left, double right) = StickyMath.ScrollClipInsets(position, min, max, fs, fe, rtl);
                        clip.LeftInset = (float)left;
                        clip.RightInset = (float)right;
                    }

                    break;
                case StickyRole.EdgeStart:
                    element.Opacity = StickyMath.EdgeOpacityStart(position, min, max, rtl);
                    break;
                case StickyRole.EdgeEnd:
                    element.Opacity = StickyMath.EdgeOpacityEnd(position, min, max, rtl);
                    break;
            }
        }
    }

    private void OnStateChanged()
    {
        if (Mode != StickyMode.Static)
        {
            return;
        }

        bool moving = scroll.State != ScrollingInteractionState.Idle;
        if (moving == animating)
        {
            return;
        }

        animating = moving;
        foreach ((UIElement element, StickyRole role) in parts)
        {
            if (moving)
            {
                Begin(element, role);
            }
            else
            {
                Mirror();
                End(element, role);
            }
        }
    }

    private void BuildAnimations()
    {
        CompositionPropertySet? sources = scroll.ExpressionAnimationSources;
        Compositor? maker = compositor;
        CompositionPropertySet? own = layoutProperties;
        if (sources is null || maker is null || own is null)
        {
            Problems.Add("the scroll view gave no property set of its own");
            return;
        }

        ExpressionAnimation Make(string expression)
        {
            ExpressionAnimation animation = maker.CreateExpressionAnimation(expression);
            animation.SetReferenceParameter("scroll", sources);
            animation.SetReferenceParameter("layout", own);
            return animation;
        }

        startAnimation = Make(StickyMath.StartExpression(rtl));
        endAnimation = Make(StickyMath.EndExpression(rtl));
        headerAnimation = Make(StickyMath.HeaderExpression());
        (string left, string right) = StickyMath.ClipExpressions(rtl);
        clipLeftAnimation = Make(left);
        clipRightAnimation = Make(right);
        (string edgeStart, string edgeEnd) = StickyMath.EdgeExpressions(rtl);
        edgeStartAnimation = Make(edgeStart);
        edgeEndAnimation = Make(edgeEnd);
    }

    private void Begin(UIElement element, StickyRole role)
    {
        if (Mode == StickyMode.UiThread || compositor is null || (Mode == StickyMode.Static && !animating))
        {
            PrepareClip(element, role);
            return;
        }

        if (startAnimation is null || endAnimation is null || headerAnimation is null || clipLeftAnimation is null
            || clipRightAnimation is null || edgeStartAnimation is null || edgeEndAnimation is null)
        {
            PrepareClip(element, role);
            return;
        }

        Visual visual = ElementCompositionPreview.GetElementVisual(element);
        switch (role)
        {
            case StickyRole.Start:
                visual.StartAnimation("Translation.X", startAnimation);
                break;
            case StickyRole.End:
                visual.StartAnimation("Translation.X", endAnimation);
                break;
            case StickyRole.Header:
                visual.StartAnimation("Translation.X", headerAnimation);
                break;
            case StickyRole.ScrollCells:
                {
                    InsetClip clip = PrepareClip(element, role)!;
                    clip.StartAnimation("LeftInset", clipLeftAnimation);
                    clip.StartAnimation("RightInset", clipRightAnimation);
                    break;
                }

            case StickyRole.EdgeStart:
                visual.StartAnimation("Opacity", edgeStartAnimation);
                break;
            case StickyRole.EdgeEnd:
                visual.StartAnimation("Opacity", edgeEndAnimation);
                break;
        }
    }

    private void End(UIElement element, StickyRole role)
    {
        Visual visual = ElementCompositionPreview.GetElementVisual(element);
        switch (role)
        {
            case StickyRole.Start:
            case StickyRole.End:
            case StickyRole.Header:
                visual.StopAnimation("Translation.X");
                break;
            case StickyRole.ScrollCells:
                if (visual.Clip is InsetClip clip)
                {
                    clip.StopAnimation("LeftInset");
                    clip.StopAnimation("RightInset");
                }

                break;
            case StickyRole.EdgeStart:
            case StickyRole.EdgeEnd:
                visual.StopAnimation("Opacity");
                break;
        }
    }

    // The clip is on the element's visual, made once; in the modes that write numbers it is written by Mirror.
    private InsetClip? PrepareClip(UIElement element, StickyRole role)
    {
        if (role != StickyRole.ScrollCells || compositor is null)
        {
            return null;
        }

        Visual visual = ElementCompositionPreview.GetElementVisual(element);
        if (visual.Clip is not InsetClip clip)
        {
            clip = compositor.CreateInsetClip();
            visual.Clip = clip;
        }

        return clip;
    }
}
