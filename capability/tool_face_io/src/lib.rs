//! The two faces of a nornir tool, written once (`NORNIR_CONVENTIONS.md` § Tool Structure).
//!
//! A tool is one function in its crate's library, returning `Result<Option<String>, E>`:
//! `Some(data)` for a tool that hands data back, `None` for one whose effect is the write.
//! Its CLI face and its PyO3 face each pass that result, with the engine's `error_type`
//! method, to the adapter here, so every tool presents an outcome the same way under the
//! contracts in `MUST_READ_BEFORE_BUILDING.md` § Approved interfaces.

use std::fmt::Display;
use std::io::Write;

/// Exit codes of the CLI contract.
pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_FAILURE: i32 = 1;
pub const EXIT_WRONG_ARGUMENTS: i32 = 2;

/// The tool's positional arguments, exactly N of them, or None when the count is wrong.
pub fn positional_args<const N: usize>() -> Option<[String; N]> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    args.try_into().ok()
}

/// CLI face, wrong arguments: the usage on stderr. Returns the exit code for `main()`.
pub fn cli_usage(usage: &str) -> i32 {
    eprintln!("{usage}");
    EXIT_WRONG_ARGUMENTS
}

/// CLI face, an outcome: data on stdout and exit 0; or the failure class on stderr's first
/// line, then the message, and exit 1. Returns the exit code for `main()`, which is the one
/// place `process::exit` is called. stdout is flushed here, because `process::exit` does not
/// flush it.
pub fn cli_report<E: Display>(outcome: Result<Option<String>, E>, class: fn(&E) -> &'static str) -> i32 {
    match outcome {
        Ok(None) => EXIT_SUCCESS,
        Ok(Some(data)) => {
            let mut stdout = std::io::stdout().lock();
            match stdout.write_all(data.as_bytes()).and_then(|()| stdout.flush()) {
                Ok(()) => EXIT_SUCCESS,
                Err(e) => {
                    eprintln!("write_error\nCould not write the result to stdout: {e}.");
                    EXIT_FAILURE
                }
            }
        }
        Err(e) => {
            eprintln!("{}\n{e}", class(&e));
            EXIT_FAILURE
        }
    }
}

/// PyO3 face, an outcome: `{"ok": bool, "data": str | None, "error": {"type": str, "message": str} | None}`.
/// It never raises for a failed outcome.
#[cfg(feature = "python")]
pub fn py_contract<E: Display>(
    py: pyo3::Python<'_>,
    outcome: Result<Option<String>, E>,
    class: fn(&E) -> &'static str,
) -> pyo3::PyResult<pyo3::PyObject> {
    use pyo3::prelude::*;
    use pyo3::types::PyDict;

    let dict = PyDict::new_bound(py);
    match outcome {
        Ok(data) => {
            dict.set_item("ok", true)?;
            dict.set_item("data", data)?;
            dict.set_item("error", py.None())?;
        }
        Err(e) => {
            let error = PyDict::new_bound(py);
            error.set_item("type", class(&e))?;
            error.set_item("message", e.to_string())?;
            dict.set_item("ok", false)?;
            dict.set_item("data", py.None())?;
            dict.set_item("error", error)?;
        }
    }
    Ok(dict.into())
}
