namespace Navree.Gallery;

/// <summary>What the command line asked the gallery to do.</summary>
public sealed class GalleryArguments
{
    /// <summary>The scenarios to run, in order.</summary>
    public IReadOnlyList<string> Run { get; private init; } = [];

    /// <summary>The folder the run writes its measurements and handshake files into, or null.</summary>
    public string? Out { get; private init; }

    /// <summary>Reads the command line.</summary>
    /// <param name="arguments">The arguments, program first.</param>
    /// <returns>What was asked.</returns>
    public static GalleryArguments Read(string[] arguments)
    {
        string? run = null;
        string? output = null;
        for (int i = 0; i + 1 < arguments.Length; i++)
        {
            if (arguments[i] == "--run")
            {
                run = arguments[i + 1];
            }
            else if (arguments[i] == "--out")
            {
                output = arguments[i + 1];
            }
        }

        return new GalleryArguments
        {
            Run = run is null ? [] : run.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries),
            Out = output,
        };
    }
}

/// <summary>Writes what a run measured, one line at a time, to a file and to the debugger.</summary>
internal static class Report
{
    private static string? file;

    /// <summary>The folder the report is in, or null.</summary>
    internal static string? Folder { get; private set; }

    /// <summary>Opens the report in a folder.</summary>
    /// <param name="folder">The folder, or null for none.</param>
    internal static void Open(string? folder)
    {
        if (folder is null)
        {
            return;
        }

        Directory.CreateDirectory(folder);
        Folder = folder;
        file = Path.Combine(folder, "results.txt");
    }

    /// <summary>Writes a line.</summary>
    /// <param name="line">The line.</param>
    internal static void Line(string line)
    {
        System.Diagnostics.Debug.WriteLine(line);
        if (file is not null)
        {
            File.AppendAllText(file, line + Environment.NewLine);
        }
    }
}

/// <summary>
/// Lets a script take a picture at a moment the run chooses: the run writes a file that says it is
/// ready and waits for a file that says the picture was taken.
/// </summary>
internal static class Handshake
{
    private static string? folder;

    /// <summary>Sets the folder the files go in.</summary>
    /// <param name="directory">The folder, or null for none (no picture is waited for).</param>
    internal static void Open(string? directory) => folder = directory;

    /// <summary>Says the window is ready to be pictured, and waits until it was or twenty seconds went by.</summary>
    /// <param name="id">A name for the picture.</param>
    /// <param name="details">What the script should know about it.</param>
    /// <param name="wait">Whether to wait for the picture.</param>
    /// <returns>A task that ends when the picture was taken.</returns>
    internal static async Task StepAsync(string id, string details, bool wait = true)
    {
        if (folder is null)
        {
            return;
        }

        File.WriteAllText(Path.Combine(folder, id + ".ready"), details);
        if (!wait)
        {
            return;
        }

        string taken = Path.Combine(folder, id + ".taken");
        for (int i = 0; i < 200 && !File.Exists(taken); i++)
        {
            await Task.Delay(100);
        }

        if (!File.Exists(taken))
        {
            Report.Line("no picture was taken for " + id);
        }
    }
}
