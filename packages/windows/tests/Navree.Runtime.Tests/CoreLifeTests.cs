using System.Text;
using Navree.Core;

namespace Navree.Runtime.Tests;

/// <summary>A core that cannot be had is said in words, and work asked of a client that has ended is answered, never left waiting.</summary>
[TestClass]
public sealed class CoreLifeTests
{
    private static readonly byte[] Config = Encoding.UTF8.GetBytes("""{"origin":"https://shop.example","client":{"platform":"android","form":"phone","vocab":0,"text_percent":100}}""");
    private static readonly TimeSpan Short = TimeSpan.FromSeconds(5);

    private static string CoreFolder => Path.GetDirectoryName(typeof(NavreeCore).Assembly.Location)!;

    // Starts a client whose core cannot be made, on a thread of the test's own, and says what the start ended with.
    private static Exception? StartWith(Exception loading) =>
        Task.Run(() =>
        {
            try
            {
                using CoreClient client = new(Config, _ => throw loading, _ => { });
                return (Exception?)null;
            }
            catch (Exception problem)
            {
                return problem;
            }
        }).WaitAsync(Short).GetAwaiter().GetResult();

    [TestMethod]
    public void A_core_library_that_is_not_there_ends_the_start_in_words_that_name_the_file_and_where_it_was_looked_for()
    {
        Exception? problem = StartWith(new DllNotFoundException("Unable to load DLL 'navree_native_core' or one of its dependencies"));

        Assert.IsNotNull(problem, "the start went on without a core");
        Expect.That(problem.Message.Contains("navree_native_core.dll", StringComparison.Ordinal), "the words do not name the file: " + problem.Message);
        Expect.That(problem.Message.Contains(CoreFolder, StringComparison.OrdinalIgnoreCase), "the words do not say where it was looked for: " + problem.Message);
        Expect.That(problem.InnerException is DllNotFoundException, "what the system said is kept for whoever looks into it");
    }

    [TestMethod]
    public void A_core_library_for_another_processor_ends_the_start_in_words_that_say_so()
    {
        Exception? problem = StartWith(new BadImageFormatException("An attempt was made to load a program with an incorrect format."));

        Assert.IsNotNull(problem, "the start went on without a core");
        Expect.That(problem.Message.Contains("navree_native_core.dll", StringComparison.Ordinal), "the words do not name the file: " + problem.Message);
        Expect.That(problem.Message.Contains("processor", StringComparison.Ordinal), "the words do not say it is for another processor: " + problem.Message);
    }

    [TestMethod]
    public async Task Work_asked_after_the_client_ended_is_answered_at_once_as_ended()
    {
        CoreClient client = new(Config, _ => { });
        client.Dispose();

        foreach (Task work in new Task[] { client.SendAsync(Events.Start), client.ReadAllAsync(), client.SnapshotAsync() })
        {
            Exception? ended = null;
            try
            {
                await work.WaitAsync(Short);
            }
            catch (Exception problem)
            {
                ended = problem;
            }

            Expect.That(ended is ObjectDisposedException, "work asked after the end was answered with " + (ended?.GetType().Name ?? "a result"));
        }

        client.Send(Events.Start);
        client.Dispose();
    }

    [TestMethod]
    public async Task Work_raced_against_the_end_is_answered_or_dropped_and_never_left_waiting()
    {
        for (int round = 0; round < 20; round++)
        {
            CoreClient client = new(Config, _ => { });
            List<Task> asked = [];
            using ManualResetEventSlim go = new(false);
            Task asking = Task.Run(() =>
            {
                go.Wait();
                for (int each = 0; each < 200; each++)
                {
                    lock (asked)
                    {
                        asked.Add(each % 3 == 0 ? client.SendAsync(Events.Start) : each % 3 == 1 ? client.ReadAllAsync() : client.SnapshotAsync());
                    }
                }
            });
            go.Set();
            await Task.Delay(round % 5);
            client.Dispose();
            await asking.WaitAsync(Short);

            Task[] all;
            lock (asked)
            {
                all = [.. asked];
            }

            try
            {
                await Task.WhenAll(all).WaitAsync(Short);
            }
            catch (ObjectDisposedException)
            {
                // Answered as ended: allowed.
            }

            Expect.That(all.All(work => work.IsCompleted), "round " + round + ": " + all.Count(work => !work.IsCompleted) + " of " + all.Length + " were left waiting");
            Expect.That(all.Where(work => work.IsFaulted).All(work => work.Exception!.InnerException is ObjectDisposedException), "work failed otherwise than as ended");
        }
    }

    [TestMethod]
    public async Task What_is_to_run_just_before_the_core_is_told_runs_on_the_cores_thread_after_the_updates_before_it_and_before_its_own()
    {
        List<string> order = [];
        int updatesOn = 0;
        using CoreClient client = new(
            Config,
            _ =>
            {
                lock (order)
                {
                    order.Add("update");
                    updatesOn = Environment.CurrentManagedThreadId;
                }
            });

        // The app starts; then the core is told one more thing, with a mark to be made just before it is told.
        client.Send(Events.Start);
        int markedOn = 0;
        _ = await client.SendAsync(
            Events.Background,
            () =>
            {
                lock (order)
                {
                    order.Add("just before");
                    markedOn = Environment.CurrentManagedThreadId;
                }
            }).WaitAsync(Short);

        string[] seen;
        lock (order)
        {
            seen = [.. order];
        }

        CollectionAssert.AreEqual(new[] { "update", "just before", "update" }, seen, "the mark was not made between the update before it and its own: " + string.Join(", ", seen));
        Assert.AreEqual(updatesOn, markedOn, "the mark was made on another thread than the one the core's updates are handed on from");
    }
}
