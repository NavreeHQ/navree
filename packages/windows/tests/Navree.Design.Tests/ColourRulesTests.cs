using Navree.Design.Inputs;

namespace Navree.Design.Tests;

/// <summary>A colour field's value as the protocol writes it, and the words said about a colour.</summary>
[TestClass]
public sealed class ColourRulesTests
{
    [TestMethod]
    [DataRow("#16a34a", "#16a34a")]
    [DataRow("16A34A", "#16a34a")]
    [DataRow("  #16A34a ", "#16a34a")]
    [DataRow("#fa0", "#ffaa00")]
    [DataRow("FFF", "#ffffff")]
    public void A_colour_is_read_with_or_without_the_hash_in_either_case_and_written_as_seven_characters_in_lower_case(string written, string value)
    {
        Assert.AreEqual(value, ColourRules.Normalise(written));
    }

    [TestMethod]
    [DataRow("")]
    [DataRow("#")]
    [DataRow("#16a34")]
    [DataRow("#16a34a0")]
    [DataRow("#gg0000")]
    [DataRow("green")]
    [DataRow(null)]
    public void Text_that_is_not_a_colour_is_not_read(string? written)
    {
        Assert.IsNull(ColourRules.Normalise(written));
    }

    [TestMethod]
    public void Every_colour_goes_round_through_its_value()
    {
        for (int red = 0; red < 256; red += 17)
        {
            for (int green = 0; green < 256; green += 17)
            {
                for (int blue = 0; blue < 256; blue += 17)
                {
                    string value = ColourRules.Hex((byte)red, (byte)green, (byte)blue);
                    Assert.AreEqual(7, value.Length);
                    Assert.IsTrue(ColourRules.TryRead(value, out byte r, out byte g, out byte b));
                    Assert.AreEqual((red, green, blue), ((int)r, (int)g, (int)b));
                }
            }
        }
    }

    [TestMethod]
    [DataRow("#000000", "black")]
    [DataRow("#ffffff", "white")]
    [DataRow("#808080", "grey")]
    [DataRow("#333333", "dark grey")]
    [DataRow("#16a34a", "green")]
    [DataRow("#ef4444", "red")]
    [DataRow("#2563eb", "blue")]
    [DataRow("#7a1212", "dark red")]
    [DataRow("#fca5a5", "light red")]
    [DataRow("#f59e0b", "orange")]
    [DataRow("#78350f", "brown")]
    [DataRow("#eab308", "yellow")]
    [DataRow("#7c3aed", "purple")]
    [DataRow("#db2777", "pink")]
    [DataRow("#0d9488", "teal")]
    public void A_colour_is_called_by_a_word_or_two(string value, string name)
    {
        Assert.IsTrue(ColourRules.TryRead(value, out byte r, out byte g, out byte b));
        Assert.AreEqual(name, ColourRules.Family(r, g, b), value);
    }

    [TestMethod]
    public void The_field_says_its_value_in_words_and_is_never_told_by_colour_alone()
    {
        Assert.AreEqual("#16a34a", ColourRules.Shown("#16A34A"));
        Assert.AreEqual("Not chosen", ColourRules.Shown(""));
        Assert.AreEqual("Not chosen", ColourRules.Shown("not a colour"));
        Assert.AreEqual("green, #16a34a", ColourRules.Words("#16a34a"));
        Assert.AreEqual("Not chosen", ColourRules.Words(null));
    }

    [TestMethod]
    public void A_screen_reader_hears_the_label_the_colours_name_and_the_value_one_character_at_a_time()
    {
        Assert.AreEqual("Label colour, green, hex 1 6 a 3 4 a", ColourRules.Spoken("Label colour", "#16a34a"));
        Assert.AreEqual("Label colour, not chosen", ColourRules.Spoken("Label colour", ""));
    }
}
