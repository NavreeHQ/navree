using Navree.Design;

namespace Navree.Design.Tests;

[TestClass]
public sealed class TextRulesTests
{
    [TestMethod]
    public void A_short_last_word_is_kept_with_the_one_before_it()
    {
        Assert.AreEqual("Stock against the point it reorders\u00A0at", TextRules.KeepEnd("Stock against the point it reorders at"));
        Assert.AreEqual("Ship to\u00A0Goa", TextRules.KeepEnd("Ship to Goa"));
        Assert.AreEqual("Pay on\u00A0UPI", TextRules.KeepEnd("Pay on UPI"));
    }

    [TestMethod]
    public void Words_that_do_not_need_it_are_left_as_they_are()
    {
        Assert.AreEqual("Revenue today", TextRules.KeepEnd("Revenue today"));
        Assert.AreEqual("Up 12% on yesterday", TextRules.KeepEnd("Up 12% on yesterday"));
        Assert.AreEqual("", TextRules.KeepEnd(""));
        Assert.AreEqual("one", TextRules.KeepEnd("one"));
        Assert.AreEqual("two words", TextRules.KeepEnd("two words"));
        Assert.AreEqual("end with a space ", TextRules.KeepEnd("end with a space "));
        Assert.AreEqual("two lines\nof it", TextRules.KeepEnd("two lines\nof it"));
        Assert.AreEqual("a  b", TextRules.KeepEnd("a  b"));
    }

    [TestMethod]
    public void What_is_said_and_copied_is_the_words_as_they_were_sent()
    {
        string sent = "Stock against the point it reorders at";
        string drawn = TextRules.KeepEnd(sent);
        Assert.AreNotEqual(sent, drawn);
        Assert.AreEqual(sent, TextRules.Said(drawn));

        // The mark that fixes the order of an amount in a right-to-left page is drawn and not said.
        Assert.AreEqual("\u20B91,24,050", TextRules.Said("\u200E\u20B91,24,050"));
        Assert.AreEqual("Ship to Goa", TextRules.Said("\u200EShip to\u00A0Goa"));
        Assert.AreEqual("plain words", TextRules.Said("plain words"));
    }

    [TestMethod]
    public void Only_the_last_space_changes_and_nothing_else_is_lost()
    {
        string once = TextRules.KeepEnd("Stock against the point it reorders at");
        Assert.AreEqual("Stock against the point it reorders at", once.Replace('\u00A0', ' '));
        Assert.AreEqual(once, TextRules.KeepEnd(once));
    }
}
