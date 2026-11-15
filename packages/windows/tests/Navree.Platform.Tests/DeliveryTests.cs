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
