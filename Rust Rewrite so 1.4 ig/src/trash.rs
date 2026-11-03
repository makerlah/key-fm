use std::cmp::min;

pub fn parse_i_file(data: &[u8]) -> Option<(i64, Option<f64>, String)> {
    if data.len() < 24 {
        return None;
    }
    let version = i64::from_le_bytes(data[0..8].try_into().ok()?);
    let _size = i64::from_le_bytes(data[8..16].try_into().ok()?);
    let filetime = u64::from_le_bytes(data[16..24].try_into().ok()?);

    let raw = match version {
        1 => {
            if data.len() < 24 {
                return None;
            }
            let end = min(24 + 520, data.len());
            &data[24..end]
        }
        2 => {
            if data.len() < 28 {
                return None;
            }
            let n = u32::from_le_bytes(data[24..28].try_into().ok()?) as usize;
            let start = 28;
            let end = min(start + n * 2, data.len());
            &data[start..end]
        }
        _ => return None,
    };

    let utf16_units: Vec<u16> = raw
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();

    let decoded: String = char::decode_utf16(utf16_units.into_iter())
        .filter_map(|r| r.ok())
        .collect();

    let name = match decoded.find('\0') {
        Some(idx) => decoded[..idx].to_string(),
        None => decoded,
    };

    let ts = if filetime != 0 {
        Some((filetime as f64) / 10_000_000.0 - 11644473600.0)
    } else {
        None
    };

    Some((version, ts, name))
}
