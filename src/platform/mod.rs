#[cfg(all(target_os = "android", not(feature = "termux")))]
compile_error!(
	"arboard has no clipboard backend for Android. If you are building for Android inside \
	Termux, enable the `termux` feature to access the clipboard through Termux's \
	`termux-clipboard-get`/`termux-clipboard-set` helpers."
);

#[cfg(all(unix, not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))))]
mod linux;
#[cfg(all(
	unix,
	not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
))]
pub use linux::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(target_os = "macos")]
mod osx;
#[cfg(target_os = "macos")]
pub use osx::*;

/// Android is not a desktop Linux, but Termux exposes the clipboard through the
/// `termux-clipboard-get`/`termux-clipboard-set` helpers.
#[cfg(all(target_os = "android", feature = "termux"))]
mod termux;
// Termux has no platform-specific extension traits, so nothing here needs to be
// re-exported outside of the crate.
#[cfg(all(target_os = "android", feature = "termux"))]
pub(crate) use termux::*;
