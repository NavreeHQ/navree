using Navree.Runtime;

namespace Navree.Platform.Tests;

/// <summary>One part that fails does not cost the rest of an update, and a fetcher that throws is a request that failed.</summary>
[TestClass]
public sealed class DeliveryTests
{
    private sealed class Throwing(Exception failure) : IFetcher
    {
        public Task<FetchResult> FetchAsync(FetchEffect effect, CancellationToken cancel) => throw failure;
    }

    private static FetchEffect Request() => new(7, "get", "https://shop.example/", [], null, 1000, false);

    private static Update Said(params Effect[] effects) => new(ViewState.Empty, effects, ReadOnlyMemory<byte>.Empty);

    [TestMethod]
    public void A_window_that_cannot_show_the_state_does_not_lose_the_effects_of_the_update()
    {
        List<string> problems = [];
        List<Effect> run = [];
        Effect[] effects = [new ReportEffect("a", "b"), new RandomEffect(1, 4)];
        Delivery.Deliver(Said(effects), _ => throw new InvalidOperationException("the window"), run.Add, problems.Add);
        CollectionAssert.AreEqual(effects, run);
        Assert.AreEqual(1, problems.Count);
        StringAssert.Contains(problems[0], nameof(InvalidOperationException));
    }

    [TestMethod]
    public void An_effect_that_fails_does_not_stop_the_ones_after_it_and_is_reported_by_its_kind_alone()
    {
        List<string> problems = [];
        List<Effect> run = [];
        Effect first = new ReportEffect("a", "secret detail");
        Effect second = new RandomEffect(1, 4);
        Delivery.Deliver(
            Said(first, second),
            _ => { },
            effect =>
            {
                run.Add(effect);
                if (effect == first)
                {
                    throw new IOException("secret detail");
                }
            },
            problems.Add);
        CollectionAssert.AreEqual(new[] { first, second }, run);
        Assert.AreEqual(1, problems.Count);
        Assert.IsFalse(problems[0].Contains("secret", StringComparison.Ordinal), problems[0]);
    }

    [TestMethod]
    public void An_effect_this_build_does_not_know_is_passed_over_and_the_rest_of_the_update_is_carried_out()
    {
        // A later core says something this build has no word for, between two things it knows, while a person is signing out.
        List<string> problems = [];
        List<Effect> run = [];
        Effect[] effects = [new ReportEffect("a", "b"), new UnknownEffect("session_paused"), new CancelEffect(7), new RandomEffect(1, 4)];
        SessionGate gate = new();
        long[] marks = gate.Asked(effects);
        gate.SessionIsEnding();
        int at = 0;
        Delivery.Deliver(
            Said(effects),
            _ => { },
            effect =>
            {
                // As the host carries an effect out: the gate is asked, and what this build does not know is said and left.
                if (gate.MayRun(effect, marks[at++]))
                {
                    run.Add(effect);
                }
            },
            problems.Add);
        CollectionAssert.AreEqual(effects, run, "an effect this build does not know, or one beside it, was not handed on");
        Assert.AreEqual(0, problems.Count, string.Join(" | ", problems));
        Expect.That(!SessionGate.StartsOrKeeps(effects[1]), "an effect this build does not know is taken for one that starts or keeps something");
    }

    [TestMethod]
    public async Task A_fetcher_that_throws_answers_the_core_with_a_failure()
    {
        FetchResult answer = await Delivery.FetchAsync(new Throwing(new InvalidOperationException()), Request(), CancellationToken.None);
        Assert.AreEqual("other", answer.Error);
        Assert.AreEqual(0, answer.Status);
    }

    [TestMethod]
    public async Task A_request_that_was_cancelled_is_not_answered()
    {
        using CancellationTokenSource gone = new();
        await gone.CancelAsync();
        await Assert.ThrowsExactlyAsync<OperationCanceledException>(
            () => Delivery.FetchAsync(new Throwing(new OperationCanceledException(gone.Token)), Request(), gone.Token));
    }
}
