//! Colon-prefixed CLI shorthand → canonical `generate://` URI
//! translator (in the traditional terse media-tool style: `xc:red`,
//! `synth:5,sine,440`).
//!
//! Recognised prefixes (case-sensitive, lowercase by convention):
//!
//! | Shorthand                | Canonical                                       |
//! |--------------------------|-------------------------------------------------|
//! | `xc:red`                 | `generate://xc?color=red`                       |
//! | `xc:#ff0000`             | `generate://xc?color=%23ff0000`                 |
//! | `pattern:checkerboard`   | `generate://pattern?type=checkerboard`          |
//! | `gradient:red-blue`      | `generate://gradient?from=red&to=blue`          |
//! | `radial:red-blue`        | `generate://gradient?type=radial&from=red&to=blue` |
//! | `plasma:`                | `generate://plasma`                             |
//! | `mandelbrot:`            | `generate://fractal?type=mandelbrot`            |
//! | `julia:`                 | `generate://fractal?type=julia`                 |
//! | `synth:5,sine,440`       | `generate://synth?type=sine&freq=440&duration=5` |
//! | `testsrc:`               | `generate://testsrc`                            |
//! | `smptebars:`             | `generate://smptebars`                          |
//! | `zoneplate:`             | `generate://zoneplate`                          |
//! | `noise:perlin`           | `generate://noise?type=perlin`                  |
//! | `label:Hello world`      | `generate://label?text=Hello%20world`           |
//!
//! Inputs that don't start with a recognised prefix pass through
//! unchanged — the source registry decides whether they're file paths,
//! `file://` URIs, `http(s)://` URIs, or already-canonical
//! `generate://` URIs.

/// Translate a single CLI input arg to a `generate://` URI when the
/// shorthand is recognised; otherwise return the input unchanged.
pub fn translate(input: &str) -> String {
    if let Some(out) = try_translate(input) {
        out
    } else {
        input.to_string()
    }
}

fn try_translate(input: &str) -> Option<String> {
    if input.starts_with("generate://") {
        return None; // canonical form, leave alone
    }
    if let Some(rest) = input.strip_prefix("xc:") {
        return Some(format!("generate://xc?color={}", encode(rest)));
    }
    if let Some(rest) = input.strip_prefix("pattern:") {
        if rest.is_empty() {
            return Some("generate://pattern".to_string());
        }
        return Some(format!("generate://pattern?type={}", encode(rest)));
    }
    if let Some(rest) = input.strip_prefix("gradient:") {
        return Some(translate_gradient(rest, false));
    }
    if let Some(rest) = input.strip_prefix("radial:") {
        return Some(translate_gradient(rest, true));
    }
    if let Some(rest) = input.strip_prefix("plasma:") {
        if rest.is_empty() {
            return Some("generate://plasma".to_string());
        }
        return Some(format!("generate://plasma?seed={}", encode(rest)));
    }
    if let Some(rest) = input.strip_prefix("mandelbrot:") {
        if rest.is_empty() {
            return Some("generate://fractal?type=mandelbrot".to_string());
        }
        return Some(format!("generate://fractal?type=mandelbrot&{}", rest));
    }
    if let Some(rest) = input.strip_prefix("julia:") {
        if rest.is_empty() {
            return Some("generate://fractal?type=julia".to_string());
        }
        return Some(format!("generate://fractal?type=julia&{}", rest));
    }
    if let Some(rest) = input.strip_prefix("synth:") {
        return Some(translate_synth(rest));
    }
    if let Some(rest) = input.strip_prefix("testsrc:") {
        if rest.is_empty() {
            return Some("generate://testsrc".to_string());
        }
        return Some(format!("generate://testsrc?{rest}"));
    }
    if let Some(rest) = input.strip_prefix("smptebars:") {
        if rest.is_empty() {
            return Some("generate://smptebars".to_string());
        }
        return Some(format!("generate://smptebars?{rest}"));
    }
    if let Some(rest) = input.strip_prefix("zoneplate:") {
        if rest.is_empty() {
            return Some("generate://zoneplate".to_string());
        }
        return Some(format!("generate://zoneplate?{rest}"));
    }
    if let Some(rest) = input.strip_prefix("noise:") {
        if rest.is_empty() {
            return Some("generate://noise".to_string());
        }
        return Some(format!("generate://noise?type={}", encode(rest)));
    }
    if let Some(rest) = input.strip_prefix("label:") {
        // Whole tail is the text — IM's `label:Hello, world!` includes
        // the comma. Options like font/size/color come from the
        // canonical query form (`generate://label?text=…&size=48`) or
        // from sibling CLI flags in a follow-up; we don't try to parse
        // a `,key=val` suffix here because that would conflict with
        // text containing literal commas.
        return Some(format!("generate://label?text={}", encode(rest)));
    }
    None
}

/// `red-blue` → `from=red&to=blue`, `red` → `from=red`. Allows an
/// optional explicit query suffix after a `,`: `red-blue,w=32&h=32`.
fn translate_gradient(rest: &str, radial: bool) -> String {
    let (palette, extra) = match rest.split_once(',') {
        Some((p, e)) => (p, Some(e)),
        None => (rest, None),
    };
    let mut parts: Vec<String> = Vec::new();
    if radial {
        parts.push("type=radial".to_string());
    }
    if let Some((from, to)) = palette.split_once('-') {
        if !from.is_empty() {
            parts.push(format!("from={}", encode(from)));
        }
        if !to.is_empty() {
            parts.push(format!("to={}", encode(to)));
        }
    } else if !palette.is_empty() {
        parts.push(format!("from={}", encode(palette)));
    }
    if let Some(e) = extra {
        if !e.is_empty() {
            parts.push(e.to_string());
        }
    }
    if parts.is_empty() {
        "generate://gradient".to_string()
    } else {
        format!("generate://gradient?{}", parts.join("&"))
    }
}

/// `5,sine,440` → `type=sine&freq=440&duration=5`. The positional order
/// is the classical terse-CLI convention DURATION, TYPE, FREQ; extra
/// positional args after `freq` are ignored (we may add them in a
/// follow-up). Explicit `KEY=VALUE` pairs after `freq` are appended
/// verbatim.
fn translate_synth(rest: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut positional: Vec<&str> = Vec::new();
    for tok in rest.split(',') {
        if tok.contains('=') {
            parts.push(tok.to_string());
        } else {
            positional.push(tok);
        }
    }
    // Map positional args to the traditional terse-CLI order:
    // DURATION, TYPE, FREQ.
    if let Some(d) = positional.first() {
        if !d.is_empty() {
            parts.push(format!("duration={}", encode(d)));
        }
    }
    if let Some(t) = positional.get(1) {
        if !t.is_empty() {
            parts.push(format!("type={}", encode(t)));
        }
    }
    if let Some(f) = positional.get(2) {
        if !f.is_empty() {
            parts.push(format!("freq={}", encode(f)));
        }
    }
    if parts.is_empty() {
        "generate://synth".to_string()
    } else {
        format!("generate://synth?{}", parts.join("&"))
    }
}

/// Minimal RFC 3986 percent-encoder for the few unsafe chars our
/// generator URIs care about: `#`, `&`, ` `, `?`, `+`. Unreserved
/// characters pass through unchanged.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b',' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02x}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xc_named_color() {
        assert_eq!(translate("xc:red"), "generate://xc?color=red");
    }

    #[test]
    fn xc_hex_color_is_percent_encoded() {
        assert_eq!(translate("xc:#ff0000"), "generate://xc?color=%23ff0000");
    }

    #[test]
    fn pattern_checkerboard() {
        assert_eq!(
            translate("pattern:checkerboard"),
            "generate://pattern?type=checkerboard"
        );
    }

    #[test]
    fn gradient_two_colors() {
        assert_eq!(
            translate("gradient:red-blue"),
            "generate://gradient?from=red&to=blue"
        );
    }

    #[test]
    fn gradient_radial() {
        assert_eq!(
            translate("radial:red-blue"),
            "generate://gradient?type=radial&from=red&to=blue"
        );
    }

    #[test]
    fn plasma_bare() {
        assert_eq!(translate("plasma:"), "generate://plasma");
    }

    #[test]
    fn mandelbrot_bare() {
        assert_eq!(
            translate("mandelbrot:"),
            "generate://fractal?type=mandelbrot"
        );
    }

    #[test]
    fn julia_bare() {
        assert_eq!(translate("julia:"), "generate://fractal?type=julia");
    }

    #[test]
    fn synth_three_positional() {
        assert_eq!(
            translate("synth:5,sine,440"),
            "generate://synth?duration=5&type=sine&freq=440"
        );
    }

    #[test]
    fn synth_pluck_decay_via_kv() {
        assert_eq!(
            translate("synth:10,pluck,440"),
            "generate://synth?duration=10&type=pluck&freq=440"
        );
    }

    #[test]
    fn testsrc_bare() {
        assert_eq!(translate("testsrc:"), "generate://testsrc");
    }

    #[test]
    fn smptebars_bare() {
        assert_eq!(translate("smptebars:"), "generate://smptebars");
    }

    #[test]
    fn zoneplate_bare() {
        assert_eq!(translate("zoneplate:"), "generate://zoneplate");
    }

    #[test]
    fn zoneplate_with_query() {
        assert_eq!(
            translate("zoneplate:w=128&h=128&k=0.1"),
            "generate://zoneplate?w=128&h=128&k=0.1"
        );
    }

    #[test]
    fn noise_perlin() {
        assert_eq!(translate("noise:perlin"), "generate://noise?type=perlin");
    }

    #[test]
    fn label_simple_text() {
        assert_eq!(
            translate("label:Hello world"),
            "generate://label?text=Hello%20world"
        );
    }

    #[test]
    fn label_with_comma_keeps_comma_in_text() {
        // Comma is part of the label, not an option separator. Comma
        // is in `encode()`'s unreserved set so it passes through; the
        // important thing is that nothing splits the text on it.
        assert_eq!(
            translate("label:Hello, world!"),
            "generate://label?text=Hello,%20world%21"
        );
    }

    #[test]
    fn label_empty_passes_empty_text() {
        assert_eq!(translate("label:"), "generate://label?text=");
    }

    #[test]
    fn unknown_prefix_passes_through() {
        assert_eq!(translate("foo.png"), "foo.png");
        assert_eq!(translate("file:///tmp/x.wav"), "file:///tmp/x.wav");
        assert_eq!(
            translate("https://example.com/x.mp4"),
            "https://example.com/x.mp4"
        );
    }

    #[test]
    fn already_canonical_passes_through() {
        let s = "generate://synth?type=sine";
        assert_eq!(translate(s), s);
    }
}
