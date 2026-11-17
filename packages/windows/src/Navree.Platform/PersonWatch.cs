using System.Globalization;
using System.Security.Cryptography;
using System.Text;

namespace Navree.Platform;

/// <summary>What hearing who a state names came to.</summary>
public enum PersonHeard
{
    /// <summary>The state names nobody: it says nothing of who was here or who comes next.</summary>
    Nobody,

    /// <summary>The first person named since the app was first started, or since somebody last signed out.</summary>
    First,

    /// <summary>The person who was named before.</summary>
    Same,

    /// <summary>Another person than the one named before, with nobody signed out between.</summary>
    Another,
}

/// <summary>
/// Who the core last said is signed in, so that a change of person nobody signed out for is seen. A session can run out, or be ended by a
/// server that says nothing of it to the client, and somebody else can then sign in: the core empties what it holds and has its cache
/// emptied, and says no more than that. What the client keeps of its own for a person (what the web view keeps, the notifications that
/// wait and those shown, the count, the jump list, the files chosen for forms, the pictures) is the client's to take away then, and this
/// is how it knows when.
/// </summary>
/// <remarks>
/// <para>
/// Two people are told apart as far as a client can tell them: by name and email. A state that names nobody changes nothing: the person
/// named before may sign in again and find everything as it was. A sign-out forgets who was named (<see cref="SignedOut"/>), since it
/// takes everything away itself, so whoever is named after one is the first.
/// </para>
/// <para>
/// What is remembered is a mark (<see cref="MarkOf"/>): it tells two people apart and says nothing of either, so it can be kept for the
/// next start, which then sees a change of person that happened across a restart. It is used from one thread, the window's.
/// </para>
/// </remarks>
public sealed class PersonWatch
{
    private string? named;

    /// <summary>Makes the watch.</summary>
    /// <param name="remembered">The mark of the person the last run named last, as it was kept; null when there is none.</param>
    public PersonWatch(string? remembered = null) => named = string.IsNullOrEmpty(remembered) ? null : remembered;

    /// <summary>The mark of the person named last; null when nobody was named since the start or the last sign-out.</summary>
    public string? Named => named;

    /// <summary>Hears who a state names as signed in.</summary>
    /// <param name="name">The person's name; null when the state names nobody.</param>
    /// <param name="email">Their email, when they have one.</param>
    /// <returns>Whether they are the first named, the one named before, or another person with nobody signed out between.</returns>
    public PersonHeard Hear(string? name, string? email)
    {
        if (name is null)
        {
            return PersonHeard.Nobody;
        }

        string now = MarkOf(name, email);
        if (named is null)
        {
            named = now;
            return PersonHeard.First;
        }

        if (string.Equals(named, now, StringComparison.Ordinal))
        {
            return PersonHeard.Same;
        }

        named = now;
        return PersonHeard.Another;
    }

    /// <summary>Somebody signed out: everything of theirs is taken away by the sign-out, and whoever is named next is the first.</summary>
    public void SignedOut() => named = null;

    /// <summary>
    /// The mark of a person: the same for the same name and email, another for any other, and nothing of either can be read from it.
    /// A person with no email and one whose email is empty are two.
    /// </summary>
    /// <param name="name">The person's name.</param>
    /// <param name="email">Their email, when they have one.</param>
    /// <returns>The mark.</returns>
    public static string MarkOf(string name, string? email)
    {
        ArgumentNullException.ThrowIfNull(name);
        string written = name.Length.ToString(CultureInfo.InvariantCulture) + ":" + name + (email is null ? "-" : "+" + email);
        return Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(written)));
    }
}
