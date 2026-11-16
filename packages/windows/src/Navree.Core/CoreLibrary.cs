using System.Runtime.InteropServices;

namespace Navree.Core;

/// <summary>The app could not have its core: the library is not there, cannot be loaded, or is for another processor. The message says which, in words.</summary>
public sealed class CoreUnavailableException : Exception
{
    /// <summary>Makes the exception.</summary>
    public CoreUnavailableException()
    {
    }

    /// <summary>Makes the exception.</summary>
    /// <param name="message">What happened, in words for the person.</param>
    public CoreUnavailableException(string message)
        : base(message)
    {
    }

    /// <summary>Makes the exception.</summary>
    /// <param name="message">What happened, in words for the person.</param>
    /// <param name="innerException">What the system said.</param>
    public CoreUnavailableException(string message, Exception innerException)
        : base(message, innerException)
    {
    }
}

/// <summary>Where the core's library is looked for, and what to say when it cannot be had.</summary>
public static class CoreLibrary
{
    /// <summary>The library's file.</summary>
    public const string FileName = "navree_native_core.dll";

    /// <summary>
    /// The one folder the library is looked for in: the folder of this assembly (<see cref="DllImportSearchPath.AssemblyDirectory"/> alone,
    /// so the system's search is not used for it).
    /// </summary>
    public static string Folder => Path.GetDirectoryName(typeof(CoreLibrary).Assembly.Location) ?? AppContext.BaseDirectory;

    /// <summary>
    /// The words for a core that could not be had, naming the file and where it was looked for; null when the problem is not about the library
    /// (a configuration the core refused, which is the app's own fault and says so itself).
    /// </summary>
    /// <param name="problem">What making the core threw.</param>
    /// <param name="folder">Where the library was looked for.</param>
    /// <returns>The words, or null.</returns>
    public static string? Explain(Exception problem, string folder)
    {
        ArgumentNullException.ThrowIfNull(problem);
        string file = Path.Combine(folder, FileName);
        return problem switch
        {
            DllNotFoundException when File.Exists(file) =>
                "The app cannot start: its core, " + file + ", is there but could not be loaded. A library it needs may be missing (the Microsoft Visual C++ runtime).",
            DllNotFoundException =>
                "The app cannot start: its core, " + FileName + ", was not found. It was looked for in " + folder + " and nowhere else.",
            BadImageFormatException =>
                "The app cannot start: its core, " + file + ", is built for another processor than this app, which runs as " + RuntimeInformation.ProcessArchitecture + ".",
            EntryPointNotFoundException =>
                "The app cannot start: its core, " + file + ", is not the version this app was built with (a function it needs is missing).",
            _ => null,
        };
    }
}
