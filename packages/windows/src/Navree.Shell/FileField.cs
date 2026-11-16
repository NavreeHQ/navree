using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Navree.Design.Inputs;
using Navree.Platform;
using Navree.Protocol;
using Navree.Runtime;
using Windows.ApplicationModel.DataTransfer;
using Windows.Storage;

namespace Navree.Shell;

/// <summary>
/// A file field: the label, a Choose button, then each file chosen with its name and size and a button to take it out. Files are chosen through the
/// system's picker (or dropped on the field); each is checked for its kind and for the most bytes of the field together, and a photograph has the place
/// it was taken taken out unless the field keeps it. The field holds nothing the core does not hold except what it has just sent, so it stands across an
/// answer like the text fields do: the same control, with what the person chose still in it.
/// </summary>
internal sealed class FileFieldView
{
    // How long what was just sent is shown while the core's answer has not come back to say it holds it.
    private static readonly TimeSpan Waiting = TimeSpan.FromSeconds(3);

    private readonly RenderContext context;
    private readonly Models.FileInput input;
    private readonly string label;
    private readonly Func<bool> disabled;
    private readonly Func<bool> busy;
    private readonly Action<FieldValue> send;
    private readonly Border box;
    private readonly Button choose;
    private readonly StackPanel rows = new() { Spacing = 6 };
    private readonly TextBlock refusal;
    private IReadOnlyList<FileRef> core;
    private IReadOnlyList<FileRef>? sent;
    private DateTime sentAt;
    private bool picking;
    private string drawnShape = string.Empty;

    internal FileFieldView(RenderContext context, Models.FileInput input, string header, string label, FieldView? held, Func<bool> disabled, Func<bool> busy, Action<FieldValue> send)
    {
        this.context = context;
        this.input = input;
        this.label = label;
        this.disabled = disabled;
        this.busy = busy;
        this.send = send;
        core = held?.Value is FilesValue files ? files.Files : [];

        refusal = Look.Label(string.Empty, "CaptionTextBlockStyle", tone: Models.Tone.Danger);
        refusal.Visibility = Visibility.Collapsed;
        AutomationProperties.SetLiveSetting(refusal, AutomationLiveSetting.Assertive);

        choose = new Button { Content = "Choose", MinHeight = 32 };
        AutomationProperties.SetName(choose, "Choose " + (input.Multiple ? "files" : "a file") + " for " + label);
        choose.Click += async (_, _) => await ChooseAsync();

        TextBlock hint = Look.Label(Hint(), "CaptionTextBlockStyle", muted: true);
        hint.VerticalAlignment = VerticalAlignment.Center;
        Grid top = new() { ColumnSpacing = 12 };
        top.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        top.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        Grid.SetColumn(hint, 1);
        top.Children.Add(choose);
        top.Children.Add(hint);

        StackPanel inside = new() { Spacing = 8 };
        inside.Children.Add(top);
        inside.Children.Add(rows);
        box = new Border
        {
            Child = inside,
            Padding = new Thickness(8),
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(4),
            BorderBrush = Look.Resource("ControlStrongStrokeDefaultBrush"),
            Background = Look.Resource("ControlFillColorDefaultBrush"),
            AllowDrop = true,
        };
        box.DragOver += OnDragOver;
        box.Drop += OnDrop;
        box.GotFocus += (_, _) => Render();

        Root = new StackPanel { Spacing = 4 };
        if (header.Length > 0)
        {
            Root.Children.Add(Look.Label(header, "BodyTextBlockStyle"));
        }

        Root.Children.Add(box);
        Root.Children.Add(refusal);

        // A file that went missing when a request was written is shown as gone as soon as that is known.
        Root.Loaded += (_, _) =>
        {
            if (context.Host is { } host)
            {
                host.FilesWentMissing -= OnMissing;
                host.FilesWentMissing += OnMissing;
            }

            Render();
        };
        Root.Unloaded += (_, _) =>
        {
            if (context.Host is { } host)
            {
                host.FilesWentMissing -= OnMissing;
            }
        };
        Render();
    }

    /// <summary>What the field is drawn as.</summary>
    internal StackPanel Root { get; }

    /// <summary>The button that opens the picker, for a run that presses it.</summary>
    internal Button ChooseButton => choose;

    /// <summary>The files the field shows now.</summary>
    internal IReadOnlyList<FileRef> Held => sent is not null && DateTime.UtcNow - sentAt < Waiting ? sent : core;

    private FieldRule Rule => new(input.Accept, input.Multiple, input.KeepLocation, context.Host?.Options.MostFileBytes ?? FileRules.MostBytes);

    private string Hint()
    {
        string kinds = FileRules.KindsInWords(input.Accept);
        long most = context.Host?.Options.MostFileBytes ?? FileRules.MostBytes;
        string limit = "up to " + FileRules.Size(most) + (input.Multiple ? " in all" : string.Empty);
        string first = kinds.Length == 0 ? limit : kinds + ", " + limit;
        return char.ToUpperInvariant(first[0]) + first[1..];
    }

    /// <summary>Shows what the core holds for the field, and whether the files are still there.</summary>
    /// <param name="view">What the screen holds for the field.</param>
    internal void Show(FieldView? view)
    {
        IReadOnlyList<FileRef> now = view?.Value is FilesValue files ? files.Files : [];
        if (sent is not null && (SameFiles(sent, now) || DateTime.UtcNow - sentAt >= Waiting))
        {
            sent = null;
        }

        // The core let go of files it held (a reset): they are let go of here too.
        if (sent is null && context.Host is { } host)
        {
            foreach (FileRef old in core.Where(old => now.All(file => file.Token != old.Token)))
            {
                host.Files.Release(old.Token);
            }
        }

        core = now;
        Render();
    }

    private static bool SameFiles(IReadOnlyList<FileRef> one, IReadOnlyList<FileRef> other) =>
        one.Count == other.Count && one.Zip(other).All(pair => pair.First.Token == pair.Second.Token);

    private void OnMissing(IReadOnlyList<string> tokens)
    {
        if (Held.Any(file => tokens.Contains(file.Token)))
        {
            Render();
        }
    }

    // ---- choosing and taking out ---------------------------------------------------------------------------------

    private bool Enabled => !disabled() && !busy() && context.Host is not null;

    private async Task ChooseAsync()
    {
        if (!Enabled || picking || context.Host is not { } host || choose.XamlRoot is null)
        {
            return;
        }

        // The button stays enabled while the picker is open: a button that is disabled under the focus gives the focus away to some other part of the window.
        picking = true;
        IReadOnlyList<string> paths;
        try
        {
            FilePickRequest request = new(FileRules.PickerTypes(input.Accept), input.Multiple, label);
            paths = await host.FilePicker.PickAsync(request, choose.XamlRoot.ContentIslandEnvironment.AppWindowId);
        }
        catch (Exception)
        {
            // The click's handler is the system's: nothing may escape it, or the app ends. The person is told, and can try again or drop the files.
            Say([FileRules.PickerFailedWords]);
            return;
        }
        finally
        {
            picking = false;
        }

        try
        {
            await OfferAsync(paths);
        }
        catch (Exception)
        {
            Say([FileRules.PickerFailedWords]);
        }
    }

    /// <summary>Offers files to the field, as the picker does and a drop does: each is checked, the ones that fit are added, and the others are told in words. It never throws.</summary>
    /// <param name="paths">Where the files are.</param>
    /// <param name="said">Words already to say, for things a drop could not offer.</param>
    /// <returns>When every file has been looked at and the core told.</returns>
    internal async Task OfferAsync(IEnumerable<string> paths, IReadOnlyList<string>? said = null)
    {
        if (context.Host is not { } host)
        {
            return;
        }

        IReadOnlyList<FileRef> before = Held;
        List<FileRef> list = input.Multiple ? [.. before] : [];
        long bytes = list.Sum(file => file.Size);
        List<string> words = [.. said ?? []];
        List<FileRef> added = [];

        // The session the person chose them in: a file still being looked at when that session ends is not held for whoever comes next.
        long chosenIn = host.Files.Session;
        foreach (string path in paths)
        {
            FieldRule rule = Rule;
            long total = bytes;
            Picked result;
            try
            {
                result = await Task.Run(() => host.Files.AddAsync(path, rule, total, chosenIn, CancellationToken.None));
            }
            catch (Exception)
            {
                result = new Picked(null, FileRules.DisplayName(path ?? string.Empty) + " could not be read, so it was not added.");
            }
            if (result.File is { } file)
            {
                if (!input.Multiple && added.Count > 0)
                {
                    // A field that takes one file holds the last chosen.
                    host.Files.Release(added[0].Token);
                    added.Clear();
                    list.Clear();
                    bytes = 0;
                }

                added.Add(file);
                list.Add(file);
                bytes += file.Size;
            }
            else if (result.Refusal is { } why)
            {
                words.Add(why);
            }
        }

        Say(words);
        if (added.Count > 0)
        {
            if (!input.Multiple)
            {
                foreach (FileRef replaced in before)
                {
                    host.Files.Release(replaced.Token);
                }
            }

            Commit(list);
        }
        else
        {
            Render();
        }
    }

    // The words under the row: why things were not added, or nothing.
    private void Say(IReadOnlyList<string> words)
    {
        refusal.Text = string.Join(" ", words);
        refusal.Visibility = words.Count == 0 ? Visibility.Collapsed : Visibility.Visible;
    }

    private void Remove(FileRef file)
    {
        context.Host?.Files.Release(file.Token);
        refusal.Text = string.Empty;
        refusal.Visibility = Visibility.Collapsed;
        Commit([.. Held.Where(held => held.Token != file.Token)]);
        _ = choose.Focus(FocusState.Programmatic);
    }

    private void Commit(List<FileRef> list)
    {
        sent = list;
        sentAt = DateTime.UtcNow;
        send(new FilesValue(list));
        Render();
    }

    // ---- dropping files on the field -----------------------------------------------------------------------------

    private void OnDragOver(object sender, DragEventArgs e)
    {
        if (Enabled && e.DataView.Contains(StandardDataFormats.StorageItems))
        {
            e.AcceptedOperation = DataPackageOperation.Copy;
            e.DragUIOverride.Caption = "Add to " + label;
        }
        else
        {
            e.AcceptedOperation = DataPackageOperation.None;
        }
    }

    // The system's handler of a drop: nothing may escape it, or the app ends. What has a path on this computer is offered; a folder, or a thing
    // with none (a file on a phone seen through Explorer, a link from a browser), is said in words (FileRules.Sort).
    private async void OnDrop(object sender, DragEventArgs e)
    {
        try
        {
            if (!Enabled || !e.DataView.Contains(StandardDataFormats.StorageItems))
            {
                return;
            }

            DragOperationDeferral deferral = e.GetDeferral();
            IReadOnlyList<IStorageItem> items;
            try
            {
                items = await e.DataView.GetStorageItemsAsync();
            }
            finally
            {
                deferral.Complete();
            }

            Dropped dropped = FileRules.Sort([.. items.Select(item => new DroppedItem(item.Name, item.Path, item is IStorageFolder))]);
            await OfferAsync(dropped.Paths, dropped.Words);
        }
        catch (Exception)
        {
            Say([FileRules.DropFailedWords]);
        }
    }

    // ---- drawing -------------------------------------------------------------------------------------------------

    private void Render()
    {
        IReadOnlyList<FileRef> held = Held;
        bool on = Enabled;
        choose.IsEnabled = on;

        // What is drawn is made again only when what it shows changed (a file, whether it is still there, whether it can be taken out), so the
        // button a person has the focus on is not replaced by a looking again at the same files.
        string shape = string.Join('|', held.Select(file => file.Token + (context.Host is { } at && !at.Files.Available(file.Token) ? "!" : string.Empty))) + (on ? "+" : "-");
        if (shape == drawnShape)
        {
            return;
        }

        drawnShape = shape;
        rows.Children.Clear();
        if (held.Count == 0)
        {
            rows.Children.Add(Look.Label("No file chosen", "BodyTextBlockStyle", muted: true));
        }

        foreach (FileRef file in held)
        {
            rows.Children.Add(Row(file, on));
        }

        AutomationProperties.SetName(box, label + ", " + FileRules.Count(held.Count));
    }

    private Grid Row(FileRef file, bool on)
    {
        bool gone = context.Host is { } host && !host.Files.Available(file.Token);
        Grid row = new() { ColumnSpacing = 8 };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        FontIcon icon = Look.Glyph("");
        icon.VerticalAlignment = VerticalAlignment.Top;
        icon.Margin = new Thickness(0, 4, 0, 0);
        row.Children.Add(icon);

        StackPanel words = new() { Spacing = 2 };
        words.Children.Add(Look.Label(file.Name, "BodyTextBlockStyle", keepEnd: false));
        if (gone)
        {
            StackPanel problem = new() { Orientation = Orientation.Horizontal, Spacing = 6 };
            FontIcon warning = Look.Glyph("", 14);
            warning.Foreground = Look.ToneBrush(Models.Tone.Danger);
            warning.VerticalAlignment = VerticalAlignment.Center;
            problem.Children.Add(warning);
            problem.Children.Add(Look.Label(FileRules.GoneWords, "CaptionTextBlockStyle", tone: Models.Tone.Danger));
            words.Children.Add(problem);
        }
        else
        {
            words.Children.Add(Look.Label(FileRules.Size(file.Size), "CaptionTextBlockStyle", muted: true));
        }

        Grid.SetColumn(words, 1);
        row.Children.Add(words);

        Button remove = new() { Content = Look.Glyph("", 14), MinWidth = 32, MinHeight = 32, IsEnabled = on, VerticalAlignment = VerticalAlignment.Top };
        AutomationProperties.SetName(remove, "Remove " + file.Name);
        ToolTipService.SetToolTip(remove, "Remove");
        remove.Click += (_, _) => Remove(file);
        Grid.SetColumn(remove, 2);
        row.Children.Add(remove);

        return row;
    }
}
