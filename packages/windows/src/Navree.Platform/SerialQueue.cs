using System.Threading.Channels;

namespace Navree.Platform;

/// <summary>
/// Runs what is given to it one after another in the order it was given, on a thread of its own: what is kept on the device is
/// written in the order it was asked for, and a clear given to the queue runs after every write given before it.
/// </summary>
public sealed class SerialQueue : IDisposable
{
    private readonly Channel<Action> work = Channel.CreateUnbounded<Action>(new UnboundedChannelOptions { SingleReader = true });
    private readonly Thread thread;

    /// <summary>Starts the queue's thread.</summary>
    public SerialQueue()
    {
        thread = new Thread(Loop) { IsBackground = true, Name = "Navree disk" };
        thread.Start();
    }

    /// <summary>Queues an action. One that throws is passed over and the next runs.</summary>
    /// <param name="action">The action.</param>
    public void Run(Action action) => work.Writer.TryWrite(action);

    /// <summary>Queues an action and completes when it has run, with what it threw; faulted at once when the queue has ended.</summary>
    /// <param name="action">The action.</param>
    /// <returns>When it has run.</returns>
    public Task RunAsync(Action action)
    {
        ArgumentNullException.ThrowIfNull(action);
        TaskCompletionSource done = new(TaskCreationOptions.RunContinuationsAsynchronously);
        bool queued = work.Writer.TryWrite(() =>
        {
            try
            {
                action();
                done.SetResult();
            }
            catch (Exception problem)
            {
                done.SetException(problem);
            }
        });
        if (!queued)
        {
            done.SetException(new ObjectDisposedException(nameof(SerialQueue)));
        }

        return done.Task;
    }

    /// <inheritdoc />
    public void Dispose()
    {
        work.Writer.TryComplete();
        thread.Join(TimeSpan.FromSeconds(2));
    }

    private void Loop()
    {
        while (work.Reader.WaitToReadAsync().AsTask().GetAwaiter().GetResult())
        {
            while (work.Reader.TryRead(out Action? action))
            {
                try
                {
                    action();
                }
                catch (Exception)
                {
                    // A write that fails leaves the next one to run: what is kept is a convenience, never a reason to stop.
                }
            }
        }
    }
}
