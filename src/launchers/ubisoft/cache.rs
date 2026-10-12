//! Bounded read-only decoding of LevelDB table values and raw Snappy blocks.
//! Formats: google/leveldb doc/table_format.md; google/snappy format_description.txt.
const LIMIT: usize = 16 * 1024 * 1024;

fn varint(bytes: &[u8], pos: &mut usize) -> Option<usize> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *bytes.get(*pos)?;
        *pos += 1;
        if shift == 63 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 127) << shift;
        if byte & 128 == 0 {
            return usize::try_from(value).ok();
        }
    }
    None
}

fn little(bytes: &[u8], pos: &mut usize, count: usize) -> Option<usize> {
    let end = pos.checked_add(count)?;
    let mut value = 0u64;
    for (i, byte) in bytes.get(*pos..end)?.iter().enumerate() {
        value |= u64::from(*byte) << (8 * i);
    }
    *pos = end;
    usize::try_from(value).ok()
}

fn snappy(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut pos = 0;
    let size = varint(bytes, &mut pos)?;
    if size > LIMIT {
        return None;
    }
    let mut out = Vec::with_capacity(size);
    while pos < bytes.len() && out.len() < size {
        let tag = *bytes.get(pos)?;
        pos += 1;
        let kind = tag & 3;
        if kind == 0 {
            let n = usize::from(tag >> 2);
            let len = if n < 60 {
                n + 1
            } else {
                little(bytes, &mut pos, n - 59)?.checked_add(1)?
            };
            if len > size - out.len() {
                return None;
            }
            let end = pos.checked_add(len)?;
            out.extend_from_slice(bytes.get(pos..end)?);
            pos = end;
        } else {
            let (len, offset) = match kind {
                1 => (
                    4 + usize::from((tag >> 2) & 7),
                    (usize::from(tag & 224) << 3) | little(bytes, &mut pos, 1)?,
                ),
                2 => (1 + usize::from(tag >> 2), little(bytes, &mut pos, 2)?),
                3 => (1 + usize::from(tag >> 2), little(bytes, &mut pos, 4)?),
                _ => return None,
            };
            if offset == 0 || offset > out.len() || len > size - out.len() {
                return None;
            }
            for _ in 0..len {
                out.push(out[out.len() - offset]);
            }
        }
    }
    (out.len() == size && pos == bytes.len()).then_some(out)
}

fn block(bytes: &[u8], offset: usize, size: usize) -> Option<Vec<u8>> {
    if size > LIMIT {
        return None;
    }
    let end = offset.checked_add(size)?;
    let data = bytes.get(offset..end)?;
    // Require the full compression/checksum trailer to be present.
    let trailer = bytes.get(end..end.checked_add(5)?)?;
    match trailer[0] {
        0 => Some(data.to_vec()),
        1 => snappy(data),
        _ => None,
    }
}

fn values(block: &[u8]) -> Option<Vec<&[u8]>> {
    let end = block.len().checked_sub(4)?;
    let count = u32::from_le_bytes(block.get(end..)?.try_into().ok()?) as usize;
    let data_end = end.checked_sub(count.checked_mul(4)?)?;
    let mut pos = 0;
    let mut key_len = 0usize;
    let mut result = Vec::new();
    while pos < data_end {
        let shared = varint(&block[..data_end], &mut pos)?;
        let unshared = varint(&block[..data_end], &mut pos)?;
        let size = varint(&block[..data_end], &mut pos)?;
        if shared > key_len {
            return None;
        }
        key_len = shared.checked_add(unshared)?;
        pos = pos.checked_add(unshared)?;
        let end = pos.checked_add(size)?;
        if end > data_end {
            return None;
        }
        result.push(block.get(pos..end)?);
        pos = end;
        if result.len() > 16384 {
            return None;
        }
    }
    Some(result)
}

pub(super) fn table_values(bytes: &[u8]) -> Option<Vec<Vec<u8>>> {
    let footer = bytes.get(bytes.len().checked_sub(48)?..)?;
    if footer.get(40..)? != [0x57, 0xfb, 0x80, 0x8b, 0x24, 0x75, 0x47, 0xdb] {
        return None;
    }
    let mut pos = 0;
    varint(&footer[..40], &mut pos)?; // Metaindex handle.
    varint(&footer[..40], &mut pos)?;
    let offset = varint(&footer[..40], &mut pos)?;
    let size = varint(&footer[..40], &mut pos)?;
    let index = block(bytes, offset, size)?;
    let mut result = Vec::new();
    let mut total = 0usize;
    for handle in values(&index)? {
        let mut pos = 0;
        let offset = varint(handle, &mut pos)?;
        let size = varint(handle, &mut pos)?;
        let data = block(bytes, offset, size)?;
        total = total.checked_add(data.len())?;
        if total > 64 * 1024 * 1024 {
            return None;
        }
        for value in values(&data)? {
            result.push(value.to_vec());
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlapping_copies_and_malformed_compression_are_bounded() {
        assert_eq!(
            snappy(&[7, 8, b'x', b'a', b'b', 1, 2]),
            Some(b"xababab".to_vec())
        );
        assert_eq!(
            snappy(&[7, 8, b'x', b'a', b'b', 14, 2, 0]),
            Some(b"xababab".to_vec())
        );
        assert_eq!(
            snappy(&[7, 8, b'x', b'a', b'b', 15, 2, 0, 0, 0]),
            Some(b"xababab".to_vec())
        );
        for invalid in [&[4, 1, 0][..], &[4, 1, 9], &[2, 8, b'a'], &[255; 10]] {
            assert!(snappy(invalid).is_none());
        }
        for len in 0..48 {
            assert!(table_values(&vec![0; len]).is_none());
        }
    }
}
