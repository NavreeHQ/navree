using System.Threading.Channels;

namespace Navree.Shell;

/// <summary>
/// Runs what is given to it one after another in the order it was given, on a thread of its own: what the core asks
/// to be kept is written in the order it asks, so a write after a clear always survives it.
/// </summary>
internal sealed class SerialQueue : IDisposable
{
    private readonly Channel<Action> work = Channel.CreateUnbounded<Action>(new UnboundedChannelOptions { SingleReader = true });
    private readonly Thread thread;

    internal SerialQueue()
    {
        thread = new Thread(Loop) { IsBackground = true, Name = "Navree disk" };
        thread.Start();
    }

    /// <summary>Queues an action.</summary>
    internal void Run(Action action) => work.Writer.TryWrite(action);

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
