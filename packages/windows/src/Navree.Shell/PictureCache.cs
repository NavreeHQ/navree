using Microsoft.UI.Xaml.Media.Imaging;
using Windows.Graphics.Imaging;
using Windows.Storage.Streams;

namespace Navree.Shell;

/// <summary>
/// The pictures a page has drawn, kept for the next time the page is drawn: a page is drawn again after every answer, and a picture that was
/// fetched and decoded a moment ago is not fetched and decoded again, nor does the place it stood in show its placeholder again. A picture is
/// decoded no larger than the largest a window shows, and the latest few are kept.
/// </summary>
internal static class PictureCache
{
    private const int Most = 48;
    private const int LargestSide = 1600;
    private const int LargestBytes = 16 * 1024 * 1024;

    private static readonly Dictionary<string, Task<BitmapImage?>> Held = [];
    private static readonly Queue<string> Order = [];

    /// <summary>The picture at an address: the one held, or the one fetched now.</summary>
    /// <param name="src">The address.</param>
    /// <param name="fetch">Fetches the bytes.</param>
    /// <returns>The picture, or none when it could not be had.</returns>
    internal static Task<BitmapImage?> GetAsync(string src, Func<string, Task<byte[]?>> fetch)
    {
        if (Held.TryGetValue(src, out Task<BitmapImage?>? held))
        {
            return held;
        }

        Task<BitmapImage?> loading = LoadAsync(src, fetch);
        if (loading.IsCompleted && loading.Result is null)
        {
            // Answered at once and with nothing: not kept.
            return loading;
        }

        Held[src] = loading;
        Order.Enqueue(src);
        while (Held.Count > Most && Order.TryDequeue(out string? oldest))
        {
            _ = Held.Remove(oldest);
        }

        return loading;
    }

    /// <summary>Lets go of every picture held (the person signed out: what was theirs is not kept).</summary>
    internal static void Clear()
    {
        Held.Clear();
        Order.Clear();
    }

    private static async Task<BitmapImage?> LoadAsync(string src, Func<string, Task<byte[]?>> fetch)
    {
        try
        {
            byte[]? bytes = await fetch(src);
            if (bytes is null || bytes.Length > LargestBytes)
            {
                Forget(src);
                return null;
            }

            using InMemoryRandomAccessStream stream = new();
            using (DataWriter writer = new(stream))
            {
                writer.WriteBytes(bytes);
                await writer.StoreAsync();
                writer.DetachStream();
            }

            stream.Seek(0);
            BitmapImage bitmap = new();
            try
            {
                // A picture larger than a window shows is decoded at the size it is shown at, not at the size it was taken at.
                BitmapDecoder decoder = await BitmapDecoder.CreateAsync(stream);
                if (decoder.PixelWidth >= decoder.PixelHeight && decoder.PixelWidth > LargestSide)
                {
                    bitmap.DecodePixelWidth = LargestSide;
                }
                else if (decoder.PixelHeight > LargestSide)
                {
                    bitmap.DecodePixelHeight = LargestSide;
                }
            }
            catch (Exception)
            {
                // Not a picture the decoder knows the size of: the bitmap tries it as it is.
            }

            stream.Seek(0);
            await bitmap.SetSourceAsync(stream);
            return bitmap;
        }
        catch (Exception)
        {
            Forget(src);
            return null;
        }
    }

    // A picture that could not be had is asked for again the next time, not remembered as missing.
    private static void Forget(string src) => _ = Held.Remove(src);
}
