using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Navree.Shell;

namespace Navree.Demo;

/// <summary>
/// The demo's entry point. Before any window is made it asks whether the app is already running: a second start hands what it was asked to
/// the first and ends. A run that walks the app's screens for pictures lets every start run, so one walk never waits on another.
/// </summary>
public static class Program
{
    [STAThread]
    private static int Main(string[] args)
    {
        WinRT.ComWrappersSupport.InitializeComWrappers();
        bool walking = Array.IndexOf(args, "--walk") >= 0 || Array.IndexOf(args, "--system-walk") >= 0 || Array.IndexOf(args, "--kinds") >= 0;
        if (!Activation.Claim(App.InstanceKey, App.Scheme, single: !walking))
        {
            return 0;
        }

        Application.Start(parameters =>
        {
            DispatcherQueueSynchronizationContext context = new(DispatcherQueue.GetForCurrentThread());
            SynchronizationContext.SetSynchronizationContext(context);
            _ = new App();
        });
        return 0;
    }
}
