/*
SPDX-License-Identifier: Apache-2.0 OR MIT

Copyright 2022 The Arboard contributors

The project to which this file belongs is licensed under either of
the Apache 2.0 or the MIT license at the licensee's choice. The terms
and conditions of the chosen license apply to this file.
*/

use std::{
	borrow::Cow,
	io::Write,
	path::{Path, PathBuf},
	process::{Command, Stdio},
};

use crate::common::Error;
#[cfg(feature = "image-data")]
use crate::common::ImageData;

/// The Termux:API helper that reads the clipboard contents.
const GET_COMMAND: &str = "termux-clipboard-get";

/// The Termux:API helper that writes to the clipboard.
const SET_COMMAND: &str = "termux-clipboard-set";

fn into_unknown<E: std::fmt::Display>(error: E) -> Error {
	Error::unknown(error.to_string())
}

/// Returns `true` if `command` can be found in `PATH`.
fn command_exists(command: &str) -> bool {
	let Some(path) = std::env::var_os("PATH") else {
		return false;
	};

	std::env::split_paths(&path).any(|dir| dir.join(command).is_file())
}

/// Turns a failed helper invocation into an error.
///
/// The helpers report failures from the `com.termux.api` app as a JSON object on stdout
/// (for instance when the app isn't running or the request was aborted), so its contents
/// are included in the error message.
fn command_failed(command: &str, output: &std::process::Output) -> Error {
	let mut message = format!("`{command}` failed with status {}", output.status);

	for (stream, contents) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
		let contents = String::from_utf8_lossy(contents);
		let contents = contents.trim();
		if !contents.is_empty() {
			message.push_str(&format!(": {stream}: {contents}"));
		}
	}

	Error::unknown(message)
}

/// Shells out to `termux-clipboard-get` and returns the raw clipboard bytes.
///
/// Note that the helper communicates with the `com.termux.api` app through a
/// broadcast, which means that it can be slow and, on some devices, requires the
/// screen to be unlocked.
fn read_clipboard() -> Result<Vec<u8>, Error> {
	let output = Command::new(GET_COMMAND)
		.stdin(Stdio::null())
		.output()
		.map_err(|e| into_unknown(format!("failed to run `{GET_COMMAND}`: {e}")))?;

	if !output.status.success() {
		return Err(command_failed(GET_COMMAND, &output));
	}

	Ok(output.stdout)
}

/// Shells out to `termux-clipboard-set`, passing the contents over stdin.
///
/// The text is deliberately *not* passed as an argument: the helper joins all its
/// arguments with a space and adds a trailing newline, which would corrupt the
/// value that we're setting.
fn write_clipboard(contents: &str) -> Result<(), Error> {
	let mut child = Command::new(SET_COMMAND)
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.map_err(|e| into_unknown(format!("failed to run `{SET_COMMAND}`: {e}")))?;

	// Take the pipe out of the child so that it gets closed once we're done writing;
	// otherwise the helper would wait for stdin to hit EOF forever.
	{
		let mut stdin = child.stdin.take().ok_or_else(|| {
			Error::unknown(format!("failed to open the stdin of `{SET_COMMAND}`"))
		})?;
		stdin.write_all(contents.as_bytes()).map_err(|e| {
			// The helper may have already exited, in which case writing fails with EPIPE
			// and the real error is available in its output instead.
			into_unknown(format!("failed to write to `{SET_COMMAND}`: {e}"))
		})?;
	}

	let output = child
		.wait_with_output()
		.map_err(|e| into_unknown(format!("failed to wait for `{SET_COMMAND}`: {e}")))?;

	if !output.status.success() {
		return Err(command_failed(SET_COMMAND, &output));
	}

	Ok(())
}

/// A shim clipboard type that can have operations performed with it, but
/// does not represent an open clipboard itself.
///
/// Android's clipboard is a global object owned by the system, and Termux has no
/// way of holding on to it, so every operation is self-contained and talks to the
/// `com.termux.api` app through its command line helpers instead.
pub(crate) struct Clipboard(());

// The other platforms have a `Drop` implementation on their clipboard, so this one
// should too for consistency.
impl Drop for Clipboard {
	fn drop(&mut self) {}
}

impl Clipboard {
	pub(crate) fn new() -> Result<Self, Error> {
		if !command_exists(GET_COMMAND) || !command_exists(SET_COMMAND) {
			return Err(Error::ClipboardNotSupported);
		}

		Ok(Self(()))
	}
}

pub(crate) struct Get<'clipboard> {
	_clipboard: &'clipboard mut Clipboard,
}

impl<'clipboard> Get<'clipboard> {
	pub(crate) fn new(clipboard: &'clipboard mut Clipboard) -> Self {
		Self { _clipboard: clipboard }
	}

	pub(crate) fn text(self) -> Result<String, Error> {
		let bytes = read_clipboard()?;

		// An empty response is what an empty clipboard looks like through this API,
		// and there is no way of telling it apart from a failure to read.
		if bytes.is_empty() {
			return Err(Error::ContentNotAvailable);
		}

		String::from_utf8(bytes).map_err(|_| Error::ConversionFailure)
	}

	pub(crate) fn html(self) -> Result<String, Error> {
		Err(Error::ClipboardNotSupported)
	}

	#[cfg(feature = "image-data")]
	pub(crate) fn image(self) -> Result<ImageData<'static>, Error> {
		Err(Error::ClipboardNotSupported)
	}

	pub(crate) fn file_list(self) -> Result<Vec<PathBuf>, Error> {
		Err(Error::ClipboardNotSupported)
	}
}

pub(crate) struct Set<'clipboard> {
	_clipboard: &'clipboard mut Clipboard,
}

impl<'clipboard> Set<'clipboard> {
	pub(crate) fn new(clipboard: &'clipboard mut Clipboard) -> Self {
		Self { _clipboard: clipboard }
	}

	pub(crate) fn text(self, text: Cow<'_, str>) -> Result<(), Error> {
		write_clipboard(&text)
	}

	pub(crate) fn html(self, html: Cow<'_, str>, alt: Option<Cow<'_, str>>) -> Result<(), Error> {
		// The clipboard only carries plain text through this API, so the alternative
		// text is what ends up being pasted.
		let _ = html;
		write_clipboard(&alt.unwrap_or(Cow::Borrowed("")))
	}

	#[cfg(feature = "image-data")]
	pub(crate) fn image(self, _image: ImageData<'_>) -> Result<(), Error> {
		Err(Error::ClipboardNotSupported)
	}

	pub(crate) fn file_list(self, _file_list: &[impl AsRef<Path>]) -> Result<(), Error> {
		Err(Error::ClipboardNotSupported)
	}
}

pub(crate) struct Clear<'clipboard> {
	_clipboard: &'clipboard mut Clipboard,
}

impl<'clipboard> Clear<'clipboard> {
	pub(crate) fn new(clipboard: &'clipboard mut Clipboard) -> Self {
		Self { _clipboard: clipboard }
	}

	pub(crate) fn clear(self) -> Result<(), Error> {
		write_clipboard("")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{common::CLIPBOARD_TEST_LOCK, Clipboard};
	use std::time::{Duration, Instant};

	/// The Android clipboard is global and shared with the rest of the device, so only one
	/// test at a time may use it.
	fn clipboard() -> (std::sync::MutexGuard<'static, ()>, Clipboard) {
		let guard = CLIPBOARD_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
		let clipboard = Clipboard::new().unwrap();
		(guard, clipboard)
	}

	/// Repeats `operation` until it produces a result that `is_done` accepts, or until a
	/// deadline passes.
	///
	/// Clipboard access goes through a broadcast to the Termux:API app, which is not
	/// guaranteed to see a change right away, and Android may deny that app access to the
	/// clipboard altogether while it is in the background, so a single attempt can fail for
	/// reasons that have nothing to do with `arboard`.
	fn eventually<T: std::fmt::Debug>(
		what: &str,
		mut operation: impl FnMut() -> T,
		is_done: impl Fn(&T) -> bool,
	) {
		let deadline = Instant::now() + Duration::from_secs(10);
		let mut last = None;

		while Instant::now() < deadline {
			let result = operation();
			if is_done(&result) {
				return;
			}
			last = Some(result);
			std::thread::sleep(Duration::from_millis(250));
		}

		panic!("gave up waiting for {what}, last result was {last:?}");
	}

	#[test]
	fn text_roundtrips() {
		let (_guard, mut clipboard) = clipboard();

		for text in [
			"some string",
			"Some utf8: 🤓 ∑φ(n)<ε 🐔",
			"trailing newline\n",
			"embedded\nnewlines\n",
			"  leading and trailing whitespace  ",
		] {
			eventually(
				"the text to be read back",
				|| {
					clipboard.set_text(text.to_owned()).unwrap();
					clipboard.get_text().ok()
				},
				|got| got.as_deref() == Some(text),
			);
		}
	}

	#[test]
	fn empty_clipboard_has_no_content() {
		let (_guard, mut clipboard) = clipboard();

		eventually(
			"the clipboard to be empty",
			|| {
				clipboard.clear().unwrap();
				clipboard.get_text().err()
			},
			|err| matches!(err, Some(Error::ContentNotAvailable)),
		);
	}

	#[test]
	fn html_stores_the_alternative_text() {
		let (_guard, mut clipboard) = clipboard();

		eventually(
			"the alternative text to be read back",
			|| {
				clipboard.set_html("<b>hello</b>".to_owned(), Some("hello".to_owned())).unwrap();
				clipboard.get_text().ok()
			},
			|got| got.as_deref() == Some("hello"),
		);
	}

	#[test]
	fn unsupported_formats_report_so() {
		let (_guard, mut clipboard) = clipboard();

		assert!(matches!(clipboard.get().html(), Err(Error::ClipboardNotSupported)));
		assert!(matches!(clipboard.get().file_list(), Err(Error::ClipboardNotSupported)));
		assert!(matches!(
			clipboard.set().file_list(&[PathBuf::from("foo")]),
			Err(Error::ClipboardNotSupported)
		));
	}
}
