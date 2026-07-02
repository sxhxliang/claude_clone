use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use liteparse::{LiteParse, LiteParseConfig, OutputFormat, config::ImageMode};
use tokio::runtime::Runtime;

#[cfg(feature = "document-ocr")]
mod cli_ocr {
    use std::future::Future;
    use std::io::Write;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::process::{Command, Stdio};

    use liteparse::ocr::{OcrEngine, OcrOptions, OcrResult};

    pub(super) struct CliTesseractOcrEngine;

    impl CliTesseractOcrEngine {
        pub(super) fn new() -> Self {
            Self
        }
    }

    impl OcrEngine for CliTesseractOcrEngine {
        fn name(&self) -> &str {
            "tesseract-cli"
        }

        fn recognize<'a, 'b: 'a, 'c: 'a>(
            &'a self,
            image_data: &'c [u8],
            width: u32,
            height: u32,
            options: &'b OcrOptions,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<Vec<OcrResult>, Box<dyn std::error::Error + Send + Sync>>,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async move { recognize_with_tesseract(image_data, width, height, options) })
        }
    }

    fn recognize_with_tesseract(
        image_data: &[u8],
        width: u32,
        height: u32,
        options: &OcrOptions,
    ) -> Result<Vec<OcrResult>, Box<dyn std::error::Error + Send + Sync>> {
        if width == 0 || height == 0 || image_data.is_empty() {
            return Ok(Vec::new());
        }

        let expected_len = width as usize * height as usize * 3;
        if image_data.len() != expected_len {
            return Err(format!(
                "invalid OCR image buffer: expected {expected_len} bytes, got {}",
                image_data.len()
            )
            .into());
        }

        let exe = tesseract_exe().ok_or_else(|| {
            "tesseract.exe was not found. Install Tesseract or set TESSERACT_EXE.".to_string()
        })?;
        let tessdata_dir = tessdata_dir();
        let language = normalize_language(&options.language).to_string();
        let dpi = options.dpi.round().max(1.0).to_string();

        let mut child = Command::new(exe)
            .arg("stdin")
            .arg("stdout")
            .args(["--tessdata-dir", tessdata_dir.to_string_lossy().as_ref()])
            .args(["--dpi", &dpi])
            .args(["--psm", "3"])
            .args(["-l", &language])
            .args(["--loglevel", "OFF"])
            .arg("tsv")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        {
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| "failed to open tesseract stdin".to_string())?;
            write!(stdin, "P6\n{} {}\n255\n", width, height)?;
            stdin.write_all(image_data)?;
        }

        let output = child.wait_with_output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "tesseract OCR failed with status {}: {}",
                output.status,
                stderr.trim()
            )
            .into());
        }

        let stdout = String::from_utf8(output.stdout)?;
        parse_tsv(&stdout)
    }

    fn parse_tsv(tsv: &str) -> Result<Vec<OcrResult>, Box<dyn std::error::Error + Send + Sync>> {
        let mut results = Vec::new();

        for line in tsv.lines().skip(1) {
            let fields = line.splitn(12, '\t').collect::<Vec<_>>();
            if fields.len() < 12 {
                continue;
            }

            let left = fields[6].parse::<f32>().unwrap_or(0.0);
            let top = fields[7].parse::<f32>().unwrap_or(0.0);
            let width = fields[8].parse::<f32>().unwrap_or(0.0);
            let height = fields[9].parse::<f32>().unwrap_or(0.0);
            let confidence = fields[10].parse::<f32>().unwrap_or(-1.0);
            let text = fields[11].trim();

            if width <= 0.0 || height <= 0.0 || confidence < 0.0 || text.is_empty() {
                continue;
            }

            results.push(OcrResult {
                text: text.to_string(),
                bbox: [left, top, left + width, top + height],
                confidence: confidence / 100.0,
                polygon: None,
            });
        }

        Ok(results)
    }

    fn tesseract_exe() -> Option<PathBuf> {
        std::env::var_os("TESSERACT_EXE")
            .map(PathBuf::from)
            .filter(|path| path.exists())
            .or_else(|| {
                std::env::var_os("APPDATA")
                    .map(PathBuf::from)
                    .map(|path| {
                        path.join("tesseract-rs")
                            .join("tesseract")
                            .join("bin")
                            .join("tesseract.exe")
                    })
                    .filter(|path| path.exists())
            })
            .or_else(|| Some(PathBuf::from("tesseract")))
    }

    fn tessdata_dir() -> PathBuf {
        std::env::var_os("TESSDATA_PREFIX")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("APPDATA")
                    .map(PathBuf::from)
                    .map(|path| path.join("tesseract-rs").join("tessdata"))
            })
            .unwrap_or_else(|| PathBuf::from("tessdata"))
    }

    fn normalize_language(lang: &str) -> &str {
        match lang.to_lowercase().trim() {
            "en" => "eng",
            "fr" => "fra",
            "de" => "deu",
            "es" => "spa",
            "it" => "ita",
            "pt" => "por",
            "ru" => "rus",
            "zh" | "zh-cn" => "chi_sim",
            "zh-tw" => "chi_tra",
            "ja" => "jpn",
            "ko" => "kor",
            "ar" => "ara",
            "hi" => "hin",
            "th" => "tha",
            "vi" => "vie",
            _ => lang,
        }
    }
}

const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;

fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| Runtime::new().expect("failed to start document parser runtime"))
}

pub(crate) async fn parse_document(path: PathBuf, ocr_enabled: bool) -> Result<String, String> {
    if is_text_document_path(&path) {
        return parse_text_document(&path);
    }

    let text_result = parse_with_liteparse(&path, false).await;
    match &text_result {
        Ok(text) if !text.trim().is_empty() => return text_result,
        Ok(_) if !ocr_enabled => return text_result,
        Err(_) if !ocr_enabled => return text_result,
        _ => {}
    }

    if !ocr_available() {
        return match text_result {
            Ok(_) => Err(
                "No text was found in the document and OCR support is not built in. Rebuild with `cargo run -p claude_clone --features document-ocr` and install tesseract.exe to enable OCR fallback."
                    .to_string(),
            ),
            Err(err) => Err(format!(
                "{err}; OCR support is not built in. Rebuild with `cargo run -p claude_clone --features document-ocr` and install tesseract.exe to enable OCR fallback."
            )),
        };
    }

    match parse_with_liteparse(&path, true).await {
        Ok(text) if !text.trim().is_empty() => Ok(text),
        Ok(_) => Err("OCR fallback did not recognize any text in the document.".to_string()),
        Err(ocr_err) => match text_result {
            Ok(_) => Err(format!("OCR fallback failed: {ocr_err}")),
            Err(text_err) => Err(format!("{text_err}; OCR fallback failed: {ocr_err}")),
        },
    }
}

async fn parse_with_liteparse(path: &Path, ocr_enabled: bool) -> Result<String, String> {
    let path_string = path.to_string_lossy().to_string();
    let parser = LiteParse::new(LiteParseConfig {
        ocr_enabled,
        output_format: OutputFormat::Markdown,
        quiet: true,
        image_mode: ImageMode::Off,
        ..LiteParseConfig::default()
    });
    #[cfg(feature = "document-ocr")]
    let parser = if ocr_enabled {
        parser.with_ocr_engine(std::sync::Arc::new(cli_ocr::CliTesseractOcrEngine::new()))
    } else {
        parser
    };

    runtime()
        .spawn(async move { parser.parse(&path_string).await })
        .await
        .map_err(|err| format!("Document parser task failed: {err}"))?
        .map(|result| result.text.trim().to_string())
        .map_err(|err| format!("Document parsing failed: {err}"))
}

fn ocr_available() -> bool {
    cfg!(feature = "document-ocr")
}

pub(crate) fn is_supported_document_path(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    matches!(
        ext.to_ascii_lowercase().as_str(),
        "pdf"
            | "doc"
            | "docx"
            | "docm"
            | "dot"
            | "dotm"
            | "dotx"
            | "odt"
            | "ott"
            | "rtf"
            | "pages"
            | "ppt"
            | "pptx"
            | "pptm"
            | "pot"
            | "potm"
            | "potx"
            | "odp"
            | "otp"
            | "key"
            | "xls"
            | "xlsx"
            | "xlsm"
            | "xlsb"
            | "ods"
            | "ots"
            | "csv"
            | "tsv"
            | "numbers"
            | "txt"
            | "md"
            | "markdown"
            | "log"
    )
}

fn is_text_document_path(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    matches!(
        ext.to_ascii_lowercase().as_str(),
        "txt" | "md" | "markdown" | "log"
    )
}

fn parse_text_document(path: &Path) -> Result<String, String> {
    let metadata = std::fs::metadata(path).map_err(|err| format!("Document read failed: {err}"))?;
    if metadata.len() > MAX_TEXT_BYTES {
        return Err(format!(
            "Document is too large for text parsing ({} bytes)",
            metadata.len()
        ));
    }

    std::fs::read_to_string(path)
        .map(|text| text.trim().to_string())
        .map_err(|err| format!("Document read failed: {err}"))
}
