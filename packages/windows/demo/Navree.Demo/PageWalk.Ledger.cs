using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Controls;
using Navree.Runtime;

namespace Navree.Demo;

// A table's search bar after its query form moved: the walk's own ledger has a query form with no id, known by its address, and the answer that
// follows the first puts it at another address (a filter carried in the address) with a search the server says it holds. The bar must then be the
// bar of the form the table has now: it shows what the core holds for that form's search, and what is typed next is held under that form.
internal sealed partial class PageWalk
{
    private const string LedgerForm = "@/ledger/paid";

    private string LedgerHolds() =>
        host.State.FrontScreen is { } front && front.Forms.TryGetValue(LedgerForm, out FormView? form) && form.Fields.TryGetValue("ledger-search", out FieldView? field) && field.Value is TextValue text
            ? text.Text
            : "nothing (the screen's forms: " + string.Join(", ", host.State.FrontScreen?.Forms.Keys ?? []) + ")";

    private async Task LedgerAsync()
    {
        Say("== a table's search after its query form moved to another address");
        window.Size(1280, 900);
        host.Send(Events.Select(0));
        await SettleAsync();
        await Task.Delay(1500);
        Button? open = ButtonStarting("A ledger");
        Check("the walk's own screen has the button that opens the ledger", open is not null, open is null ? "none" : "found");
        if (open is null || served is null)
        {
            return;
        }

        Foreground();
        _ = open.Focus(FocusState.Keyboard);
        await Task.Delay(300);
        await PressAsync(EnterKey);
        for (int waited = 0; waited < 10000 && host.State.FrontScreen?.Title != "Ledger"; waited += 250)
        {
            await Task.Delay(250);
        }

        await SettleAsync();
        await Task.Delay(1000);
        TextBox? box = SearchBox();
        DataTable? table = TableNow();
        Check("the ledger's table and its search box are on the screen", box is not null && table is not null, "box " + (box is not null) + ", table " + (table is not null));
        if (box is null)
        {
            return;
        }

        // The answer that follows brings the same table, its query form now at /ledger/paid, holding a search.
        host.Send(Events.Refresh(host.State.Nav.Front ?? 0));
        await SettleAsync();
        await Task.Delay(1000);
        Check("the answer brought the table's query form at its new address, holding the server's search", LedgerHolds() == PageFetcher.LedgerSearch, "the core holds '" + LedgerHolds() + "'");
        TextBox? after = SearchBox();
        Check("the search box shows what the core holds for the table's form now", after?.Text == PageFetcher.LedgerSearch, "the box shows '" + after?.Text + "', the core holds '" + LedgerHolds() + "', same box as before: " + ReferenceEquals(box, after));
        if (after is null)
        {
            return;
        }

        await ClickAsync(after);
        after.SelectAll();
        foreach (char letter in "kaur")
        {
            Foreground();
            Press((byte)char.ToUpperInvariant(letter));
            await Task.Delay(600);
        }

        await SettleAsync();
        await Task.Delay(2000);
        TextBox? typed = SearchBox();
        Check("what is typed next is held under the table's form now", typed is not null && typed.Text == "kaur" && LedgerHolds() == "kaur", "the box shows '" + typed?.Text + "', the core holds '" + LedgerHolds() + "'");
        await ShotAsync("ledger-search");
        host.Send(Events.Back);
        await SettleAsync();
        await Task.Delay(800);
    }
}
