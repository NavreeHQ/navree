using System.Numerics;
using Microsoft.UI.Composition;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Hosting;
using Navree.Design.Tables;

namespace Navree.Controls;

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
/// sideways: they are moved on the compositor by expressions over the scroll view's own position.
/// While the animations run nothing is written to the elements, and pointer hits and
/// accessibility rectangles follow the animation. Until the animations can be made, which waits for
/// the scroll view's property set, the same numbers are written to the elements.
/// </summary>
internal sealed partial class StickyCoordinator
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
    private bool built;
    private bool attached;

    /// <summary>Makes a coordinator for a scroll view.</summary>
    /// <param name="scroll">The scroll view the rows are in.</param>
    internal StickyCoordinator(ScrollView scroll)
    {
        this.scroll = scroll;
    }

    // A note for a measuring run to read; a build without the measuring symbol drops every call and what it would have written.
    partial void Note(string line);

    /// <summary>Whether the animations have been made.</summary>
    internal bool Built => built;

    /// <summary>The number of parts being kept in place.</summary>
    internal int Count => parts.Count;

    /// <summary>Joins the scroll view's changes. Call when the scroll view is loaded.</summary>
    /// <param name="rightToLeft">Whether the flow is right to left.</param>
    internal void Attach(bool rightToLeft)
    {
        compositor = ElementCompositionPreview.GetElementVisual(scroll).Compositor;
        layoutProperties = compositor.CreatePropertySet();
        layoutProperties.InsertScalar("Fs", (float)fs);
        layoutProperties.InsertScalar("Fe", (float)fe);
        rtl = rightToLeft;
        attached = true;
        scroll.ViewChanged += (_, _) => Mirror();
        scroll.ExtentChanged += (_, _) => OnExtentChanged();
        scroll.SizeChanged += (_, _) => OnExtentChanged();
        foreach ((UIElement element, StickyRole role) in parts)
        {
            EnableTranslation(element, role);
            PrepareClip(element, role);
        }

        Note(TryBuild()
            ? "the animations were made when the coordinator was attached"
            : "the scroll view had no property set when the coordinator was attached; it is asked again at every change of its size and extent");

        Mirror();
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
        if (attached)
        {
            EnableTranslation(element, role);
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

    /// <summary>
    /// Writes the numbers to the elements as the scroll view stands now. It is not done while the
    /// animations run: a number written to an element's translation is added to what an animation
    /// of the same translation gives, so the element would be moved twice.
    /// </summary>
    internal void Mirror()
    {
        if (!attached || built)
        {
            return;
        }

        double reach = Math.Max(0, scroll.ScrollableWidth);
        (double min, double max) = rtl ? (-reach, 0.0) : (0.0, reach);
        double position = rtl ? -scroll.HorizontalOffset : scroll.HorizontalOffset;
        foreach ((UIElement element, StickyRole role) in parts)
        {
            switch (role)
            {
                case StickyRole.Start:
                    element.Translation = new Vector3((float)StickyMath.StartTranslation(position, rtl), 0, 0);
                    break;
                case StickyRole.End:
                    element.Translation = new Vector3((float)StickyMath.EndTranslation(position, min, max, rtl), 0, 0);
                    break;
                case StickyRole.Header:
                    element.Translation = new Vector3((float)StickyMath.HeaderTranslation(position, rtl), 0, 0);
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

    private static void EnableTranslation(UIElement element, StickyRole role)
    {
        if (role is StickyRole.Start or StickyRole.End or StickyRole.Header)
        {
            ElementCompositionPreview.SetIsTranslationEnabled(element, true);
        }
    }

    private void OnExtentChanged()
    {
        if (!built && TryBuild())
        {
            Note("the animations were made when the scroll view's size or extent changed, at width " + scroll.ExtentWidth.ToString("0", System.Globalization.CultureInfo.InvariantCulture));
        }

        Mirror();
    }

    // Makes the animations once the scroll view has its property set, and starts them on every part kept.
    private bool TryBuild()
    {
        if (built)
        {
            return true;
        }

        if (!attached || !BuildAnimations())
        {
            return false;
        }

        built = true;
        foreach ((UIElement element, StickyRole role) in parts)
        {
            Begin(element, role);
        }

        return true;
    }

    private bool BuildAnimations()
    {
        CompositionPropertySet? sources = scroll.ExpressionAnimationSources;
        Compositor? maker = compositor;
        CompositionPropertySet? own = layoutProperties;
        if (sources is null || maker is null || own is null)
        {
            return false;
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
        headerAnimation = Make(StickyMath.HeaderExpression(rtl));
        (string left, string right) = StickyMath.ClipExpressions(rtl);
        clipLeftAnimation = Make(left);
        clipRightAnimation = Make(right);
        (string edgeStart, string edgeEnd) = StickyMath.EdgeExpressions(rtl);
        edgeStartAnimation = Make(edgeStart);
        edgeEndAnimation = Make(edgeEnd);
        return true;
    }

    private void Begin(UIElement element, StickyRole role)
    {
        if (compositor is null || startAnimation is null || endAnimation is null || headerAnimation is null || clipLeftAnimation is null
            || clipRightAnimation is null || edgeStartAnimation is null || edgeEndAnimation is null)
        {
            PrepareClip(element, role);
            return;
        }

        Visual visual = ElementCompositionPreview.GetElementVisual(element);
        if (role is StickyRole.Start or StickyRole.End or StickyRole.Header)
        {
            // What was written before is added to what the animation gives, so it is taken away first.
            element.Translation = Vector3.Zero;
        }

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

    // The clip is on the element's visual, made once; until the animations are made it is written by Mirror.
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
