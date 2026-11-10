//! Image conversions: sizes and fits, formats, EXIF orientation and
//! stripping, SVG rasterizing, responsive variants and limits.

use image::ImageFormat;
use navree_media::images::{self, ImageOptions};
use navree_media::{Conversion, Fit, MediaError, OutputFormat};

use crate::support::{image_bytes, jpeg_with_exif, png_with_text, svg_logo, transparent_png};

fn options() -> ImageOptions {
    ImageOptions::default()
}

fn dims(bytes: &[u8]) -> (u32, u32) {
    let img = image::load_from_memory(bytes).expect("decodable output");
    (img.width(), img.height())
}

#[test]
fn probe_reads_dimensions_and_type_without_decoding() {
    let info = images::probe(&image_bytes(640, 480, ImageFormat::Png)).unwrap();
    assert_eq!((info.width, info.height), (640, 480));
    assert_eq!(info.mime_type, "image/png");

    let info = images::probe(&svg_logo(120, 40)).unwrap();
    assert_eq!((info.width, info.height), (120, 40));
    assert_eq!(info.mime_type, "image/svg+xml");
}

#[test]
fn contain_fits_the_box_keeps_the_ratio_and_never_upscales() {
    let source = image_bytes(800, 400, ImageFormat::Png);
    let thumb = Conversion::new("thumb").fit(Fit::Contain, 200, 200);
    let out = images::convert(&source, "image/png", &thumb, &options()).unwrap();
    assert_eq!((out.width, out.height), (200, 100));
    assert_eq!(dims(&out.bytes), (200, 100));

    let big = Conversion::new("big").fit(Fit::Contain, 2000, 2000);
    let out = images::convert(&source, "image/png", &big, &options()).unwrap();
    assert_eq!(
        (out.width, out.height),
        (800, 400),
        "no upscaling by default"
    );

    let upscaled = Conversion::new("big")
        .fit(Fit::Contain, 1600, 1600)
        .upscale();
    let out = images::convert(&source, "image/png", &upscaled, &options()).unwrap();
    assert_eq!((out.width, out.height), (1600, 800));
}

#[test]
fn width_or_height_alone_keeps_the_ratio() {
    let source = image_bytes(800, 400, ImageFormat::Jpeg);
    let out = images::convert(
        &source,
        "image/jpeg",
        &Conversion::new("w").width(400),
        &options(),
    )
    .unwrap();
    assert_eq!((out.width, out.height), (400, 200));
    let out = images::convert(
        &source,
        "image/jpeg",
        &Conversion::new("h").height(100),
        &options(),
    )
    .unwrap();
    assert_eq!((out.width, out.height), (200, 100));
}

#[test]
fn cover_fills_the_box_exactly_and_fill_stretches() {
    let source = image_bytes(800, 400, ImageFormat::Png);
    let cover = Conversion::new("card").fit(Fit::Cover, 300, 300);
    let out = images::convert(&source, "image/png", &cover, &options()).unwrap();
    assert_eq!(dims(&out.bytes), (300, 300));

    let fill = Conversion::new("banner").fit(Fit::Fill, 100, 300);
    let out = images::convert(&source, "image/png", &fill, &options()).unwrap();
    assert_eq!(dims(&out.bytes), (100, 300));
}

#[test]
fn formats_are_kept_or_converted() {
    let jpeg = image_bytes(64, 64, ImageFormat::Jpeg);
    let out = images::convert(&jpeg, "image/jpeg", &Conversion::new("same"), &options()).unwrap();
    assert_eq!((out.mime_type, out.extension), ("image/jpeg", "jpg"));
    assert_eq!(image::guess_format(&out.bytes).unwrap(), ImageFormat::Jpeg);

    let webp = Conversion::new("webp").format(OutputFormat::WebP);
    let out = images::convert(&jpeg, "image/jpeg", &webp, &options()).unwrap();
    assert_eq!((out.mime_type, out.extension), ("image/webp", "webp"));
    assert_eq!(image::guess_format(&out.bytes).unwrap(), ImageFormat::WebP);

    let gif = image_bytes(32, 32, ImageFormat::Gif);
    let out = images::convert(&gif, "image/gif", &Conversion::new("still"), &options()).unwrap();
    assert_eq!(out.mime_type, "image/png", "other formats become PNG");
}

#[test]
fn jpeg_output_flattens_transparency_onto_white() {
    let png = transparent_png(20, 10);
    let jpeg = Conversion::new("jpeg")
        .format(OutputFormat::Jpeg)
        .quality(95);
    let out = images::convert(&png, "image/png", &jpeg, &options()).unwrap();
    let img = image::load_from_memory(&out.bytes).unwrap().to_rgb8();
    let left = img.get_pixel(2, 5);
    assert!(
        left.0.iter().all(|&c| c > 240),
        "transparent pixels became white: {left:?}"
    );
}

#[test]
fn greyscale_and_blur_are_applied() {
    let source = image_bytes(40, 40, ImageFormat::Png);
    let grey = Conversion::new("grey").greyscale().blur(1.5);
    let out = images::convert(&source, "image/png", &grey, &options()).unwrap();
    let img = image::load_from_memory(&out.bytes).unwrap().to_rgb8();
    for pixel in img.pixels() {
        assert!(pixel.0[0] == pixel.0[1] && pixel.0[1] == pixel.0[2]);
    }
}

#[test]
fn exif_orientation_is_applied_and_metadata_is_not_copied() {
    // Orientation 6: the camera was turned; the stored pixels are 80x40.
    let source = jpeg_with_exif(80, 40, 6);
    let out = images::convert(
        &source,
        "image/jpeg",
        &Conversion::new("upright"),
        &options(),
    )
    .unwrap();
    assert_eq!((out.width, out.height), (40, 80));
    assert!(!contains(&out.bytes, b"Exif"));
    assert!(!contains(&out.bytes, b"SERIAL-12345"));
}

#[test]
fn strip_metadata_removes_exif_from_a_jpeg_without_re_encoding() {
    let source = jpeg_with_exif(80, 40, 1);
    assert!(contains(&source, b"SERIAL-12345"));
    let stripped = images::strip_metadata(&source, "image/jpeg", &options())
        .unwrap()
        .expect("changed");
    assert!(!contains(&stripped, b"SERIAL-12345"));
    assert!(!contains(&stripped, b"Exif"));
    assert_eq!(dims(&stripped), (80, 40));
    // Same compressed image data: only the segment was removed.
    assert!(stripped.len() < source.len());
    assert_eq!(
        &stripped[stripped.len() - 200..],
        &source[source.len() - 200..]
    );
}

#[test]
fn strip_metadata_turns_a_rotated_jpeg_upright() {
    let source = jpeg_with_exif(80, 40, 6);
    let stripped = images::strip_metadata(&source, "image/jpeg", &options())
        .unwrap()
        .expect("changed");
    assert!(!contains(&stripped, b"SERIAL-12345"));
    assert_eq!(dims(&stripped), (40, 80));
}

#[test]
fn strip_metadata_removes_png_text_chunks_and_leaves_clean_files_alone() {
    let source = png_with_text(30, 20);
    assert!(contains(&source, b"Author"));
    let stripped = images::strip_metadata(&source, "image/png", &options())
        .unwrap()
        .expect("changed");
    assert!(!contains(&stripped, b"Author"));
    assert_eq!(dims(&stripped), (30, 20));

    let clean = image_bytes(30, 20, ImageFormat::Png);
    assert!(
        images::strip_metadata(&clean, "image/png", &options())
            .unwrap()
            .is_none()
    );
    // Formats without metadata support are returned unchanged.
    let gif = image_bytes(8, 8, ImageFormat::Gif);
    assert!(
        images::strip_metadata(&gif, "image/gif", &options())
            .unwrap()
            .is_none()
    );
}

#[test]
fn svg_is_rasterized_at_the_requested_size() {
    let svg = svg_logo(100, 50);
    let logo = Conversion::new("logo").width(400).format(OutputFormat::Png);
    let out = images::convert(&svg, "image/svg+xml", &logo, &options()).unwrap();
    assert_eq!((out.width, out.height), (400, 200), "vectors scale up");
    let img = image::load_from_memory(&out.bytes).unwrap().to_rgba8();
    assert_eq!(img.get_pixel(200, 100).0, [221, 0, 0, 255]);

    let png = images::rasterize_svg(&svg, Some(50), None, &options()).unwrap();
    assert_eq!(dims(&png), (50, 25));
}

#[test]
fn svg_never_loads_files_or_urls() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="10" height="10"><image width="10" height="10" xlink:href="file:///C:/Windows/win.ini"/><image width="10" height="10" xlink:href="https://example.com/a.png"/></svg>"#;
    let png = images::rasterize_svg(svg, None, None, &options()).unwrap();
    let img = image::load_from_memory(&png).unwrap().to_rgba8();
    assert!(img.pixels().all(|p| p.0[3] == 0), "nothing was drawn");
}

#[test]
fn responsive_variants_are_narrower_than_the_image() {
    let source = image_bytes(1000, 500, ImageFormat::Jpeg);
    let opts = ImageOptions {
        responsive_widths: vec![320, 640, 1280],
        ..ImageOptions::default()
    };
    let hero = Conversion::new("hero").width(900).responsive();
    let out = images::convert(&source, "image/jpeg", &hero, &opts).unwrap();
    assert_eq!(out.width, 900);
    let widths: Vec<(u32, u32)> = out.variants.iter().map(|v| (v.width, v.height)).collect();
    assert_eq!(widths, vec![(320, 160), (640, 320)]);
    for variant in &out.variants {
        assert_eq!(dims(&variant.bytes), (variant.width, variant.height));
        assert_eq!(variant.mime_type, "image/jpeg");
    }
}

#[test]
fn oversized_and_broken_images_are_refused() {
    let opts = ImageOptions {
        max_pixels: 10_000,
        ..ImageOptions::default()
    };
    let big = image_bytes(200, 100, ImageFormat::Png);
    let err = images::convert(&big, "image/png", &Conversion::new("x"), &opts).unwrap_err();
    assert!(matches!(err, MediaError::Image { .. }), "{err:?}");
    assert!(err.to_string().contains("20000 pixels"), "{err}");

    let svg = svg_logo(100, 50);
    let err = images::convert(
        &svg,
        "image/svg+xml",
        &Conversion::new("x").width(1000),
        &opts,
    )
    .unwrap_err();
    assert!(matches!(err, MediaError::Image { .. }), "{err:?}");

    let err = images::convert(
        b"not an image",
        "image/png",
        &Conversion::new("x"),
        &options(),
    )
    .unwrap_err();
    assert!(matches!(err, MediaError::Image { .. }), "{err:?}");
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
