using System.Security.Cryptography;
using System.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Navree.Design.Inputs;
using Navree.Runtime;
using Navree.Shell;

namespace Navree.Demo;

// The protocol's own form example holds a colour field and a file field. This walk opens it, reads that it is drawn by the client and not shown in
// the web view, chooses a colour with the keyboard, and chooses files through a picker the walk supplies (it never drives the system picker's own
// window): the name and size show, a wrong kind and a file that is too large are refused with words, a file is taken out, the form is sent and the
// walk's own server reads back what came, by hash, and a file deleted before the send is shown as gone and is not sent.
internal sealed partial class PageWalk
{
    private const byte KeyA = 0x41;

    // The picker the walk supplies: it answers the next paths it was given, and says what it was asked for.
    private sealed class ScriptedPicker : IFilePicker
    {
        internal Queue<string[]> Next { get; } = new();

        internal FilePickRequest? Last { get; private set; }

        internal int Asked { get; private set; }

        // When set, the picker fails as the system's can, with a COM error.
        internal bool Fails { get; set; }

        public Task<IReadOnlyList<string>> PickAsync(FilePickRequest request, Microsoft.UI.WindowId window)
        {
            Last = request;
            Asked++;
            return Fails
                ? Task.FromException<IReadOnlyList<string>>(new System.Runtime.InteropServices.COMException("the walk's picker fails as the system's can"))
                : Task.FromResult<IReadOnlyList<string>>(Next.Count > 0 ? Next.Dequeue() : []);
        }
    }

    // A small JPEG with a latitude in its EXIF block (and a scan of a few bytes), to see that the place is not in what is sent.
    private static byte[] PhotoWithPlace(int seed)
    {
        byte[] tiff =
        [
            .. "II"u8.ToArray(), 42, 0, 8, 0, 0, 0,
            1, 0, 0x25, 0x88, 4, 0, 1, 0, 0, 0, 26, 0, 0, 0, 0, 0, 0, 0,
            1, 0, 2, 0, 5, 0, 3, 0, 0, 0, 44, 0, 0, 0, 0, 0, 0, 0,
            30, 0, 0, 0, 1, 0, 0, 0, 54, 0, 0, 0, 1, 0, 0, 0, 7, 0, 0, 0, 1, 0, 0, 0,
        ];
        byte[] exif = [.. "Exif\0\0"u8.ToArray(), .. tiff];
        int length = exif.Length + 2;
        byte[] scan = new byte[2000];
        new Random(seed).NextBytes(scan);
        for (int at = 0; at < scan.Length; at++)
        {
            if (scan[at] == 0xFF)
            {
                scan[at] = 0xFE;
            }
        }

        return [0xFF, 0xD8, 0xFF, 0xE1, (byte)(length >> 8), (byte)length, .. exif, 0xFF, 0xDA, 0, 2, .. scan, 0xFF, 0xD9];
    }

    private static string Hash(byte[] bytes) => Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant();

    private IEnumerable<T> InTree<T>(DependencyObject? root = null) where T : DependencyObject => Below(root ?? window.Surface).OfType<T>();

    private Button? ButtonStarting(string start) =>
        InTree<Button>().FirstOrDefault(button => button.ActualWidth > 0 && Name(button).StartsWith(start, StringComparison.Ordinal));

    private bool TextIsOnScreen(string words) =>
        InTree<TextBlock>().Any(block => block.ActualWidth > 0 && block.Visibility == Visibility.Visible && block.Text.Contains(words, StringComparison.Ordinal));

    private FieldView? CoreField(string id) =>
        host.State.FrontScreen is { } front && front.Forms.TryGetValue("customer", out FormView? form) && form.Fields.TryGetValue(id, out FieldView? held) ? held : null;

    private int CoreCount() => CoreField("f-papers")?.Value is FilesValue files ? files.Files.Count : 0;

    private string CoreFiles() =>
        CoreField("f-papers")?.Value is FilesValue files ? string.Join(", ", files.Files.Select(file => file.Name + " " + file.Size + " " + file.MediaType)) : "nothing (the front screen is '" + host.State.FrontScreen?.Title + "', its forms: " + string.Join(", ", host.State.FrontScreen?.Forms.Select(form => form.Key + ":" + string.Join("/", form.Value.Fields.Select(field => field.Key + "=" + field.Value.Value.GetType().Name))) ?? []) + ")";

    private async Task InputsAsync()
    {
        Say("== the protocol's form example: a colour field and a file field");
        window.Size(1280, 900);
        host.Send(Events.Select(0));
        await SettleAsync();
        await Task.Delay(1500);
        Button? open = ButtonStarting("New customer");
        Check("the walk's own screen has the button that opens the form", open is not null, open is null ? "none" : "found");
        if (open is null)
        {
            return;
        }

        Foreground();
        _ = open.Focus(FocusState.Keyboard);
        await Task.Delay(300);
        await PressAsync(EnterKey);
        for (int waited = 0; waited < 10000 && host.State.FrontScreen?.Title != "New customer"; waited += 250)
        {
            await Task.Delay(250);
        }

        await SettleAsync();
        await Task.Delay(1500);
        bool native = host.State.FrontScreen is { Title: "New customer", Content: NativeContent };
        Check("the form with a colour field and a file field is drawn natively and not shown in the web view", native, "the front screen is '" + host.State.FrontScreen?.Title + "', " + host.State.FrontScreen?.Content?.GetType().Name + (host.State.FrontScreen?.Content is WebContent web ? ", why: " + string.Join(", ", web.Why) : string.Empty));
        if (!native)
        {
            return;
        }

        await ShotAsync("inputs-form");
        Say("buttons on the form: " + string.Join(" | ", InTree<Button>().Select(button => Name(button) + "/" + button.Content?.GetType().Name + "/" + button.ActualWidth)));
        string work = Path.Combine(Path.GetTempPath(), "navree-inputs-" + Environment.ProcessId);
        _ = Directory.CreateDirectory(work);
        try
        {
            await ColourAsync();
            await FilesAsync(work);
        }
        finally
        {
            try
            {
                Directory.Delete(work, true);
            }
            catch (IOException)
            {
                // A folder in the temporary place that cannot be deleted is left for the system to clear.
            }
        }
    }

    // ---- the colour field ---------------------------------------------------------------------------------------

    private async Task ColourAsync()
    {
        Say("== the colour field, by the keyboard");
        Button? button = ButtonStarting("Label colour");
        Check("the colour field is a button that says its label, the colour's name and its value", button is not null && Name(button) == "Label colour, green, hex 1 6 a 3 4 a", "the name is '" + (button is null ? "none" : Name(button)) + "'");
        if (button is null)
        {
            return;
        }

        button.StartBringIntoView();
        await Task.Delay(600);
        Check("the value is written on the field as well as shown as a swatch", Below(button).OfType<TextBlock>().Any(block => block.Text == "#16a34a"), "texts: " + string.Join(" | ", Below(button).OfType<TextBlock>().Select(block => block.Text)));
        FrameworkElement? parent = button.Parent as FrameworkElement;
        await ShowAsync(button);
        await ShotAsync("inputs-colour-closed");

        // Open with the keyboard: the focus is put on the button and Enter pressed.
        Foreground();
        _ = button.Focus(FocusState.Keyboard);
        await Task.Delay(400);
        await PressAsync(EnterKey);
        await Task.Delay(1200);
        Microsoft.UI.Xaml.Controls.ColorPicker? picker = Popups().SelectMany(popup => Below(popup)).OfType<Microsoft.UI.Xaml.Controls.ColorPicker>().FirstOrDefault();
        Check("Enter opens the system's colour picker in a flyout", picker is not null, picker is null ? "no colour picker is open" : "open, holding " + Hex(picker));
        if (picker is null)
        {
            return;
        }

        Check("the picker opens on the colour the field holds", Hex(picker) == "#16a34a", "it holds " + Hex(picker));
        Say("the focus is on " + FocusedWords());
        await ShotAsync("inputs-colour-open");

        // The hex box: select what is in it and type a new colour with the keys; then reach OK with Tab and press Enter.
        TextBox? hex = Below(picker).OfType<TextBox>().FirstOrDefault(box => box.ActualWidth > 0 && (Name(box).Contains("hex", StringComparison.OrdinalIgnoreCase) || box.Text.StartsWith('#')));
        Check("the picker has its hex box", hex is not null, hex is null ? "no hex box; text boxes: " + string.Join(" | ", Below(picker).OfType<TextBox>().Select(box => Name(box) + "=" + box.Text)) : "text '" + hex.Text + "'");
        if (hex is null)
        {
            return;
        }

        _ = hex.Focus(FocusState.Keyboard);
        await Task.Delay(300);
        keybd_event(ControlKey, 0, 0, UIntPtr.Zero);
        keybd_event(KeyA, 0, 0, UIntPtr.Zero);
        keybd_event(KeyA, 0, KeyUp, UIntPtr.Zero);
        keybd_event(ControlKey, 0, KeyUp, UIntPtr.Zero);
        await Task.Delay(200);
        foreach (char digit in "D97706")
        {
            await PressAsync((byte)digit);
        }

        await PressAsync(EnterKey);
        await Task.Delay(500);
        Check("the picker holds the colour that was typed", Hex(picker) == "#d97706", "it holds " + Hex(picker) + ", the hex box says '" + hex.Text + "'");
        Check("typing in the picker chooses nothing yet: the core still holds the first colour", CoreField("f-label")?.Value is TextValue { Text: "#16a34a" } or null, "the core holds " + (CoreField("f-label")?.Value as TextValue)?.Text);

        Button? ok = null;
        for (int presses = 0; presses < 25 && ok is null; presses++)
        {
            await PressAsync(TabKey);
            await Task.Delay(100);
            if (FocusManager.GetFocusedElement(window.Surface.XamlRoot) is Button { Content: "OK" } focused)
            {
                ok = focused;
            }
        }

        Check("Tab reaches the OK button", ok is not null, "after Tab the focus is on " + FocusedWords());
        if (ok is null)
        {
            return;
        }

        await PressAsync(EnterKey);
        await SettleAsync();
        await Task.Delay(800);
        Button? after = ButtonStarting("Label colour");
        string held = (CoreField("f-label")?.Value as TextValue)?.Text ?? "nothing";
        Check("OK sends the colour to the core as the protocol writes it", held == "#d97706", "the core holds '" + held + "'");
        Check("the field is the same object after the answer", ReferenceEquals(button, after) && ReferenceEquals(parent, after?.Parent), "same button: " + ReferenceEquals(button, after));
        Check("the field says the new colour in words, by name and by value", after is not null && Name(after) == "Label colour, orange, hex d 9 7 7 0 6" && Below(after).OfType<TextBlock>().Any(block => block.Text == "#d97706"), "the name is '" + (after is null ? "none" : Name(after)) + "'");
        Check("the focus is back on the field when the picker closes", ReferenceEquals(FocusManager.GetFocusedElement(window.Surface.XamlRoot), button), "focus is on " + FocusedWords());
        await ShowAsync(button);
        await ShotAsync("inputs-colour-chosen");

        // Escape closes the picker and chooses nothing.
        await PressAsync(EnterKey);
        await Task.Delay(900);
        Microsoft.UI.Xaml.Controls.ColorPicker? again = Popups().SelectMany(popup => Below(popup)).OfType<Microsoft.UI.Xaml.Controls.ColorPicker>().FirstOrDefault();
        Check("the picker opens again on the colour now held", again is not null && Hex(again) == "#d97706", again is null ? "no picker" : "it holds " + Hex(again));
        if (again is not null)
        {
            again.Color = Windows.UI.Color.FromArgb(255, 1, 2, 3);
            await PressAsync(EscapeKey);
            await Task.Delay(800);
            Check("Escape closes the picker and the core still holds the colour", (CoreField("f-label")?.Value as TextValue)?.Text == "#d97706" && !Popups().Any(popup => Below(popup).OfType<Microsoft.UI.Xaml.Controls.ColorPicker>().Any()), "the core holds '" + (CoreField("f-label")?.Value as TextValue)?.Text + "'");
        }
    }

    // Brings a part of the page up to near the top of the window so that a picture shows all of it.
    private async Task ShowAsync(FrameworkElement? element)
    {
        element?.StartBringIntoView(new BringIntoViewOptions { VerticalAlignmentRatio = 0.15, AnimationDesired = false });
        await Task.Delay(700);
    }

    private static string Hex(Microsoft.UI.Xaml.Controls.ColorPicker picker) => ColourRules.Hex(picker.Color.R, picker.Color.G, picker.Color.B);

    // ---- the file field -------------------------------------------------------------------------------------------

    private async Task FilesAsync(string work)
    {
        Say("== the file field, through a picker the walk supplies");
        ScriptedPicker picker = new();
        host.FilePicker = picker;
        byte[] pan = new byte[150_000];
        new Random(11).NextBytes(pan);
        byte[] lease = new byte[40_000];
        new Random(12).NextBytes(lease);
        byte[] receipt = new byte[9_000];
        new Random(13).NextBytes(receipt);
        byte[] photo = PhotoWithPlace(14);
        string panPath = Path.Combine(work, "Harpreet Kaur - PAN.pdf");
        string leasePath = Path.Combine(work, "Shop lease, Ludhiana.pdf");
        string receiptPath = Path.Combine(work, "receipt.pdf");
        string photoPath = Path.Combine(work, "Ludhiana-gate.jpg");
        string notesPath = Path.Combine(work, "notes.txt");
        string bigPath = Path.Combine(work, "full-register.pdf");
        File.WriteAllBytes(panPath, pan);
        File.WriteAllBytes(leasePath, lease);
        File.WriteAllBytes(receiptPath, receipt);
        File.WriteAllBytes(photoPath, photo);
        File.WriteAllText(notesPath, "Call back on Monday.", Encoding.UTF8);
        File.WriteAllBytes(bigPath, new byte[400_000]);

        Button? choose = ButtonStarting("Choose files for Papers");
        Check("the file field has its Choose button", choose is not null, choose is null ? "none" : "found, named '" + Name(choose) + "'");
        if (choose is null)
        {
            return;
        }

        FrameworkElement? box = InTree<FrameworkElement>().FirstOrDefault(element => Name(element).StartsWith("Papers, ", StringComparison.Ordinal) && element is Border);
        choose.StartBringIntoView();
        await Task.Delay(600);
        Check("the field says, to a screen reader, its label and that no file is chosen", box is not null && Name(box) == "Papers, no file chosen", "the name is '" + (box is null ? "none" : Name(box)) + "'");
        await ShowAsync(box);
        await ShotAsync("inputs-files-empty");

        // 1. A file chosen: its name and size show, and the core holds it under a token (never a path).
        picker.Next.Enqueue([panPath]);
        Foreground();
        _ = choose.Focus(FocusState.Keyboard);
        await Task.Delay(300);
        await PressAsync(SpaceKey);
        await SettleAsync();
        await Task.Delay(1200);
        Check("Space on Choose asked the picker for the field's kinds and for many files", picker.Asked == 1 && picker.Last is { Multiple: true } last && last.Types.Contains(".pdf") && last.Types.Contains(".jpg") && !last.Types.Contains("*"), "asked " + picker.Asked + " times, for " + (picker.Last is null ? "nothing" : string.Join(" ", picker.Last.Types) + " multiple " + picker.Last.Multiple));
        Check("the file's name and size are on the field", TextIsOnScreen("Harpreet Kaur - PAN.pdf") && TextIsOnScreen("146 KB"), "core holds: " + CoreFiles());
        Check("the core holds the file with its name, size and type", CoreFiles() == "Harpreet Kaur - PAN.pdf 150000 application/pdf", CoreFiles());
        Check("the field says, to a screen reader, how many files are chosen", box is not null && Name(box) == "Papers, 1 file chosen", "the name is '" + (box is null ? "none" : Name(box)) + "'");

        // 2. A wrong kind, and a file too large: refused with words at the field, and held nowhere.
        picker.Next.Enqueue([notesPath]);
        await PressAsync(SpaceKey);
        await SettleAsync();
        await Task.Delay(800);
        Check("a file of a wrong kind is refused with words", TextIsOnScreen("notes.txt is not a kind of file this field takes (it takes pictures and .pdf), so it was not added."), "core holds: " + CoreFiles());
        picker.Next.Enqueue([bigPath]);
        _ = choose.Focus(FocusState.Keyboard);
        await PressAsync(SpaceKey);
        await SettleAsync();
        await Task.Delay(800);
        string most = FileRules.Size(host.Options.MostFileBytes);
        Check("a file that is too large is refused with words that say the most", TextIsOnScreen("full-register.pdf is larger than " + most + " and was not added."), "the most is " + most);
        Check("neither was added", CoreFiles() == "Harpreet Kaur - PAN.pdf 150000 application/pdf", CoreFiles());
        picker.Next.Enqueue([leasePath, photoPath, receiptPath]);
        await PressAsync(SpaceKey);
        await SettleAsync();
        await Task.Delay(1200);
        Check("three files chosen at once are all added, and the words of the refusals are gone", CoreCount() == 4 && !TextIsOnScreen("is not a kind of file"), CoreFiles());
        string secondPath = Path.Combine(work, "second-pan.pdf");
        File.WriteAllBytes(secondPath, new byte[120_000]);
        picker.Next.Enqueue([secondPath]);
        _ = choose.Focus(FocusState.Keyboard);
        await PressAsync(SpaceKey);
        await SettleAsync();
        await Task.Delay(800);
        Check("a file that would take the files of the field together over the most is refused with words", TextIsOnScreen("Adding second-pan.pdf would take the files of this field over " + most + ", so it was not added.") && CoreCount() == 4, "the core holds " + CoreCount() + " files");
        await ShowAsync(box);
        await ShotAsync("inputs-files-chosen");

        // 3. A file taken out: the button for it, with the keyboard; the focus goes back to Choose.
        Button? remove = ButtonStarting("Remove receipt.pdf");
        Check("each file has a button to take it out, named for the file", remove is not null, remove is null ? "none" : "found");
        if (remove is not null)
        {
            _ = remove.Focus(FocusState.Keyboard);
            await Task.Delay(300);
            await PressAsync(EnterKey);
            await SettleAsync();
            await Task.Delay(800);
            Check("taking a file out clears it from the core and from the field", !CoreFiles().Contains("receipt.pdf", StringComparison.Ordinal) && !TextIsOnScreen("receipt.pdf") && CoreCount() == 3, CoreFiles());
            Check("the focus is on Choose after a file is taken out", ReferenceEquals(FocusManager.GetFocusedElement(window.Surface.XamlRoot), choose), "focus is on " + FocusedWords());
        }

        // 4. The lease is deleted before the form is sent; the form is sent while the server is slow, so what the field shows while it is on its way is read.
        File.Delete(leasePath);
        if (served is not null)
        {
            served.Received.Clear();
            served.Delay = TimeSpan.FromMilliseconds(2500);
        }

        Button? save = InTree<Button>().FirstOrDefault(button => button.ActualWidth > 0 && Name(button) == "Save");
        Check("the form has its Save button", save is not null, save is null ? "none" : "found");
        if (save is null)
        {
            return;
        }

        FrameworkElement? boxBefore = box;
        save.StartBringIntoView();
        await Task.Delay(800);
        await ShotAsync("inputs-before-save");
        Foreground();
        _ = save.Focus(FocusState.Keyboard);
        await Task.Delay(300);
        await PressAsync(EnterKey);
        await ShotAsync("inputs-after-save");
        bool busy = false;
        for (int waited = 0; waited < 2200; waited += 100)
        {
            await Task.Delay(100);
            busy |= host.State.FrontScreen?.Forms.TryGetValue("customer", out FormView? sending) == true && sending.Pending && ButtonStarting("Choose files for Papers") is { IsEnabled: false };
        }

        Check("while the form is on its way the field cannot be changed", busy, "pending seen with Choose disabled: " + busy);
        await SettleAsync();
        await Task.Delay(1500);
        if (served is not null)
        {
            served.Delay = TimeSpan.Zero;
        }

        IReadOnlyList<ReceivedPart> got = served?.Received.LastOrDefault() ?? [];
        ReceivedPart? gotPan = got.FirstOrDefault(part => part.FileName == "Harpreet Kaur - PAN.pdf");
        ReceivedPart? gotPhoto = got.FirstOrDefault(part => part.FileName == "Ludhiana-gate.jpg");
        Say("requests the server saw: " + string.Join(" ; ", served?.Requests.TakeLast(6) ?? []));
        Say("the server received " + got.Count + " parts: " + string.Join("; ", got.Select(part => part.Name + (part.FileName is null ? string.Empty : " file " + part.FileName + " " + part.MediaType + " " + part.Length + " bytes"))));
        Check("the server received the PAN file with exactly its bytes", gotPan is not null && gotPan.Sha256 == Hash(pan) && gotPan.Length == pan.Length && gotPan.MediaType == "application/pdf", gotPan is null ? "not received" : "hash " + gotPan.Sha256[..12] + " of " + gotPan.Length + " bytes, expected " + Hash(pan)[..12]);
        Check("the server received the photo without the place it was taken", gotPhoto is not null && gotPhoto.Sha256 != Hash(photo) && gotPhoto.Length == photo.Length, gotPhoto is null ? "not received" : "hash " + gotPhoto.Sha256[..12] + ", the original's " + Hash(photo)[..12]);
        Check("the photo that was sent holds no latitude and still holds its scan", gotPhoto is not null && gotPhoto.Content.AsSpan().IndexOf(new byte[] { 30, 0, 0, 0, 1, 0, 0, 0, 54 }) < 0 && gotPhoto.Content.AsSpan().IndexOf(photo.AsSpan(photo.Length - 100, 90)) >= 0, "the original holds the latitude: " + (photo.AsSpan().IndexOf(new byte[] { 30, 0, 0, 0, 1, 0, 0, 0, 54 }) >= 0));
        Check("the colour was sent in the protocol's form", got.FirstOrDefault(part => part.Name == "label")?.Text == "#d97706", "the form's label part is '" + got.FirstOrDefault(part => part.Name == "label")?.Text + "'");
        Check("the deleted file was left out, and no file came empty", got.All(part => part.FileName != "Shop lease, Ludhiana.pdf") && got.Where(part => part.FileName is not null).All(part => part.Length > 0), "files: " + string.Join(", ", got.Where(part => part.FileName is not null).Select(part => part.FileName + " " + part.Length)));
        Check("the server's words are shown at the file field, as on any field", TextIsOnScreen("2 papers came with it"), "the field's errors: " + string.Join(" | ", InTree<TextBlock>().Where(block => block.Visibility == Visibility.Visible && block.Foreground is not null && block.Text.Contains("paper", StringComparison.Ordinal)).Select(block => block.Text)));
        Check("the deleted file is shown as gone, with words", TextIsOnScreen(FileRules.GoneWords) && TextIsOnScreen("Shop lease, Ludhiana.pdf"), "texts: " + string.Join(" | ", InTree<TextBlock>().Where(block => block.ActualWidth > 0).Select(block => block.Text).Where(text => text.Contains("lease", StringComparison.OrdinalIgnoreCase) || text.Contains("available", StringComparison.OrdinalIgnoreCase))));
        FrameworkElement? boxAfter = InTree<FrameworkElement>().FirstOrDefault(element => Name(element).StartsWith("Papers, ", StringComparison.Ordinal) && element is Border);
        Check("the file field is the same object after the answer, with its files still in it", ReferenceEquals(boxBefore, boxAfter) && TextIsOnScreen("Harpreet Kaur - PAN.pdf") && ReferenceEquals(choose, ButtonStarting("Choose files for Papers")), "same: " + ReferenceEquals(boxBefore, boxAfter));
        await ShowAsync(boxAfter);
        await ShotAsync("inputs-files-sent");

        // 5. The gone file is taken out with the keyboard, which is the way to go on.
        Button? removeGone = ButtonStarting("Remove Shop lease, Ludhiana.pdf");
        if (removeGone is not null)
        {
            _ = removeGone.Focus(FocusState.Keyboard);
            await PressAsync(SpaceKey);
            await SettleAsync();
            await Task.Delay(600);
            Check("a file shown as gone can be taken out", !TextIsOnScreen("Shop lease, Ludhiana.pdf") && !CoreFiles().Contains("Shop lease", StringComparison.Ordinal), CoreFiles());
        }

        // 6. An item with no path on this computer, as a drop from a phone seen through Explorer or a link from a browser hands over, and a picker
        // that fails: each is said in words at the field and the app goes on. The walk cannot make a real drop; a drop offers what it holds to the
        // field the same way the picker does (FileFieldView.OfferAsync), after FileRules.Sort, whose decision the unit tests hold.
        Button? again = ButtonStarting("Choose files for Papers");
        if (again is null)
        {
            Check("the Choose button is still there for the last two cases", false, "none");
            return;
        }

        int before = CoreCount();
        picker.Next.Enqueue([string.Empty]);
        Foreground();
        _ = again.Focus(FocusState.Keyboard);
        await Task.Delay(300);
        await PressAsync(SpaceKey);
        await SettleAsync();
        await Task.Delay(800);
        Check("an item with no path on this computer is refused with words and the app goes on", TextIsOnScreen("is not a file on this computer") && CoreCount() == before, "core holds " + CoreCount() + " files, before " + before);
        picker.Fails = true;
        _ = again.Focus(FocusState.Keyboard);
        await PressAsync(SpaceKey);
        await SettleAsync();
        await Task.Delay(800);
        picker.Fails = false;
        Check("a file picker that fails is said in words and the app goes on", TextIsOnScreen(FileRules.PickerFailedWords) && CoreCount() == before, "core holds " + CoreCount() + " files");
        await ShowAsync(box);
        await ShotAsync("inputs-files-not-here");
    }
}
