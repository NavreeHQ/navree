//! Stream filters: Flate and LZW (with PNG and TIFF predictors), ASCIIHex,
//! ASCII85 and RunLength. Image codecs (DCT, JPX, JBIG2, CCITT) are left
//! encoded.
//!
//! Each filter's output is held to a limit as it is produced: a stream
//! that would inflate further is refused before it does. A stream has at most
//! [`MAX_FILTERS`] filters, and what each step reads and writes is counted for
//! the caller, whether the steps after it work or not.

use std::io::Read;

use crate::object::{Dictionary, Object};

/// Why a stream did not decode.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DecodeError {
    /// It would decode to more than the limit.
    Over,
    /// A filter this reader leaves encoded (an image codec) or does not
    /// know.
    Unsupported,
    /// The data is damaged.
    Invalid(&'static str),
}

/// Bytes read from an inflater at a time.
const CHUNK: usize = 64 * 1024;
/// Filters one stream may have (8). Real streams have one or two: an image
/// behind ASCII85 and Flate is about the most a writer does.
pub(crate) const MAX_FILTERS: usize = 8;
/// What setting up one filter costs besides what it reads and writes, in bytes
/// of work (1 KiB): an inflater takes memory of its own, however little it
/// reads.
const FILTER_COST: usize = 1024;

/// What a filter reads its data with.
#[derive(Clone, Copy, Debug)]
struct Params {
    predictor: i64,
    columns: usize,
    colors: usize,
    bits: usize,
    /// LZW's `EarlyChange`.
    early: bool,
}

impl Params {
    /// What `dict` holds, each value resolved.
    fn of<'a>(
        dict: Option<&'a Dictionary>,
        resolve: &dyn Fn(&'a Object) -> Option<&'a Object>,
    ) -> Params {
        let number =
            |key: &[u8]| -> Option<i64> { resolve(dict?.get(key)?).and_then(Object::as_i64) };
        let sample = |key: &[u8], default: i64| -> usize {
            usize::try_from(number(key).unwrap_or(default).max(1)).unwrap_or(usize::MAX)
        };
        Params {
            predictor: number(b"Predictor").unwrap_or(1),
            columns: sample(b"Columns", 1),
            colors: sample(b"Colors", 1),
            bits: sample(b"BitsPerComponent", 8),
            early: number(b"EarlyChange").is_none_or(|value| value != 0),
        }
    }
}

/// Decodes `data` through the filters of `dict`, each step held to `limit`
/// bytes. Data without filters is returned as it is, held to the limit
/// too. Values in `dict` that are references go through `resolve`.
///
/// `spent` grows by what each step reads and writes (a step that goes over
/// `limit` counts it all) and [`FILTER_COST`], before the next step can fail:
/// the caller charges it, whatever comes back.
pub(crate) fn decode<'a>(
    dict: &'a Dictionary,
    data: &'a [u8],
    limit: usize,
    resolve: &dyn Fn(&'a Object) -> Option<&'a Object>,
    spent: &mut usize,
) -> Result<Vec<u8>, DecodeError> {
    let value = |object: Option<&'a Object>| object.and_then(resolve);
    let filters: Vec<&[u8]> = match value(dict.get(b"Filter")) {
        Some(Object::Name(name)) => vec![name.as_slice()],
        Some(Object::Array(names)) if names.len() > MAX_FILTERS => {
            return Err(DecodeError::Invalid("more filters than a stream may have"));
        }
        Some(Object::Array(names)) => names
            .iter()
            .filter_map(|name| resolve(name)?.as_name())
            .collect(),
        _ => Vec::new(),
    };
    let params = value(dict.get(b"DecodeParms").or_else(|| dict.get(b"DP")));
    let mut output: Option<Vec<u8>> = None;
    for (index, filter) in filters.into_iter().enumerate() {
        let params = match params {
            Some(Object::Array(params)) => value(params.get(index)).and_then(Object::as_dict),
            Some(Object::Dictionary(params)) => Some(params),
            _ => None,
        };
        let params = Params::of(params, resolve);
        let input = output.as_deref().unwrap_or(data);
        *spent = spent.saturating_add(input.len());
        *spent = spent.saturating_add(FILTER_COST);
        // What an inflater wrote is written again only by a predictor.
        let inflates = matches!(filter, b"FlateDecode" | b"Fl" | b"LZWDecode" | b"LZW");
        let written_once = inflates && params.predictor <= 1;
        let decoded = match filter {
            b"FlateDecode" | b"Fl" => inflate(input, limit).and_then(|inflated| {
                *spent = spent.saturating_add(inflated.len());
                predict(inflated, params, limit)
            }),
            b"LZWDecode" | b"LZW" => lzw(input, params.early, limit).and_then(|inflated| {
                *spent = spent.saturating_add(inflated.len());
                predict(inflated, params, limit)
            }),
            b"ASCII85Decode" | b"A85" => ascii85(input, limit),
            b"ASCIIHexDecode" | b"AHx" => ascii_hex(input, limit),
            b"RunLengthDecode" | b"RL" => run_length(input, limit),
            // Decryption is done before the filters run.
            b"Crypt" => continue,
            _ => return Err(DecodeError::Unsupported),
        };
        let decoded = decoded.inspect_err(|error| {
            if *error == DecodeError::Over {
                *spent = spent.saturating_add(limit);
            }
        })?;
        if decoded.len() > limit {
            *spent = spent.saturating_add(limit);
            return Err(DecodeError::Over);
        }
        if !written_once {
            *spent = spent.saturating_add(decoded.len());
        }
        output = Some(decoded);
    }
    match output {
        Some(output) => Ok(output),
        None if data.len() > limit => Err(DecodeError::Over),
        None => Ok(data.to_vec()),
    }
}

/// Reads `reader` into `output` until it ends, fails, or passes `limit`.
fn read_capped(
    mut reader: impl Read,
    output: &mut Vec<u8>,
    limit: usize,
) -> Result<bool, DecodeError> {
    let mut chunk = vec![0u8; CHUNK];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => return Ok(true),
            Ok(read) => {
                if output.len().saturating_add(read) > limit {
                    return Err(DecodeError::Over);
                }
                output.extend_from_slice(chunk.get(..read).unwrap_or_default());
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            // What was read before the damage is kept.
            Err(_) => return Ok(false),
        }
    }
}

/// Flate (zlib). A stream whose zlib header or checksum is damaged is
/// read again as raw deflate; what decodes before damage is kept.
fn inflate(input: &[u8], limit: usize) -> Result<Vec<u8>, DecodeError> {
    let mut output = Vec::new();
    if input.is_empty() {
        return Ok(output);
    }
    let whole = read_capped(flate2::read::ZlibDecoder::new(input), &mut output, limit)?;
    if !whole
        && output.is_empty()
        && let Some(raw) = input.get(2..)
    {
        read_capped(flate2::read::DeflateDecoder::new(raw), &mut output, limit)?;
    }
    Ok(output)
}

/// LZW, with `EarlyChange` (1 unless the parameters say 0).
fn lzw(input: &[u8], early: bool, limit: usize) -> Result<Vec<u8>, DecodeError> {
    use weezl::{BitOrder, decode::Decoder};
    let mut decoder = if early {
        Decoder::with_tiff_size_switch(BitOrder::Msb, 8)
    } else {
        Decoder::new(BitOrder::Msb, 8)
    };
    let mut output = Vec::new();
    let mut chunk = vec![0u8; CHUNK];
    let mut input = input;
    loop {
        let result = decoder.decode_bytes(input, &mut chunk);
        let written = chunk.get(..result.consumed_out).unwrap_or_default();
        if output.len().saturating_add(written.len()) > limit {
            return Err(DecodeError::Over);
        }
        output.extend_from_slice(written);
        input = input.get(result.consumed_in..).unwrap_or_default();
        match result.status {
            Ok(weezl::LzwStatus::Ok) if result.consumed_in > 0 || result.consumed_out > 0 => {}
            // The end code, the end of the input, or damage: keep what was read.
            _ => return Ok(output),
        }
    }
}

/// ASCII85: groups of five characters for four bytes, `z` for four zeros,
/// `~>` at the end.
fn ascii85(input: &[u8], limit: usize) -> Result<Vec<u8>, DecodeError> {
    let input = input.strip_suffix(b"~>").unwrap_or(input);
    let mut output = Vec::new();
    let (mut value, mut count) = (0u32, 0usize);
    for &byte in input {
        if output.len() > limit {
            return Err(DecodeError::Over);
        }
        if byte == b'z' {
            if count != 0 {
                return Err(DecodeError::Invalid("z inside an ASCII85 group"));
            }
            output.extend_from_slice(&[0; 4]);
            continue;
        }
        if byte.is_ascii_whitespace() {
            continue;
        }
        if !(b'!'..=b'u').contains(&byte) {
            break;
        }
        value = value
            .checked_mul(85)
            .and_then(|value| value.checked_add(u32::from(byte - b'!')))
            .ok_or(DecodeError::Invalid("an ASCII85 group out of range"))?;
        count += 1;
        if count == 5 {
            output.extend_from_slice(&value.to_be_bytes());
            (value, count) = (0, 0);
        }
    }
    if count > 0 {
        for _ in count..5 {
            value = value
                .checked_mul(85)
                .and_then(|value| value.checked_add(84))
                .ok_or(DecodeError::Invalid("an ASCII85 group out of range"))?;
        }
        output.extend_from_slice(value.to_be_bytes().get(..count - 1).unwrap_or_default());
    }
    Ok(output)
}

/// ASCIIHex: two hex digits a byte, white space skipped, `>` at the end; an
/// odd last digit is the high half of its byte.
fn ascii_hex(input: &[u8], limit: usize) -> Result<Vec<u8>, DecodeError> {
    let mut output = Vec::new();
    let mut high: Option<u8> = None;
    for &byte in input {
        if output.len() > limit {
            return Err(DecodeError::Over);
        }
        if byte == b'>' {
            break;
        }
        if byte.is_ascii_whitespace() {
            continue;
        }
        let nibble = (byte as char)
            .to_digit(16)
            .ok_or(DecodeError::Invalid("a character that is not a hex digit"))?
            as u8;
        match high.take() {
            Some(high) => output.push(high << 4 | nibble),
            None => high = Some(nibble),
        }
    }
    if let Some(high) = high {
        output.push(high << 4);
    }
    Ok(output)
}

/// RunLength: a length byte `0..=127` copies the next `length + 1` bytes,
/// `129..=255` repeats the next byte `257 - length` times, `128` ends.
fn run_length(input: &[u8], limit: usize) -> Result<Vec<u8>, DecodeError> {
    let mut output = Vec::new();
    let mut at = 0;
    while let Some(&length) = input.get(at) {
        if output.len() > limit {
            return Err(DecodeError::Over);
        }
        at += 1;
        match length {
            128 => break,
            0..=127 => {
                let end = (at + usize::from(length) + 1).min(input.len());
                output.extend_from_slice(input.get(at..end).unwrap_or_default());
                at = end;
            }
            _ => {
                let Some(&byte) = input.get(at) else {
                    break;
                };
                output.resize(output.len() + (257 - usize::from(length)), byte);
                at += 1;
            }
        }
    }
    Ok(output)
}

/// Undoes the predictor of `params` (`Predictor` 2 for TIFF, 10 to 15 for
/// PNG) on `data`.
fn predict(data: Vec<u8>, params: Params, limit: usize) -> Result<Vec<u8>, DecodeError> {
    let Params {
        predictor,
        columns,
        colors,
        bits,
        ..
    } = params;
    if !matches!(predictor, 2 | 10..=15) {
        return Ok(data);
    }
    let row_bits = columns
        .checked_mul(colors)
        .and_then(|samples| samples.checked_mul(bits))
        .ok_or(DecodeError::Invalid("a predictor row too large"))?;
    match predictor {
        2 => tiff(data, columns, colors, bits),
        10..=15 => png(
            &data,
            (colors * bits).div_ceil(8),
            row_bits.div_ceil(8),
            limit,
        ),
        _ => Ok(data),
    }
}

/// The PNG predictors: each row starts with its filter type.
fn png(data: &[u8], bpp: usize, row: usize, limit: usize) -> Result<Vec<u8>, DecodeError> {
    // A row longer than the data cannot be read whole: refused before any
    // memory is set aside for it.
    if row == 0 || row >= data.len() {
        return if data.is_empty() {
            Ok(Vec::new())
        } else {
            Err(DecodeError::Invalid("a predictor row longer than the data"))
        };
    }
    let bpp = bpp.clamp(1, row);
    let mut previous = vec![0u8; row];
    let mut output = Vec::with_capacity(data.len().min(limit));
    let mut at = 0;
    while at < data.len() {
        let filter = data.get(at).copied().unwrap_or(0);
        let mut current = data
            .get(at + 1..at + 1 + row)
            .ok_or(DecodeError::Invalid("a predictor row cut short"))?
            .to_vec();
        at += 1 + row;
        match filter {
            0 => {}
            1 => {
                for i in bpp..row {
                    current[i] = current[i].wrapping_add(current[i - bpp]);
                }
            }
            2 => {
                for (byte, above) in current.iter_mut().zip(&previous) {
                    *byte = byte.wrapping_add(*above);
                }
            }
            3 => {
                for i in 0..row {
                    let left = if i >= bpp {
                        u16::from(current[i - bpp])
                    } else {
                        0
                    };
                    current[i] =
                        current[i].wrapping_add(((left + u16::from(previous[i])) / 2) as u8);
                }
            }
            4 => {
                for i in 0..row {
                    let (left, upper_left) = if i >= bpp {
                        (current[i - bpp], previous[i - bpp])
                    } else {
                        (0, 0)
                    };
                    current[i] = current[i].wrapping_add(paeth(left, previous[i], upper_left));
                }
            }
            _ => return Err(DecodeError::Invalid("an unknown PNG predictor")),
        }
        output.extend_from_slice(&current);
        previous = current;
    }
    Ok(output)
}

fn paeth(left: u8, above: u8, upper_left: u8) -> u8 {
    let (a, b, c) = (i16::from(left), i16::from(above), i16::from(upper_left));
    let estimate = a + b - c;
    let (da, db, dc) = (
        (estimate - a).abs(),
        (estimate - b).abs(),
        (estimate - c).abs(),
    );
    if da <= db && da <= dc {
        left
    } else if db <= dc {
        above
    } else {
        upper_left
    }
}

/// The TIFF predictor: each sample is added to the one before it of the
/// same color, in rows of `columns` pixels.
fn tiff(
    mut data: Vec<u8>,
    columns: usize,
    colors: usize,
    bits: usize,
) -> Result<Vec<u8>, DecodeError> {
    match bits {
        8 => {
            let stride = columns.saturating_mul(colors);
            for row in data.chunks_mut(stride.max(1)) {
                for i in colors..row.len() {
                    row[i] = row[i].wrapping_add(row[i - colors]);
                }
            }
        }
        16 => {
            let stride = columns.saturating_mul(colors).saturating_mul(2);
            for row in data.chunks_mut(stride.max(1)) {
                for sample in colors..row.len() / 2 {
                    let before = u16::from_be_bytes([
                        row[(sample - colors) * 2],
                        row[(sample - colors) * 2 + 1],
                    ]);
                    let this = u16::from_be_bytes([row[sample * 2], row[sample * 2 + 1]]);
                    row[sample * 2..sample * 2 + 2]
                        .copy_from_slice(&this.wrapping_add(before).to_be_bytes());
                }
            }
        }
        1 | 2 | 4 => {
            let stride = columns
                .saturating_mul(colors)
                .saturating_mul(bits)
                .div_ceil(8);
            for row in data.chunks_mut(stride.max(1)) {
                tiff_packed(row, columns, colors, bits);
            }
        }
        _ => {
            return Err(DecodeError::Invalid(
                "a TIFF predictor of an unsupported depth",
            ));
        }
    }
    Ok(data)
}

/// The TIFF predictor on one row of samples smaller than a byte.
fn tiff_packed(row: &mut [u8], columns: usize, colors: usize, bits: usize) {
    let mask = (1u16 << bits) - 1;
    let samples = columns.saturating_mul(colors).min(row.len() * 8 / bits);
    let mut sums = vec![0u16; colors.min(samples).max(1)];
    let mut out = vec![0u8; row.len()];
    for sample in 0..samples {
        let bit = sample * bits;
        let byte = row.get(bit / 8).copied().unwrap_or(0);
        let value = (u16::from(byte) >> (8 - bit % 8 - bits)) & mask;
        let color = sample % colors;
        let Some(sum) = sums.get_mut(color) else {
            continue;
        };
        *sum = (*sum + value) & mask;
        if let Some(slot) = out.get_mut(bit / 8) {
            *slot |= (*sum as u8) << (8 - bit % 8 - bits);
        }
    }
    // The padding past the last sample is kept as it was.
    let used = samples * bits;
    for bit in used..row.len() * 8 {
        let (byte, shift) = (bit / 8, 7 - bit % 8);
        if let (Some(from), Some(to)) = (row.get(byte).copied(), out.get_mut(byte)) {
            *to |= from & (1 << shift);
        }
    }
    row.copy_from_slice(&out);
}

/// `data` compressed with Flate.
pub(crate) fn deflate(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    // Writing to a vector cannot fail.
    let _ = encoder.write_all(data);
    encoder.finish().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    /// `decode` of a dictionary with direct values.
    fn decoded(dict: &Dictionary, data: &[u8], limit: usize) -> Result<Vec<u8>, DecodeError> {
        let mut spent = 0;
        decode(dict, data, limit, &Some, &mut spent)
    }

    use super::*;
    use crate::dictionary;

    fn with(filter: &str) -> Dictionary {
        dictionary! { "Filter" => filter }
    }

    #[test]
    fn flate_round_trips_and_is_held_to_the_limit() {
        let data = b"BT /F1 12 Tf (Hello) Tj ET".repeat(100);
        let packed = deflate(&data);
        assert_eq!(
            decoded(&with("FlateDecode"), &packed, 1 << 20).unwrap(),
            data
        );
        assert_eq!(
            decoded(&with("FlateDecode"), &packed, data.len() - 1),
            Err(DecodeError::Over)
        );
        // A megabyte of zeros from a kilobyte, refused at 64 KiB.
        let bomb = deflate(&vec![0; 1 << 20]);
        assert!(bomb.len() < 2048);
        assert_eq!(
            decoded(&with("FlateDecode"), &bomb, 64 * 1024),
            Err(DecodeError::Over)
        );
    }

    #[test]
    fn damaged_flate_keeps_what_decoded() {
        let data = b"0123456789".repeat(1000);
        let mut packed = deflate(&data);
        // A broken checksum: everything decodes.
        let last = packed.len() - 1;
        packed[last] ^= 0xFF;
        assert_eq!(
            decoded(&with("FlateDecode"), &packed, 1 << 20).unwrap(),
            data
        );
        // A zlib header that is not one: read as raw deflate.
        packed[0] = 0;
        assert_eq!(
            decoded(&with("FlateDecode"), &packed, 1 << 20).unwrap(),
            data
        );
    }

    #[test]
    fn ascii_filters_and_run_length() {
        assert_eq!(
            decoded(&with("ASCIIHexDecode"), b"48 65 6c6C6f>", 100).unwrap(),
            b"Hello"
        );
        assert_eq!(
            decoded(&with("ASCIIHexDecode"), b"414", 100).unwrap(),
            b"A@"
        );
        assert!(matches!(
            decoded(&with("ASCIIHexDecode"), b"4X", 100),
            Err(DecodeError::Invalid(_))
        ));
        assert_eq!(
            decoded(&with("ASCII85Decode"), b"87cURD]i,\"Ebo80~>", 100).unwrap(),
            b"Hello World!"
        );
        assert_eq!(
            decoded(&with("ASCII85Decode"), b"z~>", 100).unwrap(),
            [0; 4]
        );
        assert_eq!(
            decoded(&with("ASCII85Decode"), b"zzzz~>", 8),
            Err(DecodeError::Over)
        );
        assert_eq!(
            decoded(
                &with("RunLengthDecode"),
                &[2, b'a', b'b', b'c', 254, b'x', 128],
                100
            )
            .unwrap(),
            b"abcxxx"
        );
        assert_eq!(
            decoded(&with("RunLengthDecode"), &[129; 64], 100),
            Err(DecodeError::Over)
        );
    }

    #[test]
    fn filters_chain_in_order_and_images_stay_encoded() {
        let inner = deflate(b"chained");
        let hex: Vec<u8> = inner
            .iter()
            .flat_map(|b| format!("{b:02x}").into_bytes())
            .collect();
        let both = dictionary! {
            "Filter" => vec![Object::from("ASCIIHexDecode"), Object::from("FlateDecode")],
        };
        assert_eq!(decoded(&both, &hex, 100).unwrap(), b"chained");
        assert_eq!(
            decoded(&with("DCTDecode"), b"jpeg", 100),
            Err(DecodeError::Unsupported)
        );
        assert_eq!(
            decoded(&Dictionary::new(), b"plain", 100).unwrap(),
            b"plain"
        );
        assert_eq!(
            decoded(&Dictionary::new(), b"plain", 4),
            Err(DecodeError::Over)
        );
    }

    #[test]
    fn png_predictors_undo_each_filter() {
        // Two rows of three bytes: Sub, then Up.
        let rows = [1, 1, 1, 1, 2, 1, 1, 1];
        let packed = deflate(&rows);
        let dict = dictionary! {
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! { "Predictor" => 12, "Columns" => 3 },
        };
        assert_eq!(decoded(&dict, &packed, 100).unwrap(), [1, 2, 3, 2, 3, 4]);
        // Average and Paeth.
        let rows = [3, 2, 2, 2, 4, 1, 1, 1];
        let dict = dictionary! {
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! { "Predictor" => 15, "Columns" => 3 },
        };
        assert_eq!(
            decoded(&dict, &deflate(&rows), 100).unwrap(),
            [2, 3, 3, 3, 4, 5]
        );
        // A row longer than the data: refused, nothing set aside for it.
        let huge = dictionary! {
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! { "Predictor" => 12, "Columns" => i64::MAX / 16 },
        };
        assert!(matches!(
            decoded(&huge, &packed, 100),
            Err(DecodeError::Invalid(_))
        ));
    }

    #[test]
    fn the_tiff_predictor_adds_along_the_row() {
        let dict = dictionary! {
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! { "Predictor" => 2, "Columns" => 4 },
        };
        assert_eq!(
            decoded(&dict, &deflate(&[1, 1, 1, 1, 5, 0, 0, 0]), 100).unwrap(),
            [1, 2, 3, 4, 5, 5, 5, 5]
        );
        let packed = dictionary! {
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! { "Predictor" => 2, "Columns" => 8, "BitsPerComponent" => 1 },
        };
        // 1 0 0 0 0 0 0 0 sums to all ones.
        assert_eq!(
            decoded(&packed, &deflate(&[0b1000_0000]), 100).unwrap(),
            [0xFF]
        );
    }

    #[test]
    fn lzw_decodes_with_and_without_early_change() {
        // "-----A---B" from the PDF reference, with early change.
        let data = [0x80, 0x0B, 0x60, 0x50, 0x22, 0x0C, 0x0C, 0x85, 0x01];
        assert_eq!(
            decoded(&with("LZWDecode"), &data, 100).unwrap(),
            b"-----A---B"
        );
        assert_eq!(
            decoded(&with("LZWDecode"), &data, 5),
            Err(DecodeError::Over)
        );
    }

    #[test]
    fn what_each_step_reads_and_writes_is_counted_even_when_a_later_one_fails() {
        let data = vec![b' '; 4096];
        let packed = deflate(&data);
        let dict = dictionary! {
            "Filter" => vec![Object::name("FlateDecode"), Object::name("DCTDecode")],
        };
        let mut spent = 0;
        assert_eq!(
            decode(&dict, &packed, 1 << 20, &Some, &mut spent),
            Err(DecodeError::Unsupported)
        );
        assert!(spent >= data.len(), "{spent}");
        // One step past the limit counts the limit.
        let mut spent = 0;
        assert_eq!(
            decode(&with("FlateDecode"), &packed, 1024, &Some, &mut spent),
            Err(DecodeError::Over)
        );
        assert!(spent >= 1024, "{spent}");
    }

    #[test]
    fn a_stream_may_have_eight_filters_and_no_more() {
        let names = |count: usize| Object::Array(vec![Object::name("ASCIIHexDecode"); count]);
        let dict = |count: usize| dictionary! { "Filter" => names(count) };
        let mut data = b"x".to_vec();
        for _ in 0..MAX_FILTERS {
            data = data
                .iter()
                .flat_map(|byte| format!("{byte:02x}").into_bytes())
                .collect();
        }
        assert_eq!(decoded(&dict(MAX_FILTERS), &data, 1 << 20).unwrap(), b"x");
        assert!(matches!(
            decoded(&dict(MAX_FILTERS + 1), &data, 1 << 20),
            Err(DecodeError::Invalid(_))
        ));
    }
}
