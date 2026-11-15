using Navree.Design.Pages;

namespace Navree.Design.Tests;

/// <summary>A box holds the person's words; the core's value reaches it only when the core set it.</summary>
[TestClass]
public sealed class TextEchoTests
{
    private static readonly DateTime Start = new(2026, 11, 16, 9, 0, 0, DateTimeKind.Utc);

    [TestMethod]
    public void The_answers_to_letters_typed_one_after_the_other_never_write_into_the_box()
    {
        TextEcho echo = new();
        echo.Told("k", Start);
        echo.Told("ka", Start.AddMilliseconds(150));

        // The core answers the first letter after the second was typed: it holds "k", the box holds "ka".
        Assert.IsFalse(echo.ShouldWrite("k", "ka", Start.AddMilliseconds(200)));
        Assert.IsFalse(echo.ShouldWrite("ka", "kau", Start.AddMilliseconds(300)), "the second answer came after a third letter");
    }

    [TestMethod]
    public void A_value_the_core_sets_itself_is_written_into_the_box()
    {
        TextEcho echo = new();
        Assert.IsTrue(echo.ShouldWrite("Harpreet", string.Empty, Start), "a value on a field the person never typed in");
        Assert.IsFalse(echo.ShouldWrite("Harpreet", "Harpreet", Start), "the box has it already");
        echo.Told("Harpreetx", Start.AddSeconds(1));
        Assert.IsFalse(echo.ShouldWrite("Harpreetx", "Harpreetx", Start.AddSeconds(1)));
        Assert.IsTrue(echo.ShouldWrite(string.Empty, "Harpreetx", Start.AddSeconds(2)), "the core cleared the field (a form sent): the box is cleared");
    }

    [TestMethod]
    public void A_letter_typed_and_deleted_is_not_undone_by_the_answer_to_the_first_of_them()
    {
        TextEcho echo = new();
        echo.Told("a", Start);
        echo.Told(string.Empty, Start.AddMilliseconds(100));
        Assert.IsFalse(echo.ShouldWrite("a", string.Empty, Start.AddMilliseconds(150)));
        Assert.IsFalse(echo.ShouldWrite(string.Empty, string.Empty, Start.AddMilliseconds(250)));
    }

    [TestMethod]
    public void A_text_that_was_typed_twice_is_waited_for_in_the_order_it_was_typed()
    {
        TextEcho echo = new();
        echo.Told("a", Start);
        echo.Told(string.Empty, Start.AddMilliseconds(50));
        echo.Told("a", Start.AddMilliseconds(100));
        Assert.IsFalse(echo.ShouldWrite("a", "a", Start.AddMilliseconds(120)));
        Assert.IsFalse(echo.ShouldWrite(string.Empty, "a", Start.AddMilliseconds(140)), "the delete is still on its way and is not the core's own doing");
        Assert.IsFalse(echo.ShouldWrite("a", "a", Start.AddMilliseconds(160)));
    }

    [TestMethod]
    public void An_answer_that_never_comes_is_given_up_so_a_later_change_of_the_core_still_shows()
    {
        TextEcho echo = new(TimeSpan.FromSeconds(5));
        echo.Told("k", Start);
        Assert.IsTrue(echo.ShouldWrite("k", "ka", Start.AddSeconds(6)), "the box said k six seconds ago and nothing answered: k is now the core's own");
    }

    [TestMethod]
    public void The_core_saying_again_what_it_said_is_never_taken_for_a_change_of_its_own()
    {
        TextEcho echo = new(initial: "k");
        echo.Told("ka", Start);
        echo.Told("kau", Start.AddMilliseconds(100));
        Assert.IsFalse(echo.ShouldWrite("k", "kau", Start.AddMilliseconds(120)), "the box was given k when it was drawn: the core still says k");
        Assert.IsFalse(echo.ShouldWrite("ka", "kau", Start.AddMilliseconds(140)));
        Assert.IsFalse(echo.ShouldWrite("ka", "kau", Start.AddMilliseconds(160)), "an answer to something else repeats ka: the box has moved on");
        Assert.IsTrue(echo.ShouldWrite("x", "kau", Start.AddMilliseconds(180)), "a value nobody typed is the core's own");
    }
}
