//! Opening something outside the app.
//!
//! One feature so far — handing a URL from the conversation to the user's
//! browser — and it earns its own module for one reason: this is the only place
//! where **text a model wrote causes an action outside this process**. `docs/12`
//! §3.1 renders assistant markdown, and a link in an answer is model-authored, so
//! the decision of what may be opened is made here in the host rather than in the
//! webview that displays it.
//!
//! # Why not `tauri-plugin-opener`
//!
//! Measured: the plugin pulls `zbus` and its D-Bus tree (about twenty crates) onto
//! Linux to call the same `open` crate this module uses. Our own command is
//! ~3 crates, the allowlist stays in one reviewed place, and because it is *our*
//! command rather than a plugin's, no Tauri capability has to be granted to the
//! webview — the frontend asks the host, and the host decides.

/// The schemes that may be handed to the user's default handler.
///
/// Deliberately short. `file:` would let a model-authored link open a local path
/// in whatever the desktop associates with it, and the long tail of app schemes
/// (`vscode:`, `ssh:`, `obsidian:`) can launch programs with arguments — none of
/// that should be reachable from an answer's text.
const ALLOWED_SCHEMES: [&str; 3] = ["http", "https", "mailto"];

/// Open a URL in the user's default application.
///
/// Detached: the app must not hold a browser process as a child, and must not wait
/// for one to exit before its window responds.
pub fn open(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("no URL to open".to_string());
    }

    let scheme = scheme_of(trimmed)
        .ok_or_else(|| format!("`{trimmed}` does not start with a URL scheme"))?;
    if !is_allowed(scheme) {
        return Err(format!("refusing to open a `{scheme}:` link"));
    }

    open::that_detached(trimmed).map_err(|error| format!("could not open `{trimmed}`: {error}"))
}

/// The scheme a URL starts with, as written.
///
/// Follows RFC 3986's shape — a letter, then letters, digits, `+`, `-` or `.` —
/// rather than a looser "everything before the colon", so `C:\path` is not read as
/// a scheme named `C` and whitespace cannot hide one.
fn scheme_of(url: &str) -> Option<&str> {
    let (scheme, _) = url.split_once(':')?;
    let mut chars = scheme.chars();
    let well_formed = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));

    well_formed.then_some(scheme)
}

/// Whether a URL is one this module would open.
///
/// Exposed because the frontend makes the same decision when it decides whether a
/// link is clickable at all — and a scheme is case-insensitive (RFC 3986 §3.1), so
/// `HTTPS://…` is the same scheme as `https://…`.
pub fn is_openable(url: &str) -> bool {
    scheme_of(url.trim()).is_some_and(is_allowed)
}

fn is_allowed(scheme: &str) -> bool {
    ALLOWED_SCHEMES
        .iter()
        .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_allowed_schemes_pass() {
        for url in [
            "https://omp.sh/docs",
            "http://127.0.0.1:5173",
            "mailto:someone@example.com",
            "HTTPS://OMP.SH",
        ] {
            assert!(is_openable(url), "{url} must be openable");
        }
    }

    #[test]
    fn everything_else_is_refused() {
        for url in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "vscode://file/etc/passwd",
            "ssh://host",
            "data:text/html,<script>",
            "not a url",
            "C:\\Windows\\System32",
            "",
            "   ",
        ] {
            assert!(!is_openable(url), "{url} must not be openable");
        }
    }

    #[test]
    fn a_refusal_explains_itself_and_never_opens() {
        // The message is what a user sees when a model hands them a link the host
        // will not follow, so it names the reason rather than failing silently.
        let error = open("javascript:alert(1)").expect_err("must refuse");
        assert!(error.contains("javascript"), "got {error}");

        let error = open("   ").expect_err("must refuse");
        assert!(error.contains("no URL"), "got {error}");
    }

    #[test]
    fn a_scheme_hidden_behind_whitespace_or_case_is_still_read() {
        // `JAVASCRIPT:` and a leading space would otherwise slip past a naive
        // prefix check.
        assert!(!is_openable("  javascript:alert(1)"));
        assert!(!is_openable("JaVaScRiPt:alert(1)"));
    }
}
