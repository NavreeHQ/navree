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

    /// <summary>
    /// Another person than the one named before, with nobody signed out between; or the first person named at a start that found something
    /// kept and nothing that says whom it was kept for.
    /// </summary>
    Another,
}

/// <summary>
/// Who the core last said is signed in, so that a change of person nobody signed out for is seen. A session can run out, or be ended by a
/// server that says nothing of it to the client, and somebody else can then sign in: the core empties what it holds, and no effect says so.
/// What the client keeps of its own for a person (what the web view keeps, the notifications that wait and those shown, the count, the jump
/// list, the files chosen for forms, the pictures) is the client's to take away then, and this is how it knows when.
/// </summary>
/// <remarks>
/// <para>
/// A person is told from another by the key the core gives for them (<c>app.user.key</c>): the same for one person and another for another
/// account, made by the core from the id the server names the person by. Two accounts of one name and one email are two people by it, and an
/// account whose name or email changes is still one. Nothing else is compared when a key is given. A core that gives no key is an older one:
/// then, and only then, a person is told by name and email, which is as far as a client can tell by itself.
/// </para>
/// <para>
/// The two ways of telling are never held to each other. A person told by a key and a person told by name and email are two people,
/// whichever came first and whatever their names and emails are: a name and an email say nothing of which account it is, so nothing of
/// the one is kept for the other.
/// </para>
/// <para>
/// A state that names nobody changes nothing: the person named before may sign in again and find everything as it was. A sign-out forgets
/// who was named (<see cref="SignedOut"/>), since it takes everything away itself, so whoever is named after one is the first.
/// </para>
/// <para>
/// What is remembered is a mark (<see cref="MarkOfKey"/>, or <see cref="MarkOf"/> with no key): it tells two people apart and says nothing
/// of either, so it is kept for the next start, which then sees a change of person that happened across a restart. A mark of one kind is
/// never the mark of the other kind, so a mark kept by name and email is another person's when the core gives a key, and a key kept is
/// another person's when the core gives none. A start that finds something kept and no mark cannot say whose it is, and takes the first
/// person named for another. It is used from one thread, the window's.
/// </para>
/// </remarks>
public sealed class PersonWatch
{
    private const string KeyPrefix = "key:";
    private string? named;
    private bool unknownBefore;

    /// <summary>Makes the watch.</summary>
    /// <param name="remembered">The mark of the person the last run named last, as it was kept; null when there is none.</param>
    /// <param name="somethingKept">
    /// Whether something of a person is kept on the device though no mark says whose (the mark was never written, or cannot be read): the
    /// first person named is then taken for another than whoever it was kept for.
    /// </param>
    public PersonWatch(string? remembered = null, bool somethingKept = false)
    {
        named = string.IsNullOrEmpty(remembered) ? null : remembered;
        unknownBefore = named is null && somethingKept;
    }

    /// <summary>The mark of the person named last; null when nobody was named since the start or the last sign-out.</summary>
    public string? Named => named;

    /// <summary>Hears who a state names as signed in.</summary>
    /// <param name="key">The core's key for the person; null or empty from a core that gives none.</param>
    /// <param name="name">The person's name; null when the state names nobody.</param>
    /// <param name="email">Their email, when they have one.</param>
    /// <returns>Whether they are the first named, the one named before, or another person with nobody signed out between.</returns>
    public PersonHeard Hear(string? key, string? name, string? email)
    {
        if (name is null)
        {
            return PersonHeard.Nobody;
        }

        string now = string.IsNullOrEmpty(key) ? MarkOf(name, email) : MarkOfKey(key);
        if (named is not string before)
        {
            named = now;
            if (unknownBefore)
            {
                // Something was kept at the start and nothing said whose: whoever this is, it is not known to be theirs.
                unknownBefore = false;
                return PersonHeard.Another;
            }

            return PersonHeard.First;
        }

        if (string.Equals(before, now, StringComparison.Ordinal))
        {
            return PersonHeard.Same;
        }

        // Any other mark is another person. That holds across the two kinds of mark as well: a mark made from a name and an email beside
        // a key, or a key beside a state that gives none, cannot be shown to be one person, and a name and an email two accounts share
        // would make them one. So nothing of the one is kept for the other.
        named = now;
        return PersonHeard.Another;
    }

    /// <summary>Somebody signed out: everything of theirs is taken away by the sign-out, and whoever is named next is the first.</summary>
    public void SignedOut()
    {
        named = null;
        unknownBefore = false;
    }

    /// <summary>
    /// The mark of a person the core gave a key for: the key, said to be one, so it is never the mark <see cref="MarkOf"/> makes from a
    /// name and an email. Nothing of the person can be read from it.
    /// </summary>
    /// <param name="key">The core's key for the person (<c>app.user.key</c>).</param>
    /// <returns>The mark.</returns>
    public static string MarkOfKey(string key)
    {
        ArgumentException.ThrowIfNullOrEmpty(key);
        return KeyPrefix + key;
    }

    /// <summary>
    /// The mark of a person the core gave no key for: the same for the same name and email, another for any other, and neither can be read
    /// from it. A person with no email and one whose email is empty are two.
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
