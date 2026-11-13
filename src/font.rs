//! A tiny 5x7 bitmap font (digits, upper-case letters and a few signs) for the placeholder HUD of the `play` binary (std-only).
//! This is not the original's font: the original draws its score bar from pictures and Mac fonts, which are not ported yet.

pub const GLYPH_W: usize = 5;
pub const GLYPH_H: usize = 7;

/// Rows of a glyph, top to bottom, bit 4 = leftmost pixel. Unknown characters are blank; lower case maps to upper case.
pub fn glyph(c: char) -> [u8; GLYPH_H] {
    let c = c.to_ascii_uppercase();
    match c {
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        ':' => [0b00000, 0b00100, 0b00000, 0b00000, 0b00000, 0b00100, 0b00000],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        '+' => [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110],
        '&' => [0b01100, 0b10010, 0b10100, 0b01000, 0b10101, 0b10010, 0b01101],
        '/' => [0b00001, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b10000],
        '!' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100],
        '[' => [0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110],
        ']' => [0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110],
        _ => [0; GLYPH_H],
    }
}

/// Width in pixels of `text` at `scale` (6 columns per character).
pub fn text_width(text: &str, scale: usize) -> usize { text.chars().count() * (GLYPH_W + 1) * scale }

/// Draw `text` into an ARGB buffer (`stride` pixels per row) with its top-left corner at (x, y); pixels outside the buffer are skipped.
pub fn draw_text(buf: &mut [u32], stride: usize, x: i32, y: i32, text: &str, scale: usize, color: u32) {
    let rows = buf.len() / stride;
    for (i, ch) in text.chars().enumerate() {
        let g = glyph(ch);
        for (gy, bits) in g.iter().enumerate() {
            for gx in 0..GLYPH_W {
                if bits & (0b10000 >> gx) == 0 { continue; }
                for sy in 0..scale {
                    for sx in 0..scale {
                        let px = x + ((i * (GLYPH_W + 1) + gx) * scale + sx) as i32;
                        let py = y + (gy * scale + sy) as i32;
                        if px < 0 || py < 0 || px as usize >= stride || py as usize >= rows { continue; }
                        buf[py as usize * stride + px as usize] = color;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_glyph_fits_five_columns_and_is_not_blank() {
        for c in "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ:-+./![]".chars() {
            let g = glyph(c);
            assert!(g.iter().all(|&r| r < 32), "{}", c);
            assert!(g.iter().any(|&r| r != 0), "{}", c);
        }
        assert_eq!(glyph(' '), [0; GLYPH_H]);
        assert_eq!(glyph('a'), glyph('A'));
    }

    #[test]
    fn drawing_clips_and_scales() {
        let mut buf = vec![0u32; 20 * 10];
        draw_text(&mut buf, 20, 0, 0, "I", 1, 1);
        // 'I' = top bar 01110 -> columns 1..=3 of row 0
        assert_eq!(&buf[0..5], &[0, 1, 1, 1, 0]);
        let mut big = vec![0u32; 20 * 20];
        draw_text(&mut big, 20, 0, 0, "I", 2, 1);
        assert_eq!(&big[0..10], &[0, 0, 1, 1, 1, 1, 1, 1, 0, 0]);
        assert_eq!(&big[20..30], &[0, 0, 1, 1, 1, 1, 1, 1, 0, 0]);
        draw_text(&mut buf, 20, -100, -100, "I", 1, 7); // fully outside: no panic
        assert_eq!(text_width("AB", 2), 24);
    }
}
