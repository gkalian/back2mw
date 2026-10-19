//! Язык сообщений: журнал, замечания, тексты ошибок, вывод консоли и окно.
//!
//! Язык один на процесс. Сообщения собираются в строки там, где возникают (`Display` ошибок,
//! строки журнала, замечания), поэтому передать язык параметром нельзя: его пришлось бы тянуть
//! через каждую функцию всех крейтов. Вместо этого консоль и окно один раз вызывают [`set`], а
//! код пишет обе версии рядом:
//!
//! ```
//! let n = 3;
//! let line = lang::tr!("записано {n} файлов", "{n} files written");
//! assert_eq!(line, "3 files written");
//! ```
//!
//! Строки не вынесены в таблицы: пара стоит там, где показывается, её видно при чтении кода, а
//! пропустить перевод нельзя — второй аргумент обязателен.
//!
//! На вывод конвертера язык не влияет: в плагины и ассеты сообщения не попадают, прогон остаётся
//! байт-в-байт тем же. Сообщения `expect`/`assert!` не переводятся — это пояснения к
//! инвариантам для разработчика, как комментарии.

use std::cell::Cell;
use std::sync::atomic::{AtomicU8, Ordering};

/// Язык сообщений.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Lang {
    Ru,
    /// По умолчанию: без флага `--lang` и ключа `language` всё пишется по-английски.
    #[default]
    En,
}

impl Lang {
    /// Значение для этого языка: `ru` или `en`. Годится и для `&str`, и для `String`.
    pub fn tr<T>(self, ru: T, en: T) -> T {
        match self {
            Self::Ru => ru,
            Self::En => en,
        }
    }

    /// Код для `config.toml` (ключ `language`) и флага `--lang`.
    pub fn code(self) -> &'static str {
        self.tr("ru", "en")
    }

    /// Из кода; незнакомый код — `None`.
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

/// Язык процесса. Атомарный, потому что окно ведёт сборку в отдельном потоке.
static CURRENT: AtomicU8 = AtomicU8::new(EN);

thread_local! {
    /// Язык этого потока поверх общего ([`with`]).
    static SCOPED: Cell<Option<Lang>> = const { Cell::new(None) };
}

/// Выбирает язык процесса. Зовётся один раз при старте консоли и при выборе языка в окне.
pub fn set(lang: Lang) {
    CURRENT.store(lang.tr(RU, EN), Ordering::Relaxed);
}

/// Текущий язык.
pub fn current() -> Lang {
    SCOPED.get().unwrap_or_else(|| {
        if CURRENT.load(Ordering::Relaxed) == RU {
            Lang::Ru
        } else {
            Lang::En
        }
    })
}

/// Выполняет `body` с языком `lang` в этом потоке, не трогая язык процесса. Для тестов: они
/// идут параллельно в одном процессе, и [`set`] из одного менял бы тексты в остальных.
pub fn with<R>(lang: Lang, body: impl FnOnce() -> R) -> R {
    /// Возвращает прежний язык потока, даже если `body` запаниковал.
    struct Restore(Option<Lang>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SCOPED.set(self.0);
        }
    }
    let _restore = Restore(SCOPED.replace(Some(lang)));
    body()
}

/// Значение для текущего языка. Оба значения уже вычислены — для строк без подстановок;
/// с подстановками — макрос [`tr!`].
pub fn tr<T>(ru: T, en: T) -> T {
    current().tr(ru, en)
}

/// `format!` на текущем языке: `tr!("русский {x}", "english {x}", …)`. Аргументы общие для
/// обоих шаблонов и вычисляются один раз — только у выбранного.
#[macro_export]
macro_rules! tr {
    ($ru:literal, $en:literal $(, $($arg:tt)*)?) => {
        match $crate::current() {
            $crate::Lang::Ru => ::std::format!($ru $(, $($arg)*)?),
            $crate::Lang::En => ::std::format!($en $(, $($arg)*)?),
        }
    };
}

/// `write!` на текущем языке: `trw!(f, "русский {x}", "english {x}", …)` — для `Display`.
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

        let caught = std::panic::catch_unwind(|| with(Lang::Ru, || panic!("внутри")));
        assert!(caught.is_err());
        assert_eq!(current(), Lang::En);
    }

    #[test]
    fn with_does_not_leak_into_other_threads() {
        with(Lang::Ru, || {
            let other = std::thread::spawn(current)
                .join()
                .expect("поток завершился");
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
            crate::trw!(out, "файлов {}", "{} files", n).expect("запись в String не падает");
            assert_eq!(out, "файлов 3");
        });
    }
}
