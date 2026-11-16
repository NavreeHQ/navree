using Microsoft.UI.Xaml;
using Navree.Presence;
using Navree.Protocol;
using Navree.Runtime;
using Navree.Shell;
using Windows.Graphics.Imaging;
using Windows.Storage.Streams;

namespace Navree.Demo;

/// <summary>
/// Walks the app's place in Windows: the count on the taskbar, the icon in the notification area and the small window by it, the settings page,
/// opening at sign-in, a notification, a window of its own for a screen, the full screen, the offer to start again, the title bar's search. After
/// each it takes a picture of the screen as the system shows it, and every part writes what it answered to <c>system-walk.txt</c>, so a run says
/// what the system accepted and what it only was asked.
/// </summary>
internal sealed class SystemWalker(ShellHost host, ShellWindow window, SystemPlace place, string folder)
{
    private readonly List<string> log = [];

    internal async Task RunAsync()
    {
        string prefix = Prefix();
        string[] arguments = Environment.GetCommandLineArgs();
        try
        {
            Directory.CreateDirectory(folder);
            if (Array.IndexOf(arguments, "--offline") >= 0)
            {
                await OfflineAsync(prefix);
                return;
            }

            await ReadyAsync();
            window.Size(1180, 800);
            await Task.Delay(1500);
            Say("package identity: " + place.HasIdentity);
            foreach (FeatureOffer offer in place.Offers)
            {
                Say("offer " + offer.Feature + ": " + (offer.Offered ? "yes, with " + offer.Means : "no, " + offer.Reason));
            }

            await Screen(prefix, "main");

            if (Array.IndexOf(arguments, "--search") >= 0)
            {
                await Step("the title bar's search", async () =>
                {
                    host.Send(Events.Select(1));
                    await Task.Delay(2500);
                    Say("the screen has a search: " + (host.State.FrontScreen?.Screen?.Search is not null));
                    await Content(prefix, "title-bar");
                    await Screen(prefix, "screen");
                });
                return;
            }

            await Step("the count on the taskbar button", async () =>
            {
                place.SetBadge(7);
                await Task.Delay(2000);
                await Screen(prefix, "badge");
            });

            await Step("the icon in the notification area", async () =>
            {
                place.Choose(CardIds.Tray, true);
                await Task.Delay(1500);
                Say("the icon is in the notification area: " + place.TrayShown);
            });

            await Step("the small window by the icon", async () =>
            {
                place.ToggleSmallWindow();
                await Task.Delay(1800);
                Say("the small window is showing: " + place.SmallWindowShown);
                await Screen(prefix, "small-window");
                place.ToggleSmallWindow();
                await Task.Delay(500);
            });

            await Step("settings", async () =>
            {
                place.OpenSettings();
                await Task.Delay(1500);
                await Content(prefix, "settings");
                await Screen(prefix, "settings-screen");
            });

            await Step("opening at sign-in", async () =>
            {
                place.Choose(CardIds.Startup, true);
                await Task.Delay(2000);
                Say("opening at sign-in is " + place.Startup);
                await Content(prefix, "settings-startup-on");
                place.Choose(CardIds.Startup, false);
                await Task.Delay(1500);
                Say("opening at sign-in is " + place.Startup + " again");
            });

            await Step("a notification", async () =>
            {
                place.Notify(new Models.Notify
                {
                    Id = "demo-shipped",
                    Title = "Order CS-20481 has shipped",
                    Body = "Tanvi Sharma, Pune. Expected on 14 Nov.",
                    Href = "/orders/1",
                    At = new Models.WhenAfter { Secs = 1 },
                });
                await Task.Delay(5000);
                await Screen(prefix, "notification");
            });

            await Step("a window of its own for a screen", async () =>
            {
                place.OpenMainWindow();
                window.HideSettings();
                host.Send(Events.Select(1));
                await Task.Delay(2500);
                Say("a window was opened: " + place.OpenWindow());
                await Task.Delay(2500);
                Say("windows besides the main one: " + place.ExtraWindows);
                await Screen(prefix, "two-windows");
            });

            await Step("the full screen", async () =>
            {
                place.OpenMainWindow();
                window.ToggleFullScreen();
                await Task.Delay(2000);
                Say("the window takes the whole screen: " + window.IsFullScreen);
                await Task.Delay(1500);
                Say("the window's content is " + window.Surface.ActualWidth + " by " + window.Surface.ActualHeight + " effective pixels");
                if (window.CurrentPage is Microsoft.UI.Xaml.FrameworkElement page)
                {
                    double left = page.TransformToVisual(window.Surface).TransformPoint(new Windows.Foundation.Point(0, 0)).X;
                    Say("the page is " + page.ActualWidth + " wide, from " + left);
                    Microsoft.UI.Xaml.DependencyObject? above = page;
                    while (above is Microsoft.UI.Xaml.FrameworkElement element)
                    {
                        try
                        {
                            double across = element.TransformToVisual(window.Surface).TransformPoint(new Windows.Foundation.Point(0, 0)).X;
                            Say("  inside " + element.GetType().Name + ": " + element.ActualWidth + " wide, from " + across);
                        }
                        catch (Exception)
                        {
                            Say("  inside " + element.GetType().Name + ": " + element.ActualWidth + " wide");
                        }

                        above = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(element);
                    }
                }

                await Screen(prefix, "full-screen");
                await Content(prefix, "full-screen-content");
                window.ToggleFullScreen();
                await Task.Delay(1500);
                Say("the window takes the whole screen: " + window.IsFullScreen + " again");
            });

            await Step("the share sheet", async () =>
            {
                bool? shared = await place.ShareAsync(
                    new Models.Share { Title = "Order CS-20481", Text = "Order CS-20481 for Tanvi Sharma, Pune, has shipped." },
                    _ => false);
                Say("the share sheet answered: " + (shared is null ? "it could not be shown" : shared.Value.ToString()));
            }).WaitAsync(TimeSpan.FromSeconds(20)).ContinueWith(_ => { }, TaskScheduler.Default);

            await Step("the offer to start again", async () =>
            {
                // What a core answers once it has stopped, as today's core writes it (its words, with no mark).
                host.ReportCoreAnswer("{\"error\":" + System.Text.Json.JsonSerializer.Serialize(CoreAnswers.StoppedWords) + "}");
                await Task.Delay(1500);
                await Content(prefix, "stopped");
            });
        }
        catch (Exception problem)
        {
            Say("the walk failed: " + problem);
        }
        finally
        {
            foreach (string line in place.Log)
            {
                log.Add("place: " + line);
            }

            Say("done");
            File.WriteAllLines(Path.Combine(folder, prefix + "system-walk.txt"), log);
            App.Finish(place);
        }
    }

    // The first load fails: the app has no connection, and says so with a way to try again.
    private async Task OfflineAsync(string prefix)
    {
        await Task.Delay(7000);
        Say("how far the app got: " + host.State.Nav.Stage + ", why: " + host.State.Nav.Failure);
        window.Size(1000, 700);
        await Task.Delay(1500);
        await Content(prefix, "first-load-failed");
        await Screen(prefix, "first-load-failed-screen");
    }

    private async Task Step(string what, Func<Task> run)
    {
        try
        {
            Say("== " + what);
            await run();
        }
        catch (Exception problem)
        {
            Say(what + " failed: " + problem.GetType().Name + ": " + problem.Message);
        }
    }

    private async Task Screen(string prefix, string name)
    {
        string file = Path.Combine(folder, prefix + name + ".png");
        try
        {
            await ScreenPicture.SaveAsync(file);
            Say("wrote " + Path.GetFileName(file));
        }
        catch (Exception problem)
        {
            Say("no picture " + Path.GetFileName(file) + ": " + problem.Message);
        }
    }

    private async Task Content(string prefix, string name)
    {
        string file = Path.Combine(folder, prefix + name + ".png");
        try
        {
            await WindowPicture.SaveAsync((Microsoft.UI.Xaml.Controls.Panel)window.Surface, file);
            Say("wrote " + Path.GetFileName(file));
        }
        catch (Exception problem)
        {
            Say("no picture " + Path.GetFileName(file) + ": " + problem.Message);
        }
    }

    private async Task ReadyAsync()
    {
        for (int round = 0; round < 60; round++)
        {
            ViewState state = await host.SettledAsync();
            if (state.Nav.Stage == Stage.Ready)
            {
                return;
            }

            await Task.Delay(250);
        }

        Say("the app did not get ready");
    }

    private static string Prefix()
    {
        string[] arguments = Environment.GetCommandLineArgs();
        string theme = Array.IndexOf(arguments, "--dark") >= 0 ? "dark" : "light";
        string packaged = PackageIdentity.Has() ? "packaged-" : "unpackaged-";
        return packaged + theme + (Array.IndexOf(arguments, "--rtl") >= 0 ? "-rtl" : string.Empty) + (Array.IndexOf(arguments, "--offline") >= 0 ? "-offline" : string.Empty) + (Array.IndexOf(arguments, "--search") >= 0 ? "-search" : string.Empty) + "-";
    }

    private void Say(string line) => log.Add(DateTime.Now.ToString("HH:mm:ss.fff", System.Globalization.CultureInfo.InvariantCulture) + " " + line);
}

/// <summary>Draws a part of the screen, as the system shows it, to a PNG file: the windows, the taskbar, the notification area.</summary>
internal static class ScreenPicture
{
    internal static async Task SaveAsync(string file, Box? region = null)
    {
        Box area = region ?? ScreenCapture.Screen();
        byte[] pixels = ScreenCapture.Grab(area) ?? throw new InvalidOperationException("the screen could not be copied");
        using InMemoryRandomAccessStream stream = new();
        BitmapEncoder encoder = await BitmapEncoder.CreateAsync(BitmapEncoder.PngEncoderId, stream);
        encoder.SetPixelData(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Ignore, (uint)area.Width, (uint)area.Height, 96, 96, pixels);
        await encoder.FlushAsync();

        byte[] png = new byte[stream.Size];
        using (DataReader reader = new(stream.GetInputStreamAt(0)))
        {
            await reader.LoadAsync((uint)stream.Size);
            reader.ReadBytes(png);
        }

        await File.WriteAllBytesAsync(file, png);
    }
}
