using System.Net;
using System.Net.Sockets;

namespace Navree.Platform.Tests;

/// <summary>A name with two addresses, the first of which accepts and never answers, is connected by the second after a short wait.</summary>
[TestClass]
public sealed class ConnectingTests
{
    private static readonly IPAddress First = IPAddress.IPv6Loopback;
    private static readonly IPAddress Second = IPAddress.Loopback;

    [TestMethod]
    public async Task An_address_that_never_answers_costs_the_wait_and_not_the_time_out()
    {
        List<string> tried = [];
        CancellationToken firstWasCancelled = default;
        TaskCompletionSource firstEnded = new(TaskCreationOptions.RunContinuationsAsynchronously);

        async Task<string> Attempt(IPAddress address, CancellationToken cancel)
        {
            tried.Add(address.ToString());
            if (address.Equals(First))
            {
                firstWasCancelled = cancel;
                try
                {
                    await Task.Delay(Timeout.InfiniteTimeSpan, cancel);
                }
                finally
                {
                    firstEnded.TrySetResult();
                }
            }

            return "connected to " + address;
        }

        DateTime started = DateTime.UtcNow;
        string result = await Connecting.FirstAsync([First, Second], TimeSpan.FromMilliseconds(80), Attempt, _ => { }, CancellationToken.None);
        TimeSpan took = DateTime.UtcNow - started;

        Assert.AreEqual("connected to " + Second, result);
        Assert.IsLessThan(TimeSpan.FromSeconds(5), took, "the second address was tried after the wait, not after a time out");
        CollectionAssert.AreEqual(new[] { First.ToString(), Second.ToString() }, tried);
        await firstEnded.Task.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.IsTrue(firstWasCancelled.IsCancellationRequested, "the attempt that never answered is cancelled once another has");
    }

    [TestMethod]
    public async Task An_address_that_fails_at_once_is_followed_by_the_next_at_once()
    {
        List<string> tried = [];

        Task<string> Attempt(IPAddress address, CancellationToken cancel)
        {
            tried.Add(address.ToString());
            return address.Equals(First)
                ? Task.FromException<string>(new SocketException((int)SocketError.ConnectionRefused))
                : Task.FromResult("connected to " + address);
        }

        string result = await Connecting.FirstAsync([First, Second], TimeSpan.FromSeconds(30), Attempt, _ => { }, CancellationToken.None);

        Assert.AreEqual("connected to " + Second, result);
        Assert.HasCount(2, tried);
    }

    [TestMethod]
    public async Task When_every_address_fails_the_last_failure_is_told()
    {
        static Task<string> Attempt(IPAddress address, CancellationToken cancel) =>
            Task.FromException<string>(new SocketException((int)SocketError.ConnectionRefused));

        await Assert.ThrowsExactlyAsync<SocketException>(async () =>
            await Connecting.FirstAsync([First, Second], TimeSpan.FromMilliseconds(10), Attempt, _ => { }, CancellationToken.None));
    }

    [TestMethod]
    public async Task A_connection_that_comes_after_another_has_won_is_let_go()
    {
        TaskCompletionSource<string> slow = new(TaskCreationOptions.RunContinuationsAsynchronously);
        TaskCompletionSource<string> released = new(TaskCreationOptions.RunContinuationsAsynchronously);

        Task<string> Attempt(IPAddress address, CancellationToken cancel) => address.Equals(First) ? slow.Task : Task.FromResult("second");

        string result = await Connecting.FirstAsync([First, Second], TimeSpan.FromMilliseconds(20), Attempt, found => released.TrySetResult(found), CancellationToken.None);
        Assert.AreEqual("second", result);

        slow.SetResult("first, late");
        Assert.AreEqual("first, late", await released.Task.WaitAsync(TimeSpan.FromSeconds(5)));
    }

    [TestMethod]
    public async Task A_name_with_no_address_is_not_found()
    {
        await Assert.ThrowsExactlyAsync<SocketException>(async () =>
            await Connecting.FirstAsync<string>([], TimeSpan.FromMilliseconds(10), (_, _) => Task.FromResult("x"), _ => { }, CancellationToken.None));
    }
}
