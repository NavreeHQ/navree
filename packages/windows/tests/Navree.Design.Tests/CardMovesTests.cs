namespace Navree.Design.Tests;

/// <summary>The steps a card is moved by, and where a dropped card stands.</summary>
[TestClass]
public sealed class CardMovesTests
{
    private static readonly string[] Lane = ["a", "b", "c", "d"];

    [TestMethod]
    public void A_card_goes_up_before_the_card_above_and_nowhere_from_the_top()
    {
        Assert.AreEqual(new CardStep(true, "a"), CardMoves.Up(Lane, 1));
        Assert.AreEqual(new CardStep(true, "c"), CardMoves.Up(Lane, 3));
        Assert.IsFalse(CardMoves.Up(Lane, 0).Possible);
        Assert.IsFalse(CardMoves.Up(Lane, 9).Possible);
        Assert.IsFalse(CardMoves.Up(Lane, -1).Possible);
    }

    [TestMethod]
    public void A_card_goes_down_before_the_card_two_below_or_to_the_end_and_nowhere_from_the_bottom()
    {
        Assert.AreEqual(new CardStep(true, "c"), CardMoves.Down(Lane, 0));
        Assert.AreEqual(new CardStep(true, "d"), CardMoves.Down(Lane, 1));
        Assert.AreEqual(new CardStep(true, null), CardMoves.Down(Lane, 2), "the next is the last, so the card goes to the end");
        Assert.IsFalse(CardMoves.Down(Lane, 3).Possible);
        Assert.IsFalse(CardMoves.Down(Lane, -1).Possible);
    }

    [TestMethod]
    public void A_card_goes_across_at_its_own_height_or_to_the_end_of_a_shorter_lane()
    {
        string[] shorter = ["x", "y"];
        Assert.AreEqual(new CardStep(true, "y"), CardMoves.Across(shorter, 1));
        Assert.AreEqual(new CardStep(true, null), CardMoves.Across(shorter, 2));
        Assert.AreEqual(new CardStep(true, null), CardMoves.Across([], 0));
    }

    [TestMethod]
    public void The_lane_across_is_the_next_or_the_one_before_and_none_past_the_ends()
    {
        Assert.AreEqual(1, CardMoves.Neighbour(0, 3, 1));
        Assert.AreEqual(0, CardMoves.Neighbour(1, 3, -1));
        Assert.AreEqual(-1, CardMoves.Neighbour(2, 3, 1));
        Assert.AreEqual(-1, CardMoves.Neighbour(0, 3, -1));
        Assert.AreEqual(-1, CardMoves.Neighbour(-1, 3, 1));
    }

    [TestMethod]
    public void A_card_let_go_stands_before_the_first_card_whose_middle_is_below_it()
    {
        double[] middles = [20, 70, 120, 170];
        Assert.AreEqual("a", CardMoves.DropBefore(Lane, middles, 5));
        Assert.AreEqual("b", CardMoves.DropBefore(Lane, middles, 40));
        Assert.AreEqual("d", CardMoves.DropBefore(Lane, middles, 150));
        Assert.IsNull(CardMoves.DropBefore(Lane, middles, 400), "below every card: at the end");
        Assert.IsNull(CardMoves.DropBefore([], [], 10));
    }

    [TestMethod]
    public void A_move_is_offered_only_for_a_card_that_has_an_id()
    {
        Assert.IsTrue(CardMoves.Offered("card-7"));

        // Two cards that read "Call client" and have no id cannot be told apart: neither is offered a move.
        Assert.IsFalse(CardMoves.Offered(null));
        Assert.IsFalse(CardMoves.Offered(string.Empty));
    }

    [TestMethod]
    public void A_lane_moves_a_card_only_for_the_drag_of_a_card_that_is_still_on_the_screen()
    {
        // A card is picked up and dragged over a lane: it is the card that moves, whether or not the drag says which card it is.
        Assert.AreEqual(("card-7", "todo"), CardMoves.Dragged("card-7", "todo", sourceShown: true, carriesMark: true, carriedRow: "card-7"));
        Assert.AreEqual(("card-7", "todo"), CardMoves.Dragged("card-7", "todo", sourceShown: true, carriesMark: false, carriedRow: null));

        // The card was replaced by an answer while it was dragged, and never heard the drag end. Words dragged in later from another app
        // move nothing: the card that was picked up is no longer on the screen.
        Assert.IsNull(CardMoves.Dragged("card-7", "todo", sourceShown: false, carriesMark: false, carriedRow: null), "a drag whose card has left the screen moved a card");
        Assert.IsNull(CardMoves.Dragged("card-7", "todo", sourceShown: false, carriesMark: true, carriedRow: "card-7"));

        // What is dragged says it is another card than the one held.
        Assert.IsNull(CardMoves.Dragged("card-7", "todo", sourceShown: true, carriesMark: true, carriedRow: "card-9"), "a drag of another card moved the card held");
        Assert.IsNull(CardMoves.Dragged("card-7", "todo", sourceShown: true, carriesMark: true, carriedRow: null));

        // No card is being dragged.
        Assert.IsNull(CardMoves.Dragged(null, null, sourceShown: true, carriesMark: false, carriedRow: null));
        Assert.IsNull(CardMoves.Dragged("card-7", null, sourceShown: true, carriesMark: false, carriedRow: null));
    }
}
