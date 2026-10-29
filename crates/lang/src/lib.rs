//! Message language: the log, notes, error texts, console output and the window.
//!
//! There is one language per process. Messages are assembled into strings where they arise (error
//! `Display`, log lines, notes), so the language cannot be passed as a parameter: it would have to
//! be threaded through every function of every crate. Instead the console and the window call
//! [`set`] once, and the code writes both versions side by side:
//!
//! ```
//! let n = 3;
//! let line = lang::tr!("записано {n} файлов", "{n} files written");
//! assert_eq!(line, "3 files written");
//! ```
//!
//! The strings are not moved out into tables: a pair stands where it is shown, it is visible when
//! reading the code, and a translation cannot be skipped — the second argument is required.
//!
//! The language does not affect the converter's output: messages do not get into plugins and
//! assets, and a run stays byte-for-byte the same. `expect`/`assert!` messages are not translated —
//! they are notes on invariants for the developer, written in English like the comments.

use std::cell::Cell;
use std::sync::atomic::{AtomicU8, Ordering};

/// Message language.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Lang {
    Ru,
    /// The default: without the `--lang` flag and the `language` key everything is written in
    /// English.
    #[default]
    En,
}

impl Lang {
    /// The value for this language: `ru` or `en`. Works both for `&str` and for `String`.
    pub fn tr<T>(self, ru: T, en: T) -> T {
        match self {
            Self::Ru => ru,
            Self::En => en,
        }
    }

    /// The code for `config.toml` (the `language` key) and the `--lang` flag.
    pub fn code(self) -> &'static str {
        self.tr("ru", "en")
    }

    /// From a code; an unknown code gives `None`.
    pub fn from_code(code: &str) -> Option<Self> {
        match code.to_ascii_lowercase().as_str() {
            "ru" => Some(Self::Ru),
            "en" => Some(Self::En),
            _ => None,
        }
    }
}

const RU: u8 = 0;
const EN: u8 = 1;

/// The process language. Atomic, because the window runs the build in a separate thread.
static CURRENT: AtomicU8 = AtomicU8::new(EN);

thread_local! {
    /// This thread's language on top of the shared one ([`with`]).
    static SCOPED: Cell<Option<Lang>> = const { Cell::new(None) };
}

/// Chooses the process language. Called once at console startup and when the language is chosen in
/// the window.
pub fn set(lang: Lang) {
    CURRENT.store(lang.tr(RU, EN), Ordering::Relaxed);
}

/// The current language.
pub fn current() -> Lang {
    SCOPED.get().unwrap_or_else(|| {
        if CURRENT.load(Ordering::Relaxed) == RU {
            Lang::Ru
        } else {
            Lang::En
        }
    })
}

/// Runs `body` with language `lang` in this thread without touching the process language. For
/// tests: they run in parallel in one process, and [`set`] from one would change the texts in the
/// others.
pub fn with<R>(lang: Lang, body: impl FnOnce() -> R) -> R {
    /// Restores the thread's previous language, even if `body` panicked.
    struct Restore(Option<Lang>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SCOPED.set(self.0);
        }
    }
    let _restore = Restore(SCOPED.replace(Some(lang)));
    body()
}

/// The value for the current language. Both values are already computed — for strings without
/// substitutions; with substitutions use the [`tr!`] macro.
pub fn tr<T>(ru: T, en: T) -> T {
    current().tr(ru, en)
}

/// `format!` in the current language: `tr!("<russian> {x}", "<english> {x}", …)`. The arguments are
/// shared by both templates and evaluated once — only for the chosen one.
#[macro_export]
macro_rules! tr {
    ($ru:literal, $en:literal $(, $($arg:tt)*)?) => {
        match $crate::current() {
            $crate::Lang::Ru => ::std::format!($ru $(, $($arg)*)?),
            $crate::Lang::En => ::std::format!($en $(, $($arg)*)?),
        }
    };
}

/// `write!` in the current language: `trw!(f, "<russian> {x}", "<english> {x}", …)` — for
/// `Display`.
#[macro_export]
macro_rules! trw {
    ($dst:expr, $ru:literal, $en:literal $(, $($arg:tt)*)?) => {
        match $crate::current() {
            $crate::Lang::Ru => ::std::write!($dst, $ru $(, $($arg)*)?),
            $crate::Lang::En => ::std::write!($dst, $en $(, $($arg)*)?),
        }
    };
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::{Lang, current, tr, with};

    #[test]
    fn codes_round_trip_and_unknown_codes_are_rejected() {
        for lang in [Lang::Ru, Lang::En] {
            assert_eq!(Lang::from_code(lang.code()), Some(lang));
        }
        assert_eq!(Lang::from_code("EN"), Some(Lang::En));
        assert_eq!(Lang::from_code("de"), None);
    }

    #[test]
    fn english_is_the_default() {
        assert_eq!(Lang::default(), Lang::En);
        assert_eq!(current(), Lang::En);
        assert_eq!(tr("да", "yes"), "yes");
    }

    #[test]
    fn tr_picks_the_value_of_the_language() {
        assert_eq!(Lang::Ru.tr("да", "yes"), "да");
        assert_eq!(Lang::En.tr("да", "yes"), "yes");
    }

    #[test]
    fn with_switches_only_inside_the_call_and_restores_after_a_panic() {
        assert_eq!(with(Lang::Ru, || tr("да", "yes")), "да");
        assert_eq!(current(), Lang::En);

        let caught = std::panic::catch_unwind(|| with(Lang::Ru, || panic!("inside")));
        assert!(caught.is_err());
        assert_eq!(current(), Lang::En);
    }

    #[test]
    fn with_does_not_leak_into_other_threads() {
        with(Lang::Ru, || {
            let other = std::thread::spawn(current)
                .join()
                .expect("the thread finished");
            assert_eq!(other, Lang::En);
        });
    }

    #[test]
    fn macros_format_in_the_current_language() {
        let n = 3;
        assert_eq!(crate::tr!("файлов {n}", "{n} files"), "3 files");
        assert_eq!(crate::tr!("файлов {}", "{} files", n + 1), "4 files");
        assert_eq!(crate::tr!("пусто", "empty"), "empty");
        with(Lang::Ru, || {
            assert_eq!(crate::tr!("файлов {n}", "{n} files"), "файлов 3");
            let mut out = String::new();
            crate::trw!(out, "файлов {}", "{} files", n)
                .expect("writing to a String does not fail");
            assert_eq!(out, "файлов 3");
        });
    }
}
