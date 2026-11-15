using System.Globalization;
using Microsoft.UI.Xaml.Controls;
using Navree.Runtime;

namespace Navree.Demo;

// What the core holds for a field after what a person did to its box, read from the core and not from the box.
internal sealed partial class Pressing
{
    // A letter is typed after the text of a field and taken away again, and then the whole text is selected and deleted: after each
    // the core holds what the box holds. The client used to compare the box with the page as it was last drawn, so a text that came
    // back to what the page was drawn with was never sent, and clearing a field was lost.
    private async Task TypedBackAsync()
    {
        await ShowAsync(2);
        TextBox? box = Everything().OfType<TextBox>().FirstOrDefault(candidate => NameOf(candidate).StartsWith("Units of Chakla belan", StringComparison.Ordinal));
        if (box is null)
        {
            Check("the quantity of a row of Inventory is on the screen", false, "no text box");
            return;
        }

        string drawn = box.Text;
        Say("== typed back; the box was drawn holding '" + drawn + "' and the core holds '" + CoreQuantity() + "'");
        await ClickAsync(box);
        Foreground();
        await KeyAsync(0, EndKey);
        await KeyAsync(0, (byte)(Digit0 + 7));
        await Task.Delay(1500);
        TextBox? typed = QuantityBox();
        Check("a letter typed after the text is held by the core", typed?.Text == drawn + "7" && CoreQuantity() == drawn + "7", "box '" + typed?.Text + "', core '" + CoreQuantity() + "'");

        Foreground();
        await KeyAsync(0, BackSpace);
        await Task.Delay(1500);
        TextBox? back = QuantityBox();
        Check("the text back to what it was drawn with is held by the core", back?.Text == drawn && CoreQuantity() == drawn, "box '" + back?.Text + "', core '" + CoreQuantity() + "'");

        Foreground();
        await KeyAsync(CtrlKey, LetterA);
        await KeyAsync(0, Delete);
        await Task.Delay(1500);
        TextBox? cleared = QuantityBox();
        Check("a field cleared is held empty by the core", cleared?.Text == string.Empty && CoreQuantity() == string.Empty, "box '" + cleared?.Text + "', core '" + CoreQuantity() + "'");
    }

    private TextBox? QuantityBox() =>
        Everything().OfType<TextBox>().FirstOrDefault(candidate => NameOf(candidate).StartsWith("Units of Chakla belan", StringComparison.Ordinal));

    // What the core holds for the quantity of the row of the product the first box belongs to (product 18): the text, or null for none.
    private string? CoreQuantity()
    {
        ScreenView? front = host.State.FrontScreen;
        FormView? form = front?.Forms.FirstOrDefault(entry => entry.Key.Contains("/inventory/18/", StringComparison.Ordinal)).Value;
        FieldView? field = form?.Fields.Values.FirstOrDefault(candidate => candidate.Name == "quantity");
        return field?.Value is TextValue text ? text.Text : null;
    }
}
