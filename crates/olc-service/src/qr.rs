//! A QR code as a grid of squares, for a window to draw.
use qrcode::{Color, EcLevel, QrCode};

/// Square, read by rows from the top left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QrGrid {
    pub width: usize,
    pub dark: Vec<bool>,
}

impl QrGrid {
    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        self.dark.get(y * self.width + x).copied().unwrap_or(false)
    }
}

/// `None` only when the text is too long for any code.
pub fn qr_grid(text: &str) -> Option<QrGrid> {
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).ok()?;
    let width = code.width();
    let dark = code.to_colors().into_iter().map(|color| color == Color::Dark).collect();
    Some(QrGrid { width, dark })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_makes_a_square_code() {
        let grid = qr_grid("olc1qexampleexampleexampleexampleexample").unwrap();
        assert_eq!(grid.dark.len(), grid.width * grid.width);
        // The finder pattern at the top left is dark at its corner.
        assert!(grid.is_dark(0, 0));
        assert!(grid.dark.iter().any(|dark| !dark));
    }
}
