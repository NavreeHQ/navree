using Navree.Design.Inputs;
using Navree.Protocol;

namespace Navree.Design.Tests;

/// <summary>A choice is found by what it is, never by what it says.</summary>
[TestClass]
public sealed class ChoiceRulesTests
{
    private static Models.ChoiceOption Option(string label, string value, bool disabled = false) => new() { Label = label, Value = value, Disabled = disabled };

    [TestMethod]
    public void Of_two_people_with_one_name_the_one_chosen_is_the_one_whose_value_is_sent()
    {
        // A search for a customer finds two people called Amit Kumar, in Ludhiana and in Jalandhar.
        IReadOnlyList<Suggestion> offered = ChoiceRules.Offer([Option("Amit Kumar", "customer-17"), Option("Amit Kumar", "customer-42")], more: false, createWords: null, typed: "amit");
        Assert.AreEqual(2, offered.Count);

        (ChoiceAct second, Suggestion? secondLine) = ChoiceRules.Chosen(offered, offered[1]);
        Assert.AreEqual(ChoiceAct.Send, second);
        Assert.AreEqual("customer-42", secondLine?.Value, "the second Amit Kumar was chosen and another's value is sent");

        (ChoiceAct first, Suggestion? firstLine) = ChoiceRules.Chosen(offered, offered[0]);
        Assert.AreEqual(ChoiceAct.Send, first);
        Assert.AreEqual("customer-17", firstLine?.Value);

        // The list shows each line by its words.
        Assert.AreEqual("Amit Kumar", offered[1].ToString());
    }

    [TestMethod]
    public void Two_options_of_one_name_read_apart_by_the_few_words_the_page_gives_beside_the_label()
    {
        Models.ChoiceOption ludhiana = new() { Label = "Amit Kumar", Value = "customer-17", Detail = "Ludhiana" };
        Models.ChoiceOption jalandhar = new() { Label = "Amit Kumar", Value = "customer-42", Detail = "Jalandhar" };
        Assert.AreEqual("Amit Kumar (Ludhiana)", ChoiceRules.Words(ludhiana));
        Assert.AreEqual("Amit Kumar (Jalandhar)", ChoiceRules.Words(jalandhar));

        // In the searched list the two lines read apart, and the one chosen is named in full where it is shown afterwards.
        IReadOnlyList<Suggestion> offered = ChoiceRules.Offer([ludhiana, jalandhar], more: false, createWords: null, typed: "amit");
        Assert.AreNotEqual(offered[0].ToString(), offered[1].ToString(), "two people of one name read the same in the list");
        Assert.AreEqual("Amit Kumar (Jalandhar)", offered[1].Words);
        Assert.AreEqual("Amit Kumar", offered[1].Label);
        Assert.AreEqual("customer-42", ChoiceRules.Chosen(offered, offered[1]).Line?.Value);

        // An option with no such words, or with blank ones, is its label alone.
        Assert.AreEqual("Clothing", ChoiceRules.Words(Option("Clothing", "clothing")));
        Assert.AreEqual("Clothing", ChoiceRules.Words("Clothing", "  "));
        Assert.AreEqual("Clothing (12)", ChoiceRules.Words("Clothing", " 12 "));

        // A list whose words beside the label changed is another list: a control that shows the first is filled again.
        Assert.IsFalse(ChoiceRules.SameOptions([ludhiana], [ludhiana with { Detail = "Moga" }]), "a control would go on showing the old words beside the label");
        Assert.IsTrue(ChoiceRules.SameOptions([ludhiana, jalandhar], [ludhiana with { }, jalandhar with { }]));
    }

    [TestMethod]
    public void A_thing_that_only_reads_like_a_line_of_the_list_is_not_chosen()
    {
        IReadOnlyList<Suggestion> offered = ChoiceRules.Offer([Option("Amit Kumar", "customer-17"), Option("Amit Kumar", "customer-42")], more: true, createWords: "Add a customer", typed: "amit");
        IReadOnlyList<Suggestion> earlier = ChoiceRules.Offer([Option("Amit Kumar", "customer-99")], more: false, createWords: null, typed: "ami");

        Assert.AreEqual(ChoiceAct.Nothing, ChoiceRules.Chosen(offered, "Amit Kumar").Act, "the words alone chose an option");
        Assert.AreEqual(ChoiceAct.Nothing, ChoiceRules.Chosen(offered, earlier[0]).Act, "a line of a list that is no longer offered chose an option");
        Assert.AreEqual(ChoiceAct.Nothing, ChoiceRules.Chosen(offered, null).Act);
        Assert.AreEqual(ChoiceAct.Nothing, ChoiceRules.Chosen(offered, new Suggestion(SuggestionKind.Option, "Amit Kumar", "customer-42")).Act);
    }

    [TestMethod]
    public void An_option_whose_words_are_those_of_the_line_for_more_is_an_option_and_the_line_for_more_asks_for_more()
    {
        IReadOnlyList<Suggestion> offered = ChoiceRules.Offer([Option(ChoiceRules.MoreWords, "plan-show-more"), Option("Basic", "plan-basic")], more: true, createWords: "Add a plan", typed: "sh");

        // The options in their order, then the line for more, then the line that makes a new one.
        CollectionAssert.AreEqual(
            new[] { SuggestionKind.Option, SuggestionKind.Option, SuggestionKind.More, SuggestionKind.Create },
            offered.Select(line => line.Kind).ToArray());
        Assert.AreEqual("Add a plan: sh", offered[3].Label);

        (ChoiceAct act, Suggestion? line) = ChoiceRules.Chosen(offered, offered[0]);
        Assert.AreEqual(ChoiceAct.Send, act, "an option called Show more was taken for the request for more");
        Assert.AreEqual("plan-show-more", line?.Value);
        Assert.AreEqual(ChoiceAct.More, ChoiceRules.Chosen(offered, offered[2]).Act);
        Assert.AreEqual(ChoiceAct.Create, ChoiceRules.Chosen(offered, offered[3]).Act);
    }

    [TestMethod]
    public void An_option_that_cannot_be_chosen_sends_nothing_and_nothing_is_made_from_no_words()
    {
        IReadOnlyList<Suggestion> offered = ChoiceRules.Offer([Option("Gurpreet Singh (left)", "staff-3", disabled: true)], more: false, createWords: "Add a person", typed: string.Empty);
        Assert.AreEqual(1, offered.Count, "a way to make a new one was offered for no words");
        Assert.AreEqual(ChoiceAct.Nothing, ChoiceRules.Chosen(offered, offered[0]).Act);
    }

    [TestMethod]
    public void Two_lists_that_read_the_same_and_send_other_values_are_two_lists()
    {
        Models.ChoiceOption[] shown = [Option("Amit Kumar", "customer-17"), Option("Simran Kaur", "customer-20")];
        Assert.IsTrue(ChoiceRules.SameOptions(shown, [Option("Amit Kumar", "customer-17"), Option("Simran Kaur", "customer-20")]));
        Assert.IsFalse(ChoiceRules.SameOptions(shown, [Option("Amit Kumar", "customer-42"), Option("Simran Kaur", "customer-20")]), "the same words with another value were taken for the same list");
        Assert.IsFalse(ChoiceRules.SameOptions(shown, [Option("Simran Kaur", "customer-20"), Option("Amit Kumar", "customer-17")]), "another order");
        Assert.IsFalse(ChoiceRules.SameOptions(shown, [Option("Amit Kumar", "customer-17")]));
        Assert.IsFalse(ChoiceRules.SameOptions(shown, [Option("Amit Kumar", "customer-17"), Option("Simran Kaur", "customer-20", disabled: true)]));
    }

    [TestMethod]
    public void A_value_is_found_by_itself_among_options_of_one_name()
    {
        Models.ChoiceOption[] options = [Option("Amit Kumar", "customer-17"), Option("Amit Kumar", "customer-42")];
        Assert.AreEqual(1, ChoiceRules.PlaceOf(options, "customer-42"));
        Assert.AreEqual(0, ChoiceRules.PlaceOf(options, "customer-17"));
        Assert.AreEqual(-1, ChoiceRules.PlaceOf(options, "Amit Kumar"));
        Assert.AreEqual(-1, ChoiceRules.PlaceOf(options, null));
    }
}
