//! CLI: стадии прогона отдельными подкомандами и `all`.
//!
//! Требования к CLI зафиксированы с самого начала, потому что GUI — тонкая обёртка над той же
//! библиотекой, а не переписывание: шаг — отдельная подкоманда, прогресс и ошибки построчно в
//! stdout, осмысленный код возврата, никакого интерактивного ввода.
//!
//! Коды возврата: 0 — успех, 1 — прогон не удался (в том числе `validate` нашёл брак),
//! 2 — неверные аргументы или конфигурация, 3 — стадия отдельной командой не запускается.
//!
//! Язык сообщений (`lang`): флаг `--lang`, иначе ключ `language` из `config.toml` (его пишет
//! окно), иначе английский.

use std::path::PathBuf;
use std::process::ExitCode;

use back2mw::config::{self, CONFIG_FILE_NAME, Needs, Overrides};
use back2mw::pipeline::{self, PipelineError, Request};
use back2mw::{install, package, settings, validate};
use lang::Lang;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Стадии прогона в порядке исполнения: имя, описание по-русски и по-английски. Стадия, для
/// которой в источнике нет материала, пропускается — это нормальное поведение, а не особый
/// режим. Отдельной командой запускаются `validate`, `package`, `install`, `uninstall`;
/// остальные выполняет `all`.
const STAGES: &[(&str, &str, &str)] = &[
    (
        "discover",
        "проверить пути к играм, выбрать плагины и смещение",
        "check the game paths, pick the plugins and the offset",
    ),
    (
        "extract",
        "распаковать BSA источника",
        "unpack the source BSAs",
    ),
    (
        "read",
        "разобрать плагины источника",
        "parse the source plugins",
    ),
    (
        "transform",
        "отобразить записи и характеристики",
        "map records and stats",
    ),
    (
        "world",
        "слить ячейки 2×2, сшить ландшафт, применить смещение",
        "merge cells 2×2, stitch the landscape, apply the offset",
    ),
    (
        "meshes",
        "NIF 20.0.0.4 → 4.0.0.2, скопировать текстуры",
        "NIF 20.0.0.4 → 4.0.0.2, copy the textures",
    ),
    ("faces", "запечь лица NPC", "bake NPC faces"),
    (
        "audio",
        "звуки: WAV и mp3 из BSA, стерео → моно",
        "sounds: WAV and mp3 from the BSAs, stereo → mono",
    ),
    (
        "write",
        "записать плагины (по одному на плагин источника) и разложить ассеты",
        "write the plugins (one per source plugin) and lay out the assets",
    ),
    (
        "validate",
        "esmtool и niftest по готовому моду в output/",
        "esmtool and niftest over the built mod in output/",
    ),
    (
        "package",
        "собрать zip (и BSA на каждый плагин с --bsa) из output/",
        "build a zip (and a BSA per plugin with --bsa) from output/",
    ),
    (
        "install",
        "скопировать мод в Data Files с манифестом",
        "copy the mod into Data Files with a manifest",
    ),
    (
        "uninstall",
        "откатить установку по манифесту",
        "roll the install back by the manifest",
    ),
];

fn usage() {
    let stages: String = STAGES
        .iter()
        .map(|(name, ru, en)| format!("  {name:<10} {}\n", lang::tr(*ru, *en)))
        .collect();
    print!(
        "{}",
        lang::tr!(
            "back2mw {VERSION} — конвертер содержимого Oblivion в формат Morrowind

Использование:
  back2mw all [опции]
  back2mw <стадия> [опции]

Отдельной командой запускаются: all, validate, package, install, uninstall.

Стадии:
{stages}
Опции (перекрывают {CONFIG_FILE_NAME}):
  --config <файл>          конфигурация (по умолчанию {CONFIG_FILE_NAME} рядом с бинарём)
  --lang <ru|en>           язык сообщений (ключ language; по умолчанию en)
  --source-data <каталог>  каталог с Oblivion.esm и BSA (ключ source_data)
  --target-data <каталог>  каталог с Morrowind.esm (ключ target_data)
  --work <каталог>         распаковка, по умолчанию <Oblivion>/_back2mw-output (ключ work_dir)
  --out <каталог>          результат, по умолчанию <Morrowind>/_back2mw-output/output (ключ output_dir)
  --esmtool <файл>         esmtool из OpenMW (ключ esmtool, BACK2MW_ESMTOOL)
  --niftest <файл>         niftest из OpenMW (ключ niftest, BACK2MW_NIFTEST)
  --plugin <файл>          all: плагин Oblivion (можно повторять; ключ selected_plugins).
                           Без него — только STAT из Oblivion.esm
  --only <EDID>            all: только эта запись (можно повторять)
  --worldspace <EDID>      all: перенести worldspace (ячейки, ландшафт, регионы, размещения)
  --offset <dx,dy>         all: смещение worldspace в ячейках Morrowind (ключ offset;
                           по умолчанию — справа от ванили)
  --morrowind-prices       all: поднять цены оружия и брони до Morrowind, если в Oblivion
                           они ниже (ключ morrowind_prices)
  --lua-map                all: с --worldspace — лист карты мира и мод MWSE-Lua, который
                           показывает его в окне карты (ключ lua_map; нужен MWSE)
  --bsa                    package: ещё и BSA на каждый плагин
",
            "back2mw {VERSION} — converts Oblivion content into the Morrowind format

Usage:
  back2mw all [options]
  back2mw <stage> [options]

Available as separate commands: all, validate, package, install, uninstall.

Stages:
{stages}
Options (override {CONFIG_FILE_NAME}):
  --config <file>          configuration (default: {CONFIG_FILE_NAME} next to the binary)
  --lang <ru|en>           message language (key language; default: en)
  --source-data <dir>      folder with Oblivion.esm and the BSAs (key source_data)
  --target-data <dir>      folder with Morrowind.esm (key target_data)
  --work <dir>             unpacked files, default: <Oblivion>/_back2mw-output (key work_dir)
  --out <dir>              result, default: <Morrowind>/_back2mw-output/output (key output_dir)
  --esmtool <file>         esmtool from OpenMW (key esmtool, BACK2MW_ESMTOOL)
  --niftest <file>         niftest from OpenMW (key niftest, BACK2MW_NIFTEST)
  --plugin <file>          all: Oblivion plugin (repeatable; key selected_plugins).
                           Without it — only the STATs of Oblivion.esm
  --only <EDID>            all: only this record (repeatable)
  --worldspace <EDID>      all: convert a worldspace (cells, landscape, regions, references)
  --offset <dx,dy>         all: worldspace offset in Morrowind cells (key offset;
                           default: east of vanilla)
  --morrowind-prices       all: raise weapon and armor prices to Morrowind's where
                           Oblivion's are lower (key morrowind_prices)
  --lua-map                all: with --worldspace — the world map sheet and an MWSE-Lua mod
                           that shows it in the map window (key lua_map; needs MWSE)
  --bsa                    package: also a BSA per plugin
"
        )
    );
}

/// Разобранная командная строка после имени стадии.
#[derive(Default)]
struct Cli {
    config: Option<PathBuf>,
    flags: Overrides,
    only: Vec<String>,
    plugins: Vec<String>,
    bsa: bool,
    worldspace: Option<String>,
    offset: Option<(i32, i32)>,
    morrowind_prices: bool,
    lua_map: bool,
}

fn parse_flags(args: &[String]) -> Result<Cli, String> {
    let mut cli = Cli::default();
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let name = flag.as_str();
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| lang::tr!("у флага {name} нет значения", "flag {name} has no value"))
        };
        match name {
            "--config" => cli.config = Some(PathBuf::from(value()?)),
            // Сам язык уже выбран в `init_language`; здесь — только проверка значения.
            "--lang" => {
                let code = value()?;
                if Lang::from_code(&code).is_none() {
                    return Err(lang::tr!(
                        "--lang: ожидалось ru или en, получено «{code}»",
                        "--lang: expected ru or en, got \"{code}\""
                    ));
                }
            }
            "--source-data" => cli.flags.source_data = Some(PathBuf::from(value()?)),
            "--target-data" => cli.flags.target_data = Some(PathBuf::from(value()?)),
            "--work" => cli.flags.work_dir = Some(PathBuf::from(value()?)),
            "--out" => cli.flags.output_dir = Some(PathBuf::from(value()?)),
            "--esmtool" => cli.flags.esmtool = Some(PathBuf::from(value()?)),
            "--niftest" => cli.flags.niftest = Some(PathBuf::from(value()?)),
            "--only" => cli.only.push(value()?),
            "--plugin" => cli.plugins.push(value()?),
            "--bsa" => cli.bsa = true,
            "--morrowind-prices" => cli.morrowind_prices = true,
            "--lua-map" => cli.lua_map = true,
            "--worldspace" => cli.worldspace = Some(value()?),
            "--offset" => cli.offset = Some(parse_offset(&value()?)?),
            other => {
                return Err(lang::tr!(
                    "неизвестный флаг «{other}»",
                    "unknown flag \"{other}\""
                ));
            }
        }
    }
    Ok(cli)
}

/// `dx,dy` — целые, через запятую.
fn parse_offset(text: &str) -> Result<(i32, i32), String> {
    let bad = || {
        lang::tr!(
            "--offset: ожидалось «dx,dy», получено «{text}»",
            "--offset: expected \"dx,dy\", got \"{text}\""
        )
    };
    let (x, y) = text.split_once(',').ok_or_else(bad)?;
    Ok((
        x.trim().parse().map_err(|_| bad())?,
        y.trim().parse().map_err(|_| bad())?,
    ))
}

/// Значение флага `name`, где бы он ни стоял.
fn flag_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

/// Выбирает язык сообщений до разбора остального: флаг `--lang`, иначе ключ `language` из
/// `config.toml`. Раньше всего остального — чтобы и справка, и ошибки разбора флагов вышли на
/// выбранном языке. Незнакомый код и нечитаемый файл здесь молча пропускаются: о них скажут
/// `parse_flags` и `config::load_with`.
fn init_language(args: &[String]) {
    let from_config = || {
        let path = flag_value(args, "--config").map_or_else(default_config_path, PathBuf::from);
        settings::load(&path).ok()?.language
    };
    let code = flag_value(args, "--lang")
        .map(str::to_owned)
        .or_else(from_config);
    if let Some(chosen) = code.as_deref().and_then(Lang::from_code) {
        lang::set(chosen);
    }
}

/// `config.toml` рядом с бинарём; если каталог бинаря узнать нельзя — в текущем.
fn default_config_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(CONFIG_FILE_NAME)))
        .unwrap_or_else(|| PathBuf::from(CONFIG_FILE_NAME))
}

fn run(stage: &str, rest: &[String]) -> ExitCode {
    let cli = match parse_flags(rest) {
        Ok(c) => c,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let path = cli.config.unwrap_or_else(default_config_path);
    let needs = match stage {
        "all" => Needs::ALL,
        "install" | "uninstall" => Needs::TARGET,
        _ => Needs::NONE,
    };
    let config = match config::load_with(&path, cli.flags, needs) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };

    match stage {
        "all" => {
            let mut log = |line: &str| println!("{line}");
            // Плагины: флаги, иначе выбор из конфигурации (его пишет GUI), иначе срез M1.
            let plugins = if cli.plugins.is_empty() {
                config.selected_plugins.clone()
            } else {
                cli.plugins.clone()
            };
            let mut request = if plugins.is_empty() && cli.worldspace.is_none() {
                Request::oblivion(&cli.only)
            } else {
                Request {
                    plugins,
                    only: cli.only.clone(),
                    validate: true,
                    all_models: true,
                    worldspace: None,
                    offset: None,
                    morrowind_prices: false,
                    lua_map: false,
                }
            };
            request.worldspace.clone_from(&cli.worldspace);
            // Смещение: флаг, иначе ключ `offset` (его пишет и окно) — чтобы повторная сборка
            // ставила мир туда же, куда первая: от этого зависят сохранения.
            request.offset = cli.offset.or(config.offset);
            request.morrowind_prices = cli.morrowind_prices || config.morrowind_prices;
            request.lua_map = cli.lua_map || config.lua_map;
            match pipeline::run(&config, &request, &mut log) {
                Ok(o) => {
                    println!(
                        "{}",
                        lang::tr!(
                            "готово: плагинов {} ({}), STAT {}, WEAP {}, ARMO {}, CLOT {}, предметов {}, объектов мира {} (пропущено {}), LEVI {}, GLOB {}, CREA {}, LEVC {}, меши {}, текстуры {}, иконки {}",
                            "done: plugins {} ({}), STAT {}, WEAP {}, ARMO {}, CLOT {}, items {}, world objects {} (skipped {}), LEVI {}, GLOB {}, CREA {}, LEVC {}, meshes {}, textures {}, icons {}",
                            o.plugins.len(),
                            o.plugins
                                .iter()
                                .map(|p| p.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                            o.statics_written,
                            o.weapons_written,
                            o.armor_written,
                            o.clothing_written,
                            o.items_written,
                            o.world_written,
                            o.statics_skipped.len(),
                            o.leveled_written,
                            o.globals_written,
                            o.creatures_written,
                            o.creature_lists_written,
                            o.meshes_written,
                            o.textures_copied,
                            o.icons_copied
                        )
                    );
                    for p in o.plugins.iter().filter(|p| p.cells > 0) {
                        println!(
                            "{}",
                            lang::tr!(
                                "  {}: внешних ячеек {}, LAND {}, интерьеров {}",
                                "  {}: exterior cells {}, LAND {}, interiors {}",
                                p.name,
                                p.cells,
                                p.lands,
                                p.interiors
                            )
                        );
                    }
                    for p in &o.plugins {
                        if let Some(start) = &p.start {
                            println!("{}", start.line(&p.name));
                        }
                    }
                    print_skipped(&o.statics_skipped);
                    for problem in &o.problems {
                        println!("{}", lang::tr!("замечание: {problem}", "note: {problem}"));
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::from(1)
                }
            }
        }
        "validate" => validate_output(&config),
        "package" => match package::package(&config.output_dir, cli.bsa) {
            Ok(r) => {
                println!(
                    "{}",
                    lang::tr!(
                        "package: {} файлов -> {}",
                        "package: {} files -> {}",
                        r.files,
                        r.loose_zip.display()
                    )
                );
                if let Some((bsas, zip)) = r.bsa {
                    for bsa in &bsas {
                        println!("package: BSA {}", bsa.display());
                    }
                    println!("package: {}", zip.display());
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("package: {e}");
                ExitCode::from(1)
            }
        },
        "install" => match install::install(&config.output_dir, &config.target_data) {
            Ok(r) => {
                println!(
                    "{}",
                    lang::tr!(
                        "install: записано {}, уже было такими же {}, удалено устаревших {}; манифест {}",
                        "install: {} written, {} already identical, {} stale removed; manifest {}",
                        r.written,
                        r.already_present,
                        r.removed_stale,
                        install::MANIFEST_NAME
                    )
                );
                println!(
                    "{}",
                    lang::tr(
                        "Плагины нужно включить вручную (лаунчер или Morrowind.ini), архивы .bsa — перечислить в [Archives].",
                        "Enable the plugins yourself (the launcher or Morrowind.ini) and list the .bsa archives under [Archives]."
                    )
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(1)
            }
        },
        "uninstall" => match install::uninstall(&config.target_data) {
            Ok(r) => {
                println!(
                    "{}",
                    lang::tr!(
                        "uninstall: удалено {}, уже не было {}, каталогов убрано {}",
                        "uninstall: {} removed, {} already gone, {} folders removed",
                        r.removed,
                        r.missing,
                        r.dirs_removed
                    )
                );
                for kept in &r.kept_changed {
                    println!(
                        "{}",
                        lang::tr!(
                            "оставлен (изменён после установки): {kept}",
                            "kept (changed after install): {kept}"
                        )
                    );
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(1)
            }
        },
        _ => {
            eprintln!(
                "{}",
                lang::tr!(
                    "стадия «{stage}» отдельной командой пока не запускается: её выполняет all",
                    "stage \"{stage}\" is not available as a separate command yet: it runs inside all"
                )
            );
            ExitCode::from(3)
        }
    }
}

/// Пропущенные `STAT` по причинам: числа и первый пример, чтобы отчёт не был простынёй.
fn print_skipped(skipped: &[String]) {
    let mut groups: std::collections::BTreeMap<String, (usize, &str)> = Default::default();
    for line in skipped {
        let reason = line.split_once("): ").map_or(line.as_str(), |(_, r)| r);
        let key: String = reason
            .chars()
            .filter(|c| !c.is_ascii_digit())
            .take(70)
            .collect();
        groups.entry(key).or_insert((0, line)).0 += 1;
    }
    let mut list: Vec<_> = groups.into_iter().collect();
    list.sort_by_key(|entry| std::cmp::Reverse(entry.1.0));
    for (reason, (count, example)) in list.iter().take(12) {
        println!(
            "{}",
            lang::tr!(
                "пропущено {count:>5}: {reason}  (например: {example})",
                "skipped {count:>5}: {reason}  (e.g. {example})"
            )
        );
    }
}

/// Отдельная стадия `validate`: проверяет то, что лежит в `output/`.
fn validate_output(config: &config::Config) -> ExitCode {
    let Some(esmtool) = &config.esmtool else {
        eprintln!(
            "{}",
            lang::tr(
                "не задан путь к esmtool: ключ esmtool в config.toml, флаг --esmtool или переменная BACK2MW_ESMTOOL",
                "path to esmtool is not set: the esmtool key in config.toml, the --esmtool flag or the BACK2MW_ESMTOOL variable"
            )
        );
        return ExitCode::from(2);
    };
    let Some(niftest) = &config.niftest else {
        eprintln!(
            "{}",
            lang::tr(
                "не задан путь к niftest: ключ niftest в config.toml, флаг --niftest или переменная BACK2MW_NIFTEST",
                "path to niftest is not set: the niftest key in config.toml, the --niftest flag or the BACK2MW_NIFTEST variable"
            )
        );
        return ExitCode::from(2);
    };
    let check = match validate::check_output(&config.output_dir, esmtool, niftest) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    for (plugin, c) in check.plugins.iter().filter(|(_, c)| c.ok) {
        println!(
            "{}",
            lang::tr!(
                "esmtool: {} цел, STAT {}, WEAP {}",
                "esmtool: {} is intact, STAT {}, WEAP {}",
                plugin.display(),
                c.stat_records,
                c.weapon_records
            )
        );
    }
    println!(
        "{}",
        lang::tr!(
            "niftest: проверено мешей {}, брака {}",
            "niftest: {} meshes checked, {} failed",
            check.meshes,
            check.mesh_failures.len()
        )
    );
    let problems = check.problems();
    if problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        eprintln!("{}", PipelineError::Validation(problems));
        ExitCode::from(1)
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    init_language(&args);

    match args.first().map(String::as_str) {
        None | Some("-h" | "--help" | "help") => {
            usage();
            ExitCode::SUCCESS
        }
        Some("-V" | "--version") => {
            println!("{VERSION}");
            ExitCode::SUCCESS
        }
        Some(stage) if STAGES.iter().any(|(name, _, _)| *name == stage) || stage == "all" => {
            run(stage, &args[1..])
        }
        Some(other) => {
            eprintln!(
                "{}",
                lang::tr!(
                    "неизвестная команда «{other}»",
                    "unknown command \"{other}\""
                )
            );
            eprintln!();
            usage();
            ExitCode::from(2)
        }
    }
}
