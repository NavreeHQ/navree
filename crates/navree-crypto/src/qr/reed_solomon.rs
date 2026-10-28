//! Reed-Solomon error correction over GF(2^8) (ISO/IEC 18004 §7.5.2):
//! the field polynomial is x^8 + x^4 + x^3 + x^2 + 1 (0x11D), and the
//! generator polynomial of degree `d` has the roots 2^0 to 2^(d-1).

/// The product of `x` and `y` in GF(2^8).
fn multiply(x: u8, y: u8) -> u8 {
    let mut product: u32 = 0;
    for bit in (0..8).rev() {
        product = (product << 1) ^ ((product >> 7) * 0x11D);
        product ^= u32::from((y >> bit) & 1) * u32::from(x);
    }
    // The loop keeps `product` below 256.
    product as u8
}

/// The coefficients of the generator polynomial of `degree`, highest power
/// first, without the leading 1.
pub(super) fn divisor(degree: usize) -> Vec<u8> {
    let mut result = vec![0_u8; degree];
    result[degree - 1] = 1;
    let mut root = 1_u8;
    for _ in 0..degree {
        for j in 0..degree {
            result[j] = multiply(result[j], root);
            if j + 1 < degree {
                result[j] ^= result[j + 1];
            }
        }
        root = multiply(root, 2);
    }
    result
}

/// The error-correction codewords of `data`: the remainder of
/// `data * x^degree` divided by the generator polynomial.
pub(super) fn remainder(data: &[u8], divisor: &[u8]) -> Vec<u8> {
    let mut result = vec![0_u8; divisor.len()];
    for byte in data {
        let factor = byte ^ result.remove(0);
        result.push(0);
        for (slot, coefficient) in result.iter_mut().zip(divisor) {
            *slot ^= multiply(*coefficient, factor);
        }
    }
    result
}
