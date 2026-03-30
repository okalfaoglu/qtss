use chrono::Utc;
use printpdf::*;
use std::io::{BufWriter, Write};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PdfError {
    #[error("printpdf: {0}")]
    Print(#[from] printpdf::Error),
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
}

pub struct ReportRenderer;

impl ReportRenderer {
    pub fn simple_summary(title: &str, body: &str) -> Result<Vec<u8>, PdfError> {
        let (doc, page1, layer1) = PdfDocument::new(title, Mm(210.0), Mm(297.0), "ozet");
        let font = doc.add_builtin_font(BuiltinFont::Helvetica)?;
        let current_layer = doc.get_page(page1).get_layer(layer1);

        current_layer.use_text(
            format!("QTSS — {}", title),
            16.0,
            Mm(20.0),
            Mm(270.0),
            &font,
        );

        let generated = format!("Olusturulma: {}", Utc::now().format("%Y-%m-%d %H:%M UTC"));
        current_layer.use_text(generated, 10.0, Mm(20.0), Mm(255.0), &font);

        let mut y = 235.0;
        for line in body.lines().take(60) {
            current_layer.use_text(line.to_string(), 10.0, Mm(20.0), Mm(y), &font);
            y -= 6.0;
            if y < 20.0 {
                break;
            }
        }

        let mut buf = BufWriter::new(Vec::new());
        doc.save(&mut buf)?;
        buf.flush()?;
        Ok(buf
            .into_inner()
            .map_err(|e| std::io::Error::other(e.to_string()))?)
    }
}
