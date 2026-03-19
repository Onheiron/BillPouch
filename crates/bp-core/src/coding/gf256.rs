//! Arithmetic in GF(2⁸) — the Galois field with 256 elements.
//!
//! Uses the standard AES irreducible polynomial:
//!   **p(x) = x⁸ + x⁴ + x³ + x + 1** (0x11b)
//!
//! ## Operations
//! - [`add`] — addition = XOR (free)
//! - [`mul`] — multiplication via Russian-peasant (8 iterations, no lookup tables)
//! - [`inv`] — multiplicative inverse via Fermat: a⁻¹ = a²⁵⁴
//! - [`div`] — division = mul(a, inv(b))
//!
//! Element 0 is the additive identity. Element 1 is the multiplicative identity.
//! Calling [`inv`] or [`div`] with b=0 panics (division by zero).

/// Lower 8 bits of the AES reduction polynomial (x⁸ + x⁴ + x³ + x + 1).
const POLY: u8 = 0x1b;

/// Addition in GF(2⁸) — identical to XOR.
#[inline(always)]
pub fn add(a: u8, b: u8) -> u8 {
    a ^ b
}

/// Multiplication in GF(2⁸) using the Russian-peasant algorithm.
/// Runs in exactly 8 iterations regardless of input values.
#[inline]
pub fn mul(mut a: u8, mut b: u8) -> u8 {
    let mut result = 0u8;
    for _ in 0..8 {
        if b & 1 != 0 {
            result ^= a;
        }
        let high = a & 0x80;
        a <<= 1;
        if high != 0 {
            a ^= POLY;
        }
        b >>= 1;
    }
    result
}

/// Multiplicative inverse of `a` via Fermat's little theorem: a⁻¹ = a^254.
///
/// # Panics
/// Panics if `a == 0`.
#[inline]
pub fn inv(a: u8) -> u8 {
    assert!(a != 0, "gf256::inv: inverse of 0 is undefined");
    // Compute a^254 using repeated squaring.
    // 254 = 0b11111110 → exponents: 2, 4, 8, 16, 32, 64, 128 → product = 2^1*...*2^7 = 2^(1+2+...+7)
    // But we need exactly a^254:
    //   a^2 * a^4 * a^8 * a^16 * a^32 * a^64 * a^128
    //   = a^(2+4+8+16+32+64+128) = a^254 ✓
    let a2 = mul(a, a);
    let a4 = mul(a2, a2);
    let a8 = mul(a4, a4);
    let a16 = mul(a8, a8);
    let a32 = mul(a16, a16);
    let a64 = mul(a32, a32);
    let a128 = mul(a64, a64);
    mul(mul(mul(mul(mul(mul(a2, a4), a8), a16), a32), a64), a128)
}

/// Division in GF(2⁸): `a / b = a · b⁻¹`.
///
/// # Panics
/// Panics if `b == 0`.
#[inline]
pub fn div(a: u8, b: u8) -> u8 {
    if a == 0 {
        return 0;
    }
    mul(a, inv(b))
}

/// `acc[i] ^= coeff * symbol[i]` over GF(2⁸) — the inner loop of RLNC encoding.
///
/// # Panics
/// Panics if `acc.len() != symbol.len()`.
pub fn mul_acc(acc: &mut [u8], symbol: &[u8], coeff: u8) {
    assert_eq!(acc.len(), symbol.len(), "gf256::mul_acc: length mismatch");
    if coeff == 0 {
        return;
    }
    if coeff == 1 {
        for (a, &s) in acc.iter_mut().zip(symbol.iter()) {
            *a ^= s;
        }
        return;
    }
    for (a, &s) in acc.iter_mut().zip(symbol.iter()) {
        *a ^= mul(coeff, s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_is_xor() {
        assert_eq!(add(0x53, 0xca), 0x53 ^ 0xca);
    }

    #[test]
    fn mul_known_value() {
        // 0x53 * 0xca == 0x01 in GF(2^8) with poly 0x11b — verified by hand
        assert_eq!(mul(0x53, 0xca), 0x01);
    }

    #[test]
    fn mul_by_zero_and_one() {
        assert_eq!(mul(0xff, 0x00), 0x00);
        assert_eq!(mul(0x00, 0x53), 0x00);
        assert_eq!(mul(0xab, 0x01), 0xab);
        assert_eq!(mul(0x01, 0xcd), 0xcd);
    }

    #[test]
    fn mul_commutative() {
        for a in [0x00u8, 0x01, 0x53, 0xca, 0xff] {
            for b in [0x00u8, 0x01, 0x53, 0xca, 0xff] {
                assert_eq!(mul(a, b), mul(b, a));
            }
        }
    }

    #[test]
    fn inv_times_self_is_one() {
        for a in 1u8..=255 {
            assert_eq!(mul(a, inv(a)), 1, "a*inv(a) != 1 for a={a:#x}");
        }
    }

    #[test]
    fn div_round_trip() {
        assert_eq!(mul(div(0x53, 0x2b), 0x2b), 0x53);
    }

    #[test]
    fn mul_acc_accumulates_correctly() {
        let mut acc = vec![0u8; 3];
        mul_acc(&mut acc, &[0x01, 0x02, 0x03], 0x02);
        assert_eq!(acc[0], mul(0x02, 0x01));
        assert_eq!(acc[1], mul(0x02, 0x02));
        assert_eq!(acc[2], mul(0x02, 0x03));
    }
}
