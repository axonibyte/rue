//! Small helpers the transcription needs to match the prototype's semantics.

/// First-occurrence deduplication preserving order: Haskell's `nub`. The
/// prototype relies on it wherever a list of conflicts or steps is built, and
/// the goldens depend on the order it keeps.
pub fn nub<T: PartialEq + Clone>(items: &[T]) -> Vec<T> {
    let mut out: Vec<T> = Vec::with_capacity(items.len());
    for x in items {
        if !out.contains(x) {
            out.push(x.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::nub;

    #[test]
    fn nub_keeps_first_occurrences_in_order() {
        assert_eq!(nub(&[3, 1, 3, 2, 1]), vec![3, 1, 2]);
        assert_eq!(nub::<u8>(&[]), Vec::<u8>::new());
    }
}
