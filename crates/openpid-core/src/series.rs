//! A sampled signal with an explicit validity mask.

use serde::{Deserialize, Serialize};

/// `f64` samples plus a validity mask. Invalid samples are never represented
/// as NaN; consumers must check [`Series::is_valid`] or use [`Series::iter`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Series {
    values: Vec<f64>,
    /// Bit set to 1 when the sample is *invalid*. Empty when all samples are valid.
    invalid: Vec<u64>,
    invalid_count: usize,
}

impl Series {
    /// All samples valid.
    pub fn from_vec(values: Vec<f64>) -> Self {
        Series {
            values,
            invalid: Vec::new(),
            invalid_count: 0,
        }
    }

    /// Build from optional samples; `None` marks an invalid sample (stored as 0.0).
    pub fn from_options<I: IntoIterator<Item = Option<f64>>>(iter: I) -> Self {
        let mut s = Series::from_vec(Vec::new());
        for v in iter {
            if let Some(x) = v {
                s.values.push(x)
            } else {
                s.values.push(0.0);
                let i = s.values.len() - 1;
                s.set_invalid(i);
            }
        }
        s
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Raw sample storage; invalid positions hold 0.0.
    pub fn values(&self) -> &[f64] {
        &self.values
    }

    pub fn is_valid(&self, i: usize) -> bool {
        if i >= self.values.len() {
            return false;
        }
        match self.invalid.get(i / 64) {
            Some(word) => (word >> (i % 64)) & 1 == 0,
            None => true,
        }
    }

    pub fn get(&self, i: usize) -> Option<f64> {
        if self.is_valid(i) {
            Some(self.values[i])
        } else {
            None
        }
    }

    pub fn set_invalid(&mut self, i: usize) {
        assert!(i < self.values.len(), "index out of range");
        if self.invalid.is_empty() {
            self.invalid = vec![0u64; self.values.len().div_ceil(64)];
        }
        let word = &mut self.invalid[i / 64];
        let bit = 1u64 << (i % 64);
        if *word & bit == 0 {
            *word |= bit;
            self.invalid_count += 1;
            self.values[i] = 0.0;
        }
    }

    pub fn invalid_count(&self) -> usize {
        self.invalid_count
    }

    pub fn valid_count(&self) -> usize {
        self.values.len() - self.invalid_count
    }

    pub fn all_valid(&self) -> bool {
        self.invalid_count == 0
    }

    /// Iterate over samples, `None` for invalid ones.
    pub fn iter(&self) -> impl Iterator<Item = Option<f64>> + '_ {
        (0..self.values.len()).map(move |i| self.get(i))
    }

    /// Apply `f` to every valid sample, keeping the mask.
    pub fn map(&self, mut f: impl FnMut(f64) -> f64) -> Series {
        let mut out = self.clone();
        for (i, v) in out.values.iter_mut().enumerate() {
            if self.is_valid(i) {
                *v = f(*v);
            }
        }
        out
    }

    /// Element-wise combination of two equally long series; a sample is valid
    /// only when both inputs are valid.
    pub fn zip_with(&self, other: &Series, mut f: impl FnMut(f64, f64) -> f64) -> Series {
        assert_eq!(self.len(), other.len(), "series length mismatch");
        Series::from_options((0..self.len()).map(|i| match (self.get(i), other.get(i)) {
            (Some(a), Some(b)) => Some(f(a, b)),
            _ => None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_roundtrip() {
        let mut s = Series::from_vec((0..130).map(f64::from).collect());
        assert!(s.all_valid());
        s.set_invalid(0);
        s.set_invalid(64);
        s.set_invalid(129);
        s.set_invalid(129);
        assert_eq!(s.invalid_count(), 3);
        assert_eq!(s.valid_count(), 127);
        assert_eq!(s.get(0), None);
        assert_eq!(s.get(1), Some(1.0));
        assert_eq!(s.get(64), None);
        assert_eq!(s.get(129), None);
        assert_eq!(s.get(130), None);
        let m = s.map(|x| x * 2.0);
        assert_eq!(m.get(2), Some(4.0));
        assert_eq!(m.get(64), None);
    }

    #[test]
    fn zip() {
        let a = Series::from_options([Some(1.0), None, Some(3.0)]);
        let b = Series::from_options([Some(1.0), Some(2.0), None]);
        let c = a.zip_with(&b, |x, y| x + y);
        assert_eq!(c.iter().collect::<Vec<_>>(), vec![Some(2.0), None, None]);
    }
}
