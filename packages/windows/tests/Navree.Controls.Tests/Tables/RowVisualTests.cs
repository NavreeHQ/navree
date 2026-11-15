using Navree.Design.Tables;

namespace Navree.Controls.Tests.Tables;

[TestClass]
public sealed class RowVisualTests
{
    [TestMethod]
    [DataRow(false, false, false, "Normal")]
    [DataRow(false, false, true, "PointerOver")]
    [DataRow(false, true, true, "Pressed")]
    [DataRow(true, false, false, "Chosen")]
    [DataRow(true, false, true, "ChosenPointerOver")]
    [DataRow(true, true, true, "ChosenPressed")]
    public void A_row_is_drawn_in_the_state_of_the_light_and_dark_themes_when_the_contrast_colours_are_off(bool chosen, bool pressed, bool over, string expected) =>
        Assert.AreEqual(expected, RowVisual.State(chosen, pressed, over, highContrast: false));

    [TestMethod]
    [DataRow(false, false, false, "Normal")]
    [DataRow(false, false, true, "Highlighted")]
    [DataRow(false, true, true, "Highlighted")]
    [DataRow(true, false, false, "HighlightedChosen")]
    [DataRow(true, false, true, "HighlightedChosen")]
    [DataRow(true, true, true, "HighlightedChosen")]
    public void A_row_that_is_chosen_pressed_or_pointed_at_is_the_highlight_in_the_contrast_colours(bool chosen, bool pressed, bool over, string expected) =>
        Assert.AreEqual(expected, RowVisual.State(chosen, pressed, over, highContrast: true));

    [TestMethod]
    public void The_words_of_a_highlighted_row_take_the_highlights_text_and_no_other_row_does()
    {
        Assert.IsTrue(RowVisual.TextOnHighlight("Highlighted"));
        Assert.IsTrue(RowVisual.TextOnHighlight("HighlightedChosen"));
        foreach (string state in new[] { "Normal", "PointerOver", "Pressed", "Chosen", "ChosenPointerOver", "ChosenPressed" })
        {
            Assert.IsFalse(RowVisual.TextOnHighlight(state));
        }
    }
}
