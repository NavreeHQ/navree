using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Navree.Protocol;
using Navree.Shell;
using Windows.Graphics;
using Windows.Graphics.Imaging;
using Windows.Storage.Streams;

namespace Navree.Demo;

/// <summary>
/// Shows each screen of the drawn kinds (pages written for the purpose, and the protocol's own example screens) with no core behind it,
/// at three widths, and takes a picture of each page; one at twice the size, to see that the drawing is sharp. What was done, and
/// anything that went wrong, is written to <c>kinds.txt</c> beside the pictures.
/// </summary>
internal sealed class KindsWalker(Window window, string folder)
{
    private static readonly int[] Widths = [480, 840, 1280];
    private readonly List<string> log = [];

    internal async Task RunAsync()
    {
        string prefix = Prefix();
        try
        {
            Directory.CreateDirectory(folder);
            ScrollViewer scroller = new() { VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto };
            Grid root = new();
            root.Children.Add(scroller);
            if (Array.IndexOf(Environment.GetCommandLineArgs(), "--rtl") >= 0)
            {
                root.FlowDirection = FlowDirection.RightToLeft;
            }

            window.Content = root;
            await Task.Delay(800);

            foreach ((string name, string json) in Sources())
            {
                Models.Screen screen;
                try
                {
                    screen = KindsGallery.Read(json);
                }
                catch (Exception problem)
                {
                    Say(name + ": could not be read: " + problem.Message);
                    continue;
                }

                foreach (int width in Widths)
                {
                    double scale = root.XamlRoot?.RasterizationScale ?? 1.0;
                    window.AppWindow.Resize(new SizeInt32(Navree.Design.WindowRules.Raw(width + 16, scale), Navree.Design.WindowRules.Raw(1000, scale)));
                    await Task.Delay(300);
                    FrameworkElement page = KindsGallery.Draw(screen, width);
                    scroller.Content = page;
                    await Task.Delay(900);
                    page.UpdateLayout();
                    string file = Path.Combine(folder, prefix + name + "-w" + width + ".png");
                    await SaveAsync(page, file, 1, name);
                    if (width == 840)
                    {
                        // What a person sees where the pointer is (or the arrow keys have reached) in each drawing.
                        int read = Navree.Controls.Drawing.Readouts.Show(page, 2);
                        await Task.Delay(300);
                        await SaveAsync(page, Path.Combine(folder, prefix + name + "-w840-readout.png"), 1, name);
                        Say(name + ": a place read out in " + read + " drawings");
                        Navree.Controls.Drawing.Readouts.Clear(page);
                    }

                    if (width == 840 && name == "a-charts")
                    {
                        await RealInputAsync(page, prefix, name);
                    }

                    if (width == 840 && prefix == "light-")
                    {
                        await SaveAsync(page, Path.Combine(folder, prefix + name + "-w840-x2.png"), 2, name);
                        await InspectAsync(Path.Combine(folder, prefix + name + "-w840-ui.txt"));
                    }
                }
            }
        }
        catch (Exception problem)
        {
            Say("the walk failed: " + problem);
        }
        finally
        {
            Say("done");
            File.WriteAllLines(Path.Combine(folder, prefix + "kinds.txt"), log);
            Application.Current.Exit();
        }
    }

    // A real pointer over a chart and real arrow keys on one: which place each read out is written to the log, and each is pictured.
    private async Task RealInputAsync(FrameworkElement page, string prefix, string name)
    {
        try
        {
            IntPtr handle = WinRT.Interop.WindowNative.GetWindowHandle(window);
            RealInput.Forward(handle);
            await Task.Delay(300);
            double scale = page.XamlRoot?.RasterizationScale ?? 1;
            Windows.Foundation.Point? spot = Navree.Controls.Drawing.FirstDrawing.Where(page, window.Content, 3);
            if (spot is Windows.Foundation.Point found)
            {
                RealInput.PointerTo(handle, found.X * scale, found.Y * scale);
                await Task.Delay(500);
                (int px, int py) = RealInput.Where();
                Say(name + ": the pointer was put on place 3 of the first chart (the content point " + (int)found.X + "," + (int)found.Y + ", the screen point " + px + "," + py + "); it reads out place " + Navree.Controls.Drawing.FirstDrawing.Current(page));
                await SaveAsync(page, Path.Combine(folder, prefix + name + "-w840-pointer.png"), 1, name);
            }
            else
            {
                Say(name + ": no chart to put a pointer on");
            }

            Navree.Controls.Drawing.Readouts.Clear(page);
            RealInput.PointerTo(handle, 2, 2);
            await Task.Delay(200);
            bool focused = Navree.Controls.Drawing.FirstDrawing.Focus(page);
            await Task.Delay(300);
            int firstPlace = Navree.Controls.Drawing.FirstDrawing.Current(page);
            RealInput.Press(0x27);
            await Task.Delay(150);
            RealInput.Press(0x27);
            await Task.Delay(300);
            Say(name + ": the first chart took the focus " + focused + ", read out place " + firstPlace + " at once and place " + Navree.Controls.Drawing.FirstDrawing.Current(page) + " after the right arrow twice");
            await SaveAsync(page, Path.Combine(folder, prefix + name + "-w840-keys.png"), 1, name);
            Navree.Controls.Drawing.Readouts.Clear(page);
        }
        catch (Exception problem)
        {
            Say("real input failed: " + problem.Message);
        }
    }

    private static string Prefix()
    {
        string[] arguments = Environment.GetCommandLineArgs();
        string theme = Array.IndexOf(arguments, "--dark") >= 0 ? "dark" : "light";
        return theme + (Array.IndexOf(arguments, "--rtl") >= 0 ? "-rtl" : string.Empty) + "-";
    }

    // The pages written for the purpose, then the protocol's own example screens, found above the program in the repository.
    private static IEnumerable<(string Name, string Json)> Sources()
    {
        DirectoryInfo? at = new(AppContext.BaseDirectory);
        while (at is not null)
        {
            string pages = Path.Combine(at.FullName, "packages", "windows", "demo", "Navree.Demo", "kinds");
            if (Directory.Exists(pages))
            {
                foreach (string file in Directory.GetFiles(pages, "*.json").OrderBy(path => path, StringComparer.Ordinal))
                {
                    yield return (Path.GetFileNameWithoutExtension(file), File.ReadAllText(file));
                }

                string corpus = Path.Combine(at.FullName, "crates", "navree-native", "corpus");
                foreach (string name in new[] { "today", "sleep", "fitness", "designed-health", "dashboard", "designed-store", "inbox", "kinds" })
                {
                    string file = Path.Combine(corpus, "screen-" + name + ".json");
                    if (File.Exists(file))
                    {
                        yield return ("example-" + name, File.ReadAllText(file));
                    }
                }

                yield break;
            }

            at = at.Parent;
        }
    }

    private async Task SaveAsync(FrameworkElement page, string file, double scale, string name)
    {
        try
        {
            RenderTargetBitmap bitmap = new();
            Panel? panel = page as Panel;
            if (panel is not null)
            {
                panel.Background = new SolidColorBrush(
                    page.ActualTheme == ElementTheme.Dark
                        ? Microsoft.UI.ColorHelper.FromArgb(0xFF, 0x20, 0x20, 0x20)
                        : Microsoft.UI.ColorHelper.FromArgb(0xFF, 0xF3, 0xF3, 0xF3));
            }

            if (scale == 1)
            {
                await bitmap.RenderAsync(page);
            }
            else
            {
                await bitmap.RenderAsync(page, (int)(page.ActualWidth * scale), (int)(page.ActualHeight * scale));
            }

            IBuffer pixels = await bitmap.GetPixelsAsync();
            byte[] raw = new byte[pixels.Length];
            using (DataReader reader = DataReader.FromBuffer(pixels))
            {
                reader.ReadBytes(raw);
            }

            using InMemoryRandomAccessStream stream = new();
            BitmapEncoder encoder = await BitmapEncoder.CreateAsync(BitmapEncoder.PngEncoderId, stream);
            encoder.SetPixelData(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Premultiplied, (uint)bitmap.PixelWidth, (uint)bitmap.PixelHeight, 96 * scale, 96 * scale, raw);
            await encoder.FlushAsync();
            byte[] png = new byte[stream.Size];
            using (DataReader reader = new(stream.GetInputStreamAt(0)))
            {
                await reader.LoadAsync((uint)stream.Size);
                reader.ReadBytes(png);
            }

            await File.WriteAllBytesAsync(file, png);
            Say("wrote " + Path.GetFileName(file) + " (" + bitmap.PixelWidth + "x" + bitmap.PixelHeight + ")");
        }
        catch (Exception problem)
        {
            Say("no picture " + Path.GetFileName(file) + " of " + name + ": " + problem.Message);
        }
    }

    // What a screen reader is given, as the system's own tool reads it from outside the app (when the tool is on the machine).
    private async Task InspectAsync(string file)
    {
        try
        {
            System.Diagnostics.ProcessStartInfo info = new("winapp", "ui inspect -a " + Environment.ProcessId.ToString(System.Globalization.CultureInfo.InvariantCulture) + " --depth 30")
            {
                RedirectStandardOutput = true,
                UseShellExecute = false,
                CreateNoWindow = true,
            };
            using System.Diagnostics.Process? tool = System.Diagnostics.Process.Start(info);
            if (tool is null)
            {
                return;
            }

            string text = await tool.StandardOutput.ReadToEndAsync();
            await tool.WaitForExitAsync();
            File.WriteAllText(file, System.Text.RegularExpressions.Regex.Replace(text, "\u001b\\[[0-9;]*m", string.Empty));
        }
        catch (Exception problem)
        {
            Say("no accessibility tree: " + problem.Message);
        }
    }

    private void Say(string line) => log.Add(DateTime.Now.ToString("HH:mm:ss.fff", System.Globalization.CultureInfo.InvariantCulture) + " " + line);
}
