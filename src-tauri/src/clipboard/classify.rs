//! Classification of captured clipboard items into sub_kinds.
//!
//! OWNER: worker W2.

use std::path::Path;

use crate::model::SubKind;

/// Classifies a text string into an appropriate `SubKind`.
pub fn classify_text(text: &str) -> SubKind {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return SubKind::Plain;
    }

    // 1. Link: single well-formed URL
    if is_single_url(trimmed) {
        return SubKind::Link;
    }

    // 2. Color: #RGB, #RRGGBB, #RGBA, #RRGGBBAA, rgb(...), rgba(...), hsl(...), hsla(...)
    if is_color(trimmed) {
        return SubKind::Color;
    }

    // 3. Code: >30% indented lines, valid JSON object/array, or XML/HTML tags
    if is_code(text, trimmed) {
        return SubKind::Code;
    }

    SubKind::Plain
}

/// Checks if a string is a single well-formed URL.
fn is_single_url(s: &str) -> bool {
    // Must not contain whitespace or newlines
    if s.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }

    // Check scheme
    let (scheme, rest) = if let Some(idx) = s.find("://") {
        let (sch, rest) = s.split_at(idx);
        (sch, &rest[3..])
    } else if let Some(rest) = s.strip_prefix("mailto:") {
        ("mailto", rest)
    } else {
        return false;
    };

    let scheme_lower = scheme.to_ascii_lowercase();
    let valid_scheme = matches!(
        scheme_lower.as_str(),
        "http" | "https" | "ftp" | "ftps" | "file" | "ws" | "wss" | "mailto"
    );

    if !valid_scheme || rest.is_empty() {
        return false;
    }

    if scheme_lower == "mailto" {
        return rest.contains('@') && rest.contains('.');
    }

    // For file:///, path must be present
    if scheme_lower == "file" {
        return !rest.is_empty();
    }

    // For web protocols, host must be non-empty
    let host_part = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host_part.is_empty() {
        return false;
    }
    let host_no_port = host_part.split(':').next().unwrap_or(host_part);
    host_no_port.contains('.')
        || host_no_port.eq_ignore_ascii_case("localhost")
        || host_no_port == "127.0.0.1"
        || host_no_port.starts_with('[')
}

/// Checks if a string matches CSS/hex color formats.
fn is_color(s: &str) -> bool {
    // Hex colors: #RGB, #RGBA, #RRGGBB, #RRGGBBAA
    if let Some(hex) = s.strip_prefix('#') {
        let len = hex.len();
        if (len == 3 || len == 4 || len == 6 || len == 8)
            && hex.chars().all(|c| c.is_ascii_hexdigit())
        {
            return true;
        }
    }

    // Functional colors: rgb(...), rgba(...), hsl(...), hsla(...)
    let lower = s.to_ascii_lowercase();
    if (lower.starts_with("rgb(")
        || lower.starts_with("rgba(")
        || lower.starts_with("hsl(")
        || lower.starts_with("hsla("))
        && lower.ends_with(')')
    {
        let Some(start) = lower.find('(') else {
            return false;
        };
        if lower.len() <= start + 1 {
            return false;
        }
        let inner = lower[start + 1..lower.len().saturating_sub(1)].trim();
        // Check that inner has comma or space separated values
        if !inner.is_empty() {
            let parts: Vec<&str> = if inner.contains(',') {
                inner.split(',').map(|p| p.trim()).collect()
            } else {
                inner.split_whitespace().filter(|p| *p != "/").collect()
            };
            if parts.len() == 3 || parts.len() == 4 {
                return true;
            }
        }
    }

    false
}

/// Checks if text is likely code (indentation, JSON, XML/HTML).
fn is_code(raw: &str, trimmed: &str) -> bool {
    // Check indentation heuristic on lines
    let non_empty_lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
    if non_empty_lines.len() >= 2 {
        let indented = non_empty_lines
            .iter()
            .filter(|l| l.starts_with(' ') || l.starts_with('\t'))
            .count();
        if (indented as f64) / (non_empty_lines.len() as f64) > 0.3 {
            return true;
        }
    }

    // Check if valid JSON object or array
    if ((trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']')))
        && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
    {
        return true;
    }

    // Check if XML / HTML snippet
    if trimmed.starts_with('<')
        && trimmed.ends_with('>')
        && (trimmed.starts_with("<?xml")
            || trimmed.starts_with("<!DOCTYPE")
            || trimmed.starts_with("<!--")
            || trimmed.starts_with("<html")
            || trimmed.starts_with("<svg")
            || looks_like_html_xml_tag(trimmed))
    {
        return true;
    }

    false
}

/// Quick check for valid-looking XML/HTML tag structure
fn looks_like_html_xml_tag(s: &str) -> bool {
    if s.starts_with('<') && s.ends_with('>') {
        let inside = &s[1..s.len() - 1];
        let tag_name = inside
            .split([' ', '>', '/', '\t', '\n'])
            .next()
            .unwrap_or("");
        if !tag_name.is_empty()
            && tag_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ':')
        {
            return true;
        }
    }
    false
}

/// Checks if an image is animated (GIF with multiple frames or WebP with ANIM chunk).
pub fn is_animated_image(bytes: &[u8]) -> bool {
    if bytes.len() < 12 {
        return false;
    }

    // GIF check
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        let mut count = 0;
        for window in bytes.windows(2) {
            // Graphic control extension (0x21, 0xF9) indicates frames
            if window == [0x21, 0xF9] {
                count += 1;
                if count > 1 {
                    return true;
                }
            }
        }
        return count > 1;
    }

    // WebP check (RIFF....WEBP with ANIM chunk)
    if bytes.starts_with(b"RIFF")
        && bytes.len() >= 16
        && &bytes[8..12] == b"WEBP"
        && bytes.windows(4).any(|w| w == b"ANIM")
    {
        return true;
    }

    false
}

/// Checks if a file path has a video extension.
pub fn is_video_file(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    matches!(
        ext.as_str(),
        "mp4"
            | "mkv"
            | "avi"
            | "mov"
            | "wmv"
            | "flv"
            | "webm"
            | "m4v"
            | "3gp"
            | "ts"
            | "mpg"
            | "mpeg"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_links() {
        assert_eq!(classify_text("https://example.com"), SubKind::Link);
        assert_eq!(
            classify_text("http://localhost:3000/api/items?id=12#top"),
            SubKind::Link
        );
        assert_eq!(
            classify_text("https://github.com/reteren/rebuffer"),
            SubKind::Link
        );
        assert_eq!(classify_text("mailto:user@example.com"), SubKind::Link);
        assert_eq!(
            classify_text("file:///C:/Users/test/file.txt"),
            SubKind::Link
        );

        // Not links
        assert_eq!(
            classify_text("This is not a link: https://example.com"),
            SubKind::Plain
        );
        assert_eq!(
            classify_text("https://example.com and some text"),
            SubKind::Plain
        );
        assert_eq!(classify_text("http://"), SubKind::Plain);
    }

    #[test]
    fn test_classify_colors() {
        assert_eq!(classify_text("#fff"), SubKind::Color);
        assert_eq!(classify_text("#FFFFFF"), SubKind::Color);
        assert_eq!(classify_text("#7aa2ff"), SubKind::Color);
        assert_eq!(classify_text("#12345678"), SubKind::Color);
        assert_eq!(classify_text("rgb(255, 0, 128)"), SubKind::Color);
        assert_eq!(classify_text("rgba(255, 0, 128, 0.5)"), SubKind::Color);
        assert_eq!(classify_text("hsl(120, 100%, 50%)"), SubKind::Color);
        assert_eq!(classify_text("hsla(120, 100%, 50%, 0.8)"), SubKind::Color);

        // Not colors
        assert_eq!(classify_text("#notacolor"), SubKind::Plain);
        assert_eq!(classify_text("#12"), SubKind::Plain);
        assert_eq!(classify_text("color: #fff"), SubKind::Plain);
    }

    #[test]
    fn test_classify_code() {
        // Indented code
        let indented = "fn main() {\n    let x = 10;\n    println!(\"{}\", x);\n}";
        assert_eq!(classify_text(indented), SubKind::Code);

        let python = "def foo():\n    x = 1\n    return x";
        assert_eq!(classify_text(python), SubKind::Code);

        // JSON
        let json = r#"{"name": "Rebuffer", "version": 1, "enabled": true}"#;
        assert_eq!(classify_text(json), SubKind::Code);

        let json_arr = r#"[1, 2, 3, "test"]"#;
        assert_eq!(classify_text(json_arr), SubKind::Code);

        // XML/HTML
        let xml = r#"<?xml version="1.0"?><root><name>test</name></root>"#;
        assert_eq!(classify_text(xml), SubKind::Code);

        let html = r#"<div class="card"><p>Hello World</p></div>"#;
        assert_eq!(classify_text(html), SubKind::Code);

        // Plain text
        let prose = "First line of a story.\nSecond line of a story.\nThird line of a story.";
        assert_eq!(classify_text(prose), SubKind::Plain);
    }

    #[test]
    fn test_video_detection() {
        assert!(is_video_file("C:/Videos/recording.mp4"));
        assert!(is_video_file("clip.MKV"));
        assert!(is_video_file("movie.webm"));
        assert!(is_video_file("test.mov"));
        assert!(!is_video_file("photo.png"));
        assert!(!is_video_file("document.pdf"));
        assert!(!is_video_file("audio.mp3"));
    }
}
