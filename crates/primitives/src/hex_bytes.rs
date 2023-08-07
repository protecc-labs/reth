use crate::serde_helper::hex_bytes;
use bytes::Buf;
use reth_codecs::Compact;
use reth_rlp::{Decodable, DecodeError, Encodable};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Borrow,
    clone::Clone,
    fmt::{Debug, Display, Formatter, LowerHex, Result as FmtResult},
    ops::Deref,
    str::FromStr,
};
use thiserror::Error;

/// Wrapper type around Bytes to deserialize/serialize "0x" prefixed ethereum hex strings
#[derive(Clone, Default, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct Bytes(#[serde(with = "hex_bytes")] pub bytes::Bytes);

fn bytes_to_hex(b: &Bytes) -> String {
    hex::encode(b.0.as_ref())
}

impl Debug for Bytes {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "Bytes(0x{})", bytes_to_hex(self))
    }
}

impl Display for Bytes {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "0x{}", bytes_to_hex(self))
    }
}

impl LowerHex for Bytes {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "0x{}", bytes_to_hex(self))
    }
}

impl Bytes {
    /// Return bytes as [`Vec::<u8>`]
    pub fn to_vec(&self) -> Vec<u8> {
        self.as_ref().to_vec()
    }
}

impl Deref for Bytes {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        self.as_ref()
    }
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Borrow<[u8]> for Bytes {
    fn borrow(&self) -> &[u8] {
        self.as_ref()
    }
}

impl IntoIterator for Bytes {
    type Item = u8;
    type IntoIter = bytes::buf::IntoIter<bytes::Bytes>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a Bytes {
    type Item = &'a u8;
    type IntoIter = core::slice::Iter<'a, u8>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_ref().iter()
    }
}

impl From<&[u8]> for Bytes {
    fn from(src: &[u8]) -> Self {
        Self(bytes::Bytes::copy_from_slice(src))
    }
}

impl From<bytes::Bytes> for Bytes {
    fn from(src: bytes::Bytes) -> Self {
        Self(src)
    }
}

impl From<Bytes> for bytes::Bytes {
    fn from(src: Bytes) -> Self {
        src.0
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(src: Vec<u8>) -> Self {
        Self(src.into())
    }
}

impl<const N: usize> From<[u8; N]> for Bytes {
    fn from(src: [u8; N]) -> Self {
        src.to_vec().into()
    }
}

impl<'a, const N: usize> From<&'a [u8; N]> for Bytes {
    fn from(src: &'a [u8; N]) -> Self {
        src.to_vec().into()
    }
}

impl PartialEq<[u8]> for Bytes {
    fn eq(&self, other: &[u8]) -> bool {
        self.as_ref() == other
    }
}

impl PartialEq<Bytes> for [u8] {
    fn eq(&self, other: &Bytes) -> bool {
        *other == *self
    }
}

impl PartialEq<Vec<u8>> for Bytes {
    fn eq(&self, other: &Vec<u8>) -> bool {
        self.as_ref() == &other[..]
    }
}

impl PartialEq<Bytes> for Vec<u8> {
    fn eq(&self, other: &Bytes) -> bool {
        *other == *self
    }
}

impl PartialEq<bytes::Bytes> for Bytes {
    fn eq(&self, other: &bytes::Bytes) -> bool {
        other == self.as_ref()
    }
}

impl Encodable for Bytes {
    fn encode(&self, out: &mut dyn bytes::BufMut) {
        self.0.encode(out)
    }
    fn length(&self) -> usize {
        self.0.length()
    }
}

impl Decodable for Bytes {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self(bytes::Bytes::decode(buf)?))
    }
}

#[derive(Debug, Clone, Error)]
#[error("Failed to parse bytes: {0}")]
pub struct ParseBytesError(String);

impl FromStr for Bytes {
    type Err = ParseBytesError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Some(value) = value.strip_prefix("0x") {
            hex::decode(value)
        } else {
            hex::decode(value)
        }
        .map(Into::into)
        .map_err(|e| ParseBytesError(format!("Invalid hex: {e}")))
    }
}

impl Compact for Bytes {
    fn to_compact<B>(self, buf: &mut B) -> usize
    where
        B: bytes::BufMut + AsMut<[u8]>,
    {
        let len = self.len();
        buf.put(self.0);
        len
    }
    fn from_compact(mut buf: &[u8], len: usize) -> (Self, &[u8]) {
        (buf.copy_to_bytes(len).into(), buf)
    }
}

#[cfg(any(test, feature = "arbitrary"))]
use proptest::strategy::Strategy;
#[cfg(any(test, feature = "arbitrary"))]
impl proptest::prelude::Arbitrary for Bytes {
    type Parameters = proptest::arbitrary::ParamsFor<u8>;
    fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
        proptest::collection::vec(proptest::arbitrary::any_with::<u8>(args), 0..80)
            .prop_map(move |vec| bytes::Bytes::from(vec).into())
            .boxed()
    }

    type Strategy = proptest::prelude::BoxedStrategy<Bytes>;
}

#[cfg(any(test, feature = "arbitrary"))]
impl<'a> arbitrary::Arbitrary<'a> for Bytes {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let size = u.int_in_range(0..=80)?;
        Ok(Self(bytes::Bytes::copy_from_slice(u.bytes(size)?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_bytes() {
        let b = bytes::Bytes::from("0123456789abcdef");
        let wrapped_b = Bytes::from(b.clone());
        let expected = Bytes(b);

        assert_eq!(wrapped_b, expected);
    }

    #[test]
    fn test_from_slice() {
        let arr = [1, 35, 69, 103, 137, 171, 205, 239];
        let b = Bytes::from(&arr);
        let expected = Bytes(bytes::Bytes::from(arr.to_vec()));

        assert_eq!(b, expected);
    }

    #[test]
    fn hex_formatting() {
        let b = Bytes::from(vec![1, 35, 69, 103, 137, 171, 205, 239]);
        let expected = String::from("0x0123456789abcdef");
        assert_eq!(format!("{b:x}"), expected);
        assert_eq!(format!("{b}"), expected);
    }

    #[test]
    fn test_from_str() {
        let b = Bytes::from_str("0x1213");
        assert!(b.is_ok());
        let b = b.unwrap();
        assert_eq!(b.as_ref(), hex::decode("1213").unwrap());

        let b = Bytes::from_str("1213");
        let b = b.unwrap();
        assert_eq!(b.as_ref(), hex::decode("1213").unwrap());
    }

    #[test]
    fn test_debug_formatting() {
        let b = Bytes::from(vec![1, 35, 69, 103, 137, 171, 205, 239]);
        assert_eq!(format!("{b:?}"), "Bytes(0x0123456789abcdef)");
        assert_eq!(format!("{b:#?}"), "Bytes(0x0123456789abcdef)");
    }

    #[test]
    fn test_to_vec() {
        let vec = vec![1, 35, 69, 103, 137, 171, 205, 239];
        let b = Bytes::from(vec.clone());

        assert_eq!(b.to_vec(), vec);
    }

    #[test]
    fn test_encodable_length_lt_56() {
        let b = Bytes::from(vec![1, 35, 69, 103, 137, 171, 205, 239]);
        // since the payload length is less than 56, this should give the length
        // of the array + 1 = 9
        assert_eq!(b.length(), 9);
    }

    #[test]
    fn test_encodable_length_gt_56() {
        let b = Bytes::from(vec![255; 57]);
        // since the payload length is greater than 56, this should give the length
        // of the array + (1 + 8 - payload_length.leading_zeros() as usize / 8) = 59
        assert_eq!(b.length(), 59);
    }

    #[test]
    fn test_encodable_encode() {
        let b = Bytes::from(vec![1, 35, 69, 103, 137, 171, 205, 239]);
        let mut buf = Vec::new();
        b.encode(&mut buf);
        let expected: Vec<u8> = vec![136, 1, 35, 69, 103, 137, 171, 205, 239];
        assert_eq!(buf, expected);
    }

    #[test]
    fn test_decodable_decode() {
        let buf: Vec<u8> = vec![136, 1, 35, 69, 103, 137, 171, 205, 239];
        let b = Bytes::decode(&mut &buf[..]).unwrap();
        let expected = Bytes::from(vec![1, 35, 69, 103, 137, 171, 205, 239]);
        assert_eq!(b, expected);
    }

    #[test]
    fn test_vec_partialeq() {
        let vec = vec![1, 35, 69, 103, 137, 171, 205, 239];
        let b = Bytes::from(vec.clone());
        assert_eq!(b, vec);
        assert_eq!(vec, b);

        let wrong_vec = vec![1, 3, 52, 137];
        assert_ne!(b, wrong_vec);
        assert_ne!(wrong_vec, b);
    }

    #[test]
    fn test_slice_partialeq() {
        let vec = vec![1, 35, 69, 103, 137, 171, 205, 239];
        let b = Bytes::from(vec.clone());
        assert_eq!(b, vec[..]);
        assert_eq!(vec[..], b);

        let wrong_vec = [1, 3, 52, 137];
        assert_ne!(b, wrong_vec[..]);
        assert_ne!(wrong_vec[..], b);
    }

    #[test]
    fn test_bytes_partialeq() {
        let b = bytes::Bytes::from("0123456789abcdef");
        let wrapped_b = Bytes::from(b.clone());
        assert_eq!(wrapped_b, b);

        let wrong_b = bytes::Bytes::from("0123absd");
        assert_ne!(wrong_b, b);
    }

    #[test]
    fn arbitrary() {
        proptest::proptest!(|(bytes: Bytes)| {
            let mut buf = vec![];
            bytes.clone().to_compact(&mut buf);

            let (decoded, remaining_buf) = Bytes::from_compact(&buf, buf.len());

            assert!(bytes == decoded);
            assert!(remaining_buf.is_empty());
        });
    }
}
