using System.Globalization;

namespace Navree.Design.Tests;

/// <summary>One rule writes a number in words: a value that rounds up to the next whole is that whole, with no fraction.</summary>
[TestClass]
public sealed class KindsWordsTests
{
    private static readonly ValueStyle Plain = new();

    [TestMethod]
    public void A_value_that_rounds_up_to_the_next_whole_is_written_as_that_whole()
    {
        Assert.AreEqual("5", ValueWords.Text(4.96, Plain));
        Assert.AreEqual("100", ValueWords.Text(99.97, Plain));
        Assert.AreEqual("1,000", ValueWords.Text(999.95, Plain));
        Assert.AreEqual("₹1,00,000", ValueWords.Text(99999.96, new ValueStyle("₹", string.Empty, true)));
    }

    [TestMethod]
    public void A_value_that_rounds_to_nothing_has_no_sign_and_no_fraction()
    {
        Assert.AreEqual("0", ValueWords.Text(-0.01, Plain));
        Assert.AreEqual("0", ValueWords.Text(0.04, Plain));
        Assert.AreEqual("0", ValueWords.Text(0, Plain));
    }

    [TestMethod]
    public void The_rounding_edges_are_the_amounts_edges()
    {
        Assert.AreEqual("0.1", ValueWords.Text(0.05, Plain));
        Assert.AreEqual("4.9", ValueWords.Text(4.94, Plain));
        Assert.AreEqual("4.5", ValueWords.Text(4.5, Plain));
        Assert.AreEqual("−4.5", ValueWords.Text(-4.5, Plain));
        Assert.AreEqual("−5", ValueWords.Text(-4.96, Plain));
        Assert.AreEqual("12.3", ValueWords.Text(12.34, Plain));
    }

    [TestMethod]
    public void The_axis_and_the_amount_write_every_value_alike()
    {
        CultureInfo invariant = CultureInfo.InvariantCulture;
        for (int hundredths = -20000; hundredths <= 20000; hundredths++)
        {
            double value = hundredths / 100.0;
            string amount = NumberWords.Amount(value, invariant).Replace('-', '−');
            if (amount is "−0")
            {
                amount = "0";
            }

            Assert.AreEqual(amount, ValueWords.Text(value, Plain), "the value " + value.ToString(invariant));
        }
    }
}
