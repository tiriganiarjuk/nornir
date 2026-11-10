//! Text rendering of a closure.
//!
//!   render_symbols  — the contract bytes: one block per symbol, canonical order
//!   render_closure  — the symbols, then the findings block if there are any
//!   render_simulate — CLI view with banners separating the closure from the file
//!
//! No prose addressed to a reader lives here; a consumer that wants to say
//! what the text is for prepends its own sentence.

use crate::{Closure, ResolveMode};

/// One block per symbol: a header line, the rendered text, a blank line.
/// This is the text a digest is taken over.
pub fn render_symbols(closure: &Closure) -> String {
    let mut out = String::new();
    for symbol in &closure.symbols {
        out.push_str(&format!("\u{2500}\u{2500} {} \u{2500}\u{2500}\n", symbol.key()));
        out.push_str(&symbol.rendered);
        out.push_str("\n\n");
    }
    out
}

/// The findings, one per line under a header; empty when there are none.
pub fn render_findings(closure: &Closure) -> String {
    if closure.findings.is_empty() {
        return String::new();
    }
    let mut out = String::from("\u{2500}\u{2500} findings \u{2500}\u{2500}\n");
    for finding in &closure.findings {
        out.push_str(&finding.to_string());
        out.push('\n');
    }
    out
}

/// Symbols followed by findings. Empty when there is neither.
pub fn render_closure(closure: &Closure) -> String {
    let mut out = render_symbols(closure);
    out.push_str(&render_findings(closure));
    out
}

/// The simulate view: closure under an injection banner, then the file content.
pub fn render_simulate(closure: &Closure, file_content: &str, mode: ResolveMode) -> String {
    let mut out = format!(
        "\u{2550}\u{2550}\u{2550} INJECTION ({}) \u{2550}\u{2550}\u{2550}\n\n",
        mode.label()
    );
    let body = render_closure(closure);
    if body.is_empty() {
        out.push_str("(no project-local dependencies)\n\n");
    } else {
        out.push_str(&body);
    }
    out.push_str("\u{2550}\u{2550}\u{2550} FILE CONTENT \u{2550}\u{2550}\u{2550}\n\n");
    out.push_str(file_content);
    out
}
