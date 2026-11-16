namespace Navree.Tests;

/// <summary>Finds the files the protocol and the other clients keep in the repository: examples, recorded answers, vectors.</summary>
public static class Repository
{
    /// <summary>The repository's root: the folder above <c>packages</c> that holds the workspace's <c>Cargo.toml</c>.</summary>
    public static string Root { get; } = FindRoot();

    /// <summary>The JSON files in a folder of the repository, by path.</summary>
    public static string[] Json(string folder)
    {
        string[] files = Directory.GetFiles(Path.Combine(Root, folder), "*.json");
        Array.Sort(files, StringComparer.Ordinal);
        return files;
    }

    private static string FindRoot()
    {
        DirectoryInfo? folder = new(AppContext.BaseDirectory);
        while (folder is not null)
        {
            if (File.Exists(Path.Combine(folder.FullName, "Cargo.toml"))
                && Directory.Exists(Path.Combine(folder.FullName, "crates", "navree-native")))
            {
                return folder.FullName;
            }

            folder = folder.Parent;
        }

        throw new DirectoryNotFoundException("The repository's root was not found above " + AppContext.BaseDirectory);
    }
}
