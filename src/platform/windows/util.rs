/// NUL-terminated UTF-16 for Win32 APIs.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Copies `s` into a fixed-size UTF-16 buffer, truncating and NUL-terminating.
pub fn copy_wide(dst: &mut [u16], s: &str) {
    let mut n = 0;
    let capacity = dst.len().saturating_sub(1);
    for (slot, unit) in dst.iter_mut().zip(s.encode_utf16()).take(capacity) {
        *slot = unit;
        n += 1;
    }
    if let Some(end) = dst.get_mut(n) {
        *end = 0;
    }
}
