using Navree.Runtime;

namespace Navree.Platform;

/// <summary>
/// How the windows present a locked cover over the app: a sign-in a session's end led to, or the waiting cover the core holds
/// after a sign-in until the shell has named who signed in.
/// </summary>
/// <param name="IsCovered">Whether a locked cover is up over the app.</param>
/// <param name="CoverScreenId">The screen shown across the whole main window, when one is.</param>
/// <param name="CanGoBack">Whether back leads to an earlier step of the cover itself (or a modal opened over it).</param>
/// <param name="UnlockedModalAbove">The front screen of an unlocked modal opened over the cover, when there is one.</param>
/// <param name="BlankSecondaryWindow">Whether windows of one screen must blank what they show until the cover goes.</param>
public sealed record CoverLayout(
    bool IsCovered,
    long? CoverScreenId,
    bool CanGoBack,
    long? UnlockedModalAbove,
    bool BlankSecondaryWindow);

/// <summary>
/// What a locked cover hides and allows across the app's windows and web views. A locked modal is never shown as a closable dialog
/// or a side pane: it takes the whole main window with an opaque background, hides the navigation pane, title-bar search and account
/// menu, offers neither a close button nor Escape dismissal, blanks secondary windows, and closes any web view under it.
/// </summary>
public static class CoverRules
{
    /// <summary>Whether any modal in the state is locked.</summary>
    /// <param name="state">The state.</param>
    /// <returns>Whether a locked cover is up.</returns>
    public static bool IsCovered(ViewState state)
    {
        ArgumentNullException.ThrowIfNull(state);
        return LastLockedIndex(state.Nav.Modals) >= 0;
    }

    /// <summary>Whether a screen belongs to a locked modal, and so is drawn as the window's cover rather than in a dialog or pane.</summary>
    /// <param name="state">The state.</param>
    /// <param name="screenId">The screen's number.</param>
    /// <returns>Whether a locked modal holds it.</returns>
    public static bool IsLockedModalScreen(ViewState state, long screenId)
    {
        ArgumentNullException.ThrowIfNull(state);
        return state.Nav.Modals.Any(modal => modal.Locked && modal.Stack.Contains(screenId));
    }

    /// <summary>Decides how the windows present the state when a locked cover may be up.</summary>
    /// <param name="state">The state.</param>
    /// <returns>The cover layout.</returns>
    public static CoverLayout Decide(ViewState state)
    {
        ArgumentNullException.ThrowIfNull(state);
        IReadOnlyList<Modal> modals = state.Nav.Modals;
        int lockedAt = LastLockedIndex(modals);
        if (lockedAt < 0)
        {
            return new CoverLayout(
                IsCovered: false,
                CoverScreenId: null,
                CanGoBack: state.Nav.CanGoBack,
                UnlockedModalAbove: null,
                BlankSecondaryWindow: false);
        }

        Modal cover = modals[lockedAt];
        long? coverScreen = cover.Stack.Count > 0 ? cover.Stack[^1] : null;
        long? unlockedAbove = null;
        bool backAbove = false;
        for (int index = lockedAt + 1; index < modals.Count; index++)
        {
            Modal above = modals[index];
            if (above.Stack.Count > 0)
            {
                backAbove = true;
                if (state.Nav.Front is long front && above.Stack.Contains(front))
                {
                    unlockedAbove = front;
                }
            }
        }

        return new CoverLayout(
            IsCovered: true,
            CoverScreenId: coverScreen,
            CanGoBack: cover.Stack.Count > 1 || backAbove,
            UnlockedModalAbove: unlockedAbove,
            BlankSecondaryWindow: true);
    }

    /// <summary>
    /// Whether a screen may keep a live web view in this state. A web view is kept only while the app is ready, the screen is still
    /// held with web content, and no locked cover stands over it (a web sign-in page that is itself the front of the locked cover is
    /// allowed until that cover turns blank or closes).
    /// </summary>
    /// <param name="state">The state.</param>
    /// <param name="screenId">The screen's number.</param>
    /// <returns>Whether a live web view may stand for that screen.</returns>
    public static bool AllowsWeb(ViewState state, long screenId)
    {
        ArgumentNullException.ThrowIfNull(state);
        if (state.Nav.Stage != Stage.Ready
            || !state.Screens.TryGetValue(screenId, out ScreenView? screen)
            || screen.Content is not WebContent)
        {
            return false;
        }

        IReadOnlyList<Modal> modals = state.Nav.Modals;
        int lockedAt = LastLockedIndex(modals);
        if (lockedAt < 0)
        {
            return state.Nav.Held().Contains(screenId);
        }

        Modal cover = modals[lockedAt];
        if (cover.Stack.Count > 0 && cover.Stack[^1] == screenId)
        {
            return true;
        }

        for (int index = lockedAt + 1; index < modals.Count; index++)
        {
            Modal above = modals[index];
            if (above.Stack.Count > 0 && above.Stack[^1] == screenId)
            {
                return true;
            }
        }

        return false;
    }

    private static int LastLockedIndex(IReadOnlyList<Modal> modals)
    {
        for (int index = modals.Count - 1; index >= 0; index--)
        {
            if (modals[index].Locked)
            {
                return index;
            }
        }

        return -1;
    }
}
