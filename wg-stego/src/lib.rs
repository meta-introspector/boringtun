/// Stego encoding: zero-width characters encode each base64 char between cover words.
pub fn stego_encode(data: &[u8]) -> String {
    let b64 = base64::encode(data);
    let cover = [
        "the", "quick", "brown", "fox", "jumps", "over", "a", "lazy", "dog",
        "and", "then", "runs", "back", "to", "its", "den", "in", "the", "woods",
        "where", "it", "sleeps", "until", "dawn", "breaks", "again", "softly",
    ];
    let mut out = String::new();
    for (ci, ch) in b64.chars().enumerate() {
        let byte = ch as u8;
        out.push_str(cover[ci % cover.len()]);
        out.push('\u{200B}');
        for bit in (0..8).rev() {
            if (byte >> bit) & 1 == 1 {
                out.push('\u{200C}');
            } else {
                out.push('\u{200D}');
            }
        }
        out.push(' ');
    }
    out.push_str("\n---\n");
    out
}

/// Decode stego paste back to raw bytes.
pub fn stego_decode(paste: &str) -> Option<Vec<u8>> {
    let mut b64_chars = Vec::new();
    let mut in_bits = false;
    let mut byte: u8 = 0;
    let mut bit_count = 0;

    for ch in paste.chars() {
        match ch {
            '\u{200B}' => { in_bits = true; byte = 0; bit_count = 0; }
            '\u{200C}' if in_bits => { byte = (byte << 1) | 1; bit_count += 1; }
            '\u{200D}' if in_bits => { byte <<= 1; bit_count += 1; }
            ' ' | '\n' if in_bits && bit_count == 8 => {
                b64_chars.push(byte as char);
                in_bits = false;
            }
            _ => { in_bits = false; }
        }
    }

    let b64: String = b64_chars.into_iter().collect();
    base64::decode(&b64).ok()
}
