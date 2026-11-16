using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.Graphics.Imaging;
using Windows.Storage.Streams;

namespace Navree.Demo;

/// <summary>Draws an element of the window to a PNG file, from inside the app.</summary>
internal static class WindowPicture
{
    /// <summary>Saves what <paramref name="root"/> shows, on an opaque ground, to <paramref name="file"/>.</summary>
    internal static async Task SaveAsync(Microsoft.UI.Xaml.Controls.Panel root, string file)
    {
        // Mica is drawn by the system behind the window, not by the element, so the
        // picture gets a plain ground of the same lightness in its place.
        root.Background = new SolidColorBrush(
            root.ActualTheme == ElementTheme.Dark
                ? ColorHelper.FromArgb(0xFF, 0x20, 0x20, 0x20)
                : ColorHelper.FromArgb(0xFF, 0xF3, 0xF3, 0xF3));

        RenderTargetBitmap bitmap = new();
        await bitmap.RenderAsync(root);
        IBuffer pixels = await bitmap.GetPixelsAsync();

        byte[] raw = new byte[pixels.Length];
        using (DataReader reader = DataReader.FromBuffer(pixels))
        {
            reader.ReadBytes(raw);
        }

        using InMemoryRandomAccessStream stream = new();
        BitmapEncoder encoder = await BitmapEncoder.CreateAsync(BitmapEncoder.PngEncoderId, stream);
        encoder.SetPixelData(
            BitmapPixelFormat.Bgra8,
            BitmapAlphaMode.Premultiplied,
            (uint)bitmap.PixelWidth,
            (uint)bitmap.PixelHeight,
            96,
            96,
            raw);
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
