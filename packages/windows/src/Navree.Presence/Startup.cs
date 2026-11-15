using Microsoft.Win32;

namespace Navree.Presence;

/// <summary>Whether the app opens when the person signs in to Windows.</summary>
public enum StartupState
{
    /// <summary>It does not.</summary>
    Off,

    /// <summary>It does.</summary>
    On,

    /// <summary>The person turned it off in Windows' own settings: only the person can turn it on again there.</summary>
    DisabledByUser,

    /// <summary>The person's organisation decides.</summary>
    ByPolicy,
}

/// <summary>Opening at sign-in, as plain rules.</summary>
public static class StartupRules
{
    /// <summary>The longest a value of the system's list of programs to run at sign-in may be.</summary>
    public const int LongestCommand = 259;

    /// <summary>The command the list holds: the program's path in quotes and the flag that says it was started by the system, not the person.</summary>
    /// <param name="exePath">The program's path.</param>
    /// <returns>The command, or null when it would not fit the list.</returns>
    public static string? Command(string exePath)
    {
        if (string.IsNullOrWhiteSpace(exePath) || exePath.Contains('"'))
        {
            return null;
        }

        string command = "\"" + exePath + "\" " + LaunchArguments.Background;
        return command.Length <= LongestCommand ? command : null;
    }

    /// <summary>Whether a value of the list is this program's own.</summary>
    /// <param name="value">What the list holds.</param>
    /// <param name="exePath">The program's path.</param>
    /// <returns>Whether it is.</returns>
    public static bool IsOurs(string? value, string exePath) =>
        value is not null && value.StartsWith("\"" + exePath + "\"", StringComparison.OrdinalIgnoreCase);

    /// <summary>
    /// Whether the person turned an entry off in Windows' settings, from the bytes Windows keeps for it: the first byte is even when the entry
    /// is enabled (2 or 6) and odd when it is disabled (3 and up).
    /// </summary>
    /// <param name="approval">The bytes, or null when Windows keeps none (it is on).</param>
    /// <returns>Whether it is turned off.</returns>
    public static bool IsDisabled(byte[]? approval) => approval is { Length: > 0 } && (approval[0] & 1) == 1;

    /// <summary>The state of a startup task of a package, from the system's own numbers (<c>StartupTaskState</c>).</summary>
    /// <param name="state">0 disabled, 1 disabled by the user, 2 enabled, 3 disabled by policy, 4 enabled by policy.</param>
    /// <returns>The state.</returns>
    public static StartupState FromTask(int state) =>
        state switch
        {
            2 => StartupState.On,
            1 => StartupState.DisabledByUser,
            3 or 4 => StartupState.ByPolicy,
            _ => StartupState.Off,
        };

    /// <summary>Whether the app may turn it on or off itself, or the person has to, in Windows' own settings.</summary>
    /// <param name="state">The state.</param>
    /// <returns>Whether the app may.</returns>
    public static bool AppMayChange(StartupState state) => state is StartupState.Off or StartupState.On;
}

/// <summary>
/// This person's list of programs to run at sign-in (the <c>Run</c> key of the current user), for a build without package identity. The
/// person can still turn an entry off in Windows' settings, which is kept apart (<c>StartupApproved</c>) and is read, not changed.
/// </summary>
public sealed class RunKey
{
    /// <summary>Where the system's list is.</summary>
    public const string RunPath = @"Software\Microsoft\Windows\CurrentVersion\Run";

    /// <summary>Where the system keeps what the person turned off.</summary>
    public const string ApprovedPath = @"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

    private readonly string name;
    private readonly string runPath;
    private readonly string approvedPath;

    /// <summary>Opens the list for one entry.</summary>
    /// <param name="name">The entry's name, the app's own.</param>
    /// <param name="runPath">The key, the system's own unless a test gives its own.</param>
    /// <param name="approvedPath">The key of the person's choices, the system's own unless a test gives its own.</param>
    public RunKey(string name, string runPath = RunPath, string approvedPath = ApprovedPath)
    {
        ArgumentException.ThrowIfNullOrEmpty(name);
        this.name = name;
        this.runPath = runPath;
        this.approvedPath = approvedPath;
    }

    /// <summary>What the entry is now.</summary>
    /// <param name="exePath">The program's path.</param>
    /// <returns>The state.</returns>
    public StartupState State(string exePath)
    {
        using RegistryKey? run = Registry.CurrentUser.OpenSubKey(runPath);
        if (!StartupRules.IsOurs(run?.GetValue(name) as string, exePath))
        {
            return StartupState.Off;
        }

        using RegistryKey? approved = Registry.CurrentUser.OpenSubKey(approvedPath);
        return StartupRules.IsDisabled(approved?.GetValue(name) as byte[]) ? StartupState.DisabledByUser : StartupState.On;
    }

    /// <summary>Puts the entry in the list, or takes it out.</summary>
    /// <param name="exePath">The program's path.</param>
    /// <param name="on">Whether it opens at sign-in.</param>
    /// <returns>Whether the list now says what was asked.</returns>
    public bool Set(string exePath, bool on)
    {
        try
        {
            if (on)
            {
                string? command = StartupRules.Command(exePath);
                if (command is null)
                {
                    return false;
                }

                using RegistryKey run = Registry.CurrentUser.CreateSubKey(runPath);
                run.SetValue(name, command, RegistryValueKind.String);
                return true;
            }

            using RegistryKey? existing = Registry.CurrentUser.OpenSubKey(runPath, writable: true);
            existing?.DeleteValue(name, throwOnMissingValue: false);
            return true;
        }
        catch (Exception problem) when (problem is UnauthorizedAccessException or System.Security.SecurityException or IOException)
        {
            return false;
        }
    }
}
