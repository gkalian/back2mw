//! Graphical interface: game paths, the list of Oblivion plugins, conversion.
//!
//! All the logic is in the `back2mw` library (finding games, the plugin list, settings, the
//! conversion job, checking, packaging, installing and rolling back); here there is only the
//! window. Long work runs in a separate thread, and the window receives log lines and the result
//! from it over a channel.
//!
//! The window can do everything the console version can: besides "Convert" (build, BSA,
//! installation with one button) — the world offset, building without installing, the distribution
//! zip, separate checking, packaging and installing of a finished mod from the result folder,
//! rolling back an installation, an `EDID` selection, paths to `esmtool`/`niftest`, the work
//! directory and the result folder, another `config.toml` (by a button or
//! `back2mw-gui.exe --config <file>`). The language is Russian or English (the `lang` crate): both
//! the window and the log with library errors.

// In a Windows release build — a windowed application without a console.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use back2mw::config::{self, Needs, Overrides};
use back2mw::install::{self, InstallError, InstallReport, UninstallReport};
use back2mw::job::{Job, JobReport};
use back2mw::package::{self, PackageReport};
use back2mw::pipeline::{VANILLA_EXTERIOR, WORLD_GAP};
use back2mw::plugins::PluginInfo;
use back2mw::settings::{self, Settings};
use back2mw::validate::{self, OutputCheck};
use back2mw::worlds::{self, WorldInfo};
use back2mw::{detect, plugins};
use eframe::egui;
use lang::Lang;

const GREEN: egui::Color32 = egui::Color32::from_rgb(60, 160, 60);
const AMBER: egui::Color32 = egui::Color32::from_rgb(200, 140, 0);
const RED: egui::Color32 = egui::Color32::from_rgb(200, 60, 60);
/// An input field under "Advanced".
const FIELD: [f32; 2] = [460.0, 20.0];

fn main() -> eframe::Result {
    let config_path = config_arg(std::env::args().skip(1)).unwrap_or_else(settings::config_path);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("back2mw — Oblivion → Morrowind")
            .with_inner_size([980.0, 760.0])
            .with_min_inner_size([680.0, 460.0]),
        ..Default::default()
    };
    eframe::run_native(
        "back2mw",
        options,
        Box::new(move |cc| {
            Ok(Box::new(App::new(
                cc.egui_ctx.clone(),
                config_path,
                Lang::default(),
            )))
        }),
    )
}

/// `--config <file>` from the command line, as in the console version.
fn config_arg(mut args: impl Iterator<Item = String>) -> Option<PathBuf> {
    while let Some(arg) = args.next() {
        if arg == "--config" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}

/// File size for the table: "1.5 MB" in English, with Russian units in Russian.
fn format_size(bytes: u64, lang: Lang) -> String {
    let units = lang.tr(["Б", "КБ", "МБ", "ГБ"], ["B", "KB", "MB", "GB"]);
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < units.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", units[0])
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

/// `EDID`s from the "Only records" field: separated by commas, semicolons or spaces.
fn parse_only(text: &str) -> Vec<String> {
    text.split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A path from an input field; empty means not set.
fn field_path(text: &str) -> Option<PathBuf> {
    let t = text.trim();
    (!t.is_empty()).then(|| PathBuf::from(t))
}

/// World label in the list: "SEWorld — Shivering Isles (Oblivion.esm), cities: 5".
fn world_label(w: &WorldInfo, lang: Lang) -> String {
    let mut out = w.editor_id.clone();
    if let Some(name) = &w.name {
        out.push_str(" — ");
        out.push_str(name);
    }
    out.push_str(&format!(" ({})", w.plugin));
    if !w.cities.is_empty() {
        let n = w.cities.len();
        out.push_str(&lang.tr(format!(", городов: {n}"), format!(", cities: {n}")));
    }
    out
}

/// Order in the world drop-down: by `EDID`, case-insensitive (the label starts with it), and by
/// plugin when equal. The order in which background threads deliver them is random from the user's
/// point of view.
fn sort_worlds(worlds: &mut [&WorldInfo]) {
    worlds.sort_by_cached_key(|w| (w.editor_id.to_lowercase(), w.plugin.to_lowercase()));
}

/// A table row.
struct Row {
    info: PluginInfo,
    checked: bool,
}

/// One of the two games in the top rows.
struct GameRow {
    label: &'static str,
    /// What is shown in the input field.
    text: String,
    /// The data folder, if the path is valid.
    data: Option<PathBuf>,
}

impl GameRow {
    fn new(label: &'static str) -> Self {
        Self {
            label,
            text: String::new(),
            data: None,
        }
    }

    fn set(&mut self, root: &Path, data: PathBuf) {
        self.text = detect::game_root(root, &data).display().to_string();
        self.data = Some(data);
    }
}

/// What the background thread is doing — for the label next to the spinner.
#[derive(Clone, Copy)]
enum Task {
    Convert,
    Validate,
    Package,
    Install,
    Uninstall,
}

impl Task {
    fn label(self, lang: Lang) -> &'static str {
        match self {
            Self::Convert => lang.tr("Идёт конвертация…", "Converting…"),
            Self::Validate => lang.tr("Идёт проверка…", "Validating…"),
            Self::Package => lang.tr("Собираю zip…", "Packing zip…"),
            Self::Install => lang.tr("Устанавливаю…", "Installing…"),
            Self::Uninstall => lang.tr("Удаляю установку…", "Uninstalling…"),
        }
    }
}

/// Result of the last action.
enum Outcome {
    Job(Result<Box<JobReport>, String>),
    Validate(Result<OutputCheck, String>),
    Package(Result<PackageReport, String>),
    Install(Result<InstallReport, String>),
    Uninstall(Result<UninstallReport, String>),
    /// The action did not start: paths are not set, the config cannot be read, and so on.
    Error(String),
}

enum Msg {
    Log(String),
    Done(Outcome),
    /// Installing a finished mod ran into other people's files — ask.
    Conflicts(Vec<String>),
}

/// A question on top of the window.
enum Confirm {
    /// Before conversion: `Data Files` has other people's files with our names.
    Job(Box<Job>, Vec<String>),
    /// Installation from the result folder: the same.
    Install(Vec<String>),
    /// Rolling back the installation.
    Uninstall,
}

/// Result of the worldspace search in one plugin.
type WorldScan = (String, Result<Vec<WorldInfo>, String>);

struct App {
    /// The window context: background threads use it to wake up a redraw.
    ctx: egui::Context,
    lang: Lang,
    config_path: PathBuf,
    morrowind: GameRow,
    oblivion: GameRow,
    rows: Vec<Row>,
    list_error: Option<String>,
    pack_bsa: bool,
    /// Bring prices in line with Morrowind.
    morrowind_prices: bool,
    /// World map for MWSE (`--lua-map`).
    lua_map: bool,
    /// Only build into the result folder, do not install.
    build_only: bool,
    /// Also the distribution zip.
    package_zip: bool,
    log: Vec<String>,
    running: Option<(Receiver<Msg>, Task)>,
    result: Option<Outcome>,
    /// Error reading or saving the settings, if there was one.
    save_error: Option<String>,
    /// Games found automatically on this launch.
    auto_detected: bool,
    /// Worldspaces of all plugins in the list (without a parent), as the search goes.
    worlds: Vec<WorldInfo>,
    /// Plugins whose worldspaces could not be read: name and reason.
    world_errors: Vec<String>,
    /// A worldspace search is running in the background.
    world_scan: Option<Receiver<WorldScan>>,
    /// The chosen worldspace (`EDID`); `None` means objects only.
    worldspace: Option<String>,
    /// A custom offset instead of the default (`--offset`).
    custom_offset: bool,
    offset: (i32, i32),
    /// The "Advanced" fields: empty means the default.
    work_text: String,
    output_text: String,
    esmtool_text: String,
    niftest_text: String,
    /// `--only`: not saved — a forgotten selection would silently cut down the next build.
    only_text: String,
    confirm: Option<Confirm>,
}

impl App {
    /// The window from the settings in `config_path`. `lang` is the language if the file does not
    /// record one.
    fn new(ctx: egui::Context, config_path: PathBuf, lang: Lang) -> Self {
        let (saved, load_error) = match settings::load(&config_path) {
            Ok(s) => (s, None),
            Err(e) => (Settings::default(), Some(e)),
        };
        let text = |p: &Option<PathBuf>| p.as_ref().map(|p| p.display().to_string());
        let lang = saved
            .language
            .as_deref()
            .and_then(Lang::from_code)
            .unwrap_or(lang);
        // The log, notes and library errors are in the window's language.
        lang::set(lang);
        let mut app = Self {
            ctx,
            lang,
            config_path,
            morrowind: GameRow::new("Morrowind"),
            oblivion: GameRow::new("Oblivion"),
            rows: Vec::new(),
            list_error: None,
            pack_bsa: saved.pack_bsa,
            morrowind_prices: saved.morrowind_prices,
            lua_map: saved.lua_map,
            build_only: saved.build_only,
            package_zip: saved.package_zip,
            log: Vec::new(),
            running: None,
            result: None,
            save_error: load_error,
            auto_detected: false,
            worlds: Vec::new(),
            world_errors: Vec::new(),
            world_scan: None,
            worldspace: saved.worldspace.clone(),
            custom_offset: saved.offset.is_some(),
            // The default for a world whose south-western cell is (0, 0)
            // (`pipeline::default_offset`).
            offset: saved
                .offset
                .unwrap_or((VANILLA_EXTERIOR.2 + 1 + WORLD_GAP, VANILLA_EXTERIOR.1)),
            work_text: text(&saved.work_dir).unwrap_or_default(),
            output_text: text(&saved.output_dir).unwrap_or_default(),
            esmtool_text: text(&saved.esmtool).unwrap_or_default(),
            niftest_text: text(&saved.niftest).unwrap_or_default(),
            only_text: String::new(),
            confirm: None,
        };

        // Saved paths are checked again: the game may have been removed or moved.
        if let Some(dir) = &saved.target_data
            && let Some(data) = detect::morrowind_data(dir)
        {
            app.morrowind.set(dir, data);
        }
        if let Some(dir) = &saved.source_data
            && let Some(data) = detect::oblivion_data(dir)
        {
            app.oblivion.set(dir, data);
        }
        // Whatever is missing is searched for in the standard Steam and GOG folders.
        if app.morrowind.data.is_none() || app.oblivion.data.is_none() {
            let found = detect::detect();
            if app.morrowind.data.is_none()
                && let Some(root) = found.morrowind
                && let Some(data) = detect::morrowind_data(&root)
            {
                app.morrowind.set(&root, data);
                app.auto_detected = true;
            }
            if app.oblivion.data.is_none()
                && let Some(root) = found.oblivion
                && let Some(data) = detect::oblivion_data(&root)
            {
                app.oblivion.set(&root, data);
                app.auto_detected = true;
            }
        }
        app.reload_plugins(&saved.selected_plugins);
        // A broken file is not overwritten: let the player fix it first.
        if app.save_error.is_none() {
            app.save();
        }
        app
    }

    /// Rereads the list of Oblivion plugins. `selected` is what to tick.
    fn reload_plugins(&mut self, selected: &[String]) {
        self.rows.clear();
        self.list_error = None;
        let Some(data) = &self.oblivion.data else {
            return;
        };
        match plugins::list(data) {
            Ok(list) => {
                self.rows = list
                    .into_iter()
                    .map(|info| Row {
                        checked: info.error.is_none()
                            && selected.iter().any(|s| s.eq_ignore_ascii_case(&info.name)),
                        info,
                    })
                    .collect();
            }
            Err(e) => self.list_error = Some(format!("{}: {e}", data.display())),
        }
        self.scan_worlds();
    }

    /// Searches for worldspaces in all readable plugins of the list — in the background:
    /// `Oblivion.esm` weighs 250 MB. The previous search is abandoned: its thread will see the
    /// closed channel and stop.
    fn scan_worlds(&mut self) {
        self.worlds.clear();
        self.world_errors.clear();
        self.world_scan = None;
        let Some(data) = self.oblivion.data.clone() else {
            return;
        };
        let names: Vec<String> = self
            .rows
            .iter()
            .filter(|r| r.info.error.is_none())
            .map(|r| r.info.name.clone())
            .collect();
        if names.is_empty() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            for name in names {
                let found = worlds::list(&data, &name);
                if tx.send((name, found)).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
        self.world_scan = Some(rx);
    }

    /// Collects the found worldspaces.
    fn poll_worlds(&mut self) {
        let Some(rx) = &self.world_scan else { return };
        loop {
            match rx.try_recv() {
                Ok((_, Ok(list))) => self.worlds.extend(list),
                Ok((name, Err(e))) => self.world_errors.push(format!("{name}: {e}")),
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.world_scan = None;
                    return;
                }
            }
        }
    }

    /// Worldspaces of the ticked plugins — the choice is made among them, alphabetically.
    fn available_worlds(&self) -> Vec<&WorldInfo> {
        let mut out: Vec<&WorldInfo> = self
            .worlds
            .iter()
            .filter(|w| {
                self.rows
                    .iter()
                    .any(|r| r.checked && r.info.name.eq_ignore_ascii_case(&w.plugin))
            })
            .collect();
        sort_worlds(&mut out);
        out
    }

    fn selected_names(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|r| r.checked)
            .map(|r| r.info.name.clone())
            .collect()
    }

    fn save(&mut self) {
        let s = Settings {
            source_data: self.oblivion.data.clone(),
            target_data: self.morrowind.data.clone(),
            selected_plugins: self.selected_names(),
            pack_bsa: self.pack_bsa,
            morrowind_prices: self.morrowind_prices,
            lua_map: self.lua_map,
            worldspace: self.worldspace.clone(),
            offset: self.custom_offset.then_some(self.offset),
            language: Some(self.lang.code().to_owned()),
            build_only: self.build_only,
            package_zip: self.package_zip,
            work_dir: field_path(&self.work_text),
            output_dir: field_path(&self.output_text),
            esmtool: field_path(&self.esmtool_text),
            niftest: field_path(&self.niftest_text),
        };
        self.save_error = settings::save(&self.config_path, &s).err();
    }

    /// The configuration as the console would see it with the flags from the "Advanced" fields: the
    /// `_back2mw-output` defaults in the game folders, the tools — also from
    /// `BACK2MW_ESMTOOL`/`NIFTEST`.
    fn resolved_config(&self) -> Result<config::Config, String> {
        let flags = Overrides {
            work_dir: field_path(&self.work_text),
            output_dir: field_path(&self.output_text),
            esmtool: field_path(&self.esmtool_text),
            niftest: field_path(&self.niftest_text),
            ..Overrides::default()
        };
        config::load_with(&self.config_path, flags, Needs::NONE).map_err(|e| e.to_string())
    }

    fn busy(&self) -> bool {
        self.running.is_some() || self.confirm.is_some()
    }

    /// Refusal before the action starts: the result right away, without a log.
    fn refuse(&mut self, message: String) {
        self.log.clear();
        self.result = Some(Outcome::Error(message));
    }

    fn start(&mut self) {
        let (Some(source), Some(target)) =
            (self.oblivion.data.clone(), self.morrowind.data.clone())
        else {
            return;
        };
        let cfg = match self.resolved_config() {
            Ok(c) => c,
            Err(e) => return self.refuse(e),
        };
        let job = Job {
            source_data: source,
            target_data: target,
            plugins: self.selected_names(),
            pack_bsa: self.pack_bsa,
            morrowind_prices: self.morrowind_prices,
            lua_map: self.worldspace.is_some() && self.lua_map,
            only: parse_only(&self.only_text),
            work_dir: cfg.work_dir,
            output_dir: cfg.output_dir,
            esmtool: cfg.esmtool,
            niftest: cfg.niftest,
            worldspace: self.worldspace.clone(),
            offset: (self.worldspace.is_some() && self.custom_offset).then_some(self.offset),
            install: !self.build_only,
            zip: self.package_zip,
            replace: Vec::new(),
        };
        // Before the long build: whether `Data Files` holds other people's plugins or archives with
        // the same names (copied by hand, from another tool). If so, ask.
        if job.install {
            match install::foreign_files(&job.target_data, &back2mw::job::planned_files(&job)) {
                Ok(foreign) if !foreign.is_empty() => {
                    self.confirm = Some(Confirm::Job(Box::new(job), foreign));
                    return;
                }
                Ok(_) => {}
                Err(e) => return self.refuse(e.to_string()),
            }
        }
        self.launch_job(job);
    }

    fn launch_job(&mut self, job: Job) {
        self.spawn(Task::Convert, move |log| {
            Msg::Done(Outcome::Job(
                back2mw::job::run(&job, log)
                    .map(Box::new)
                    .map_err(|e| e.to_string()),
            ))
        });
    }

    /// Starts work in a background thread. `work` receives the log and returns the result.
    fn spawn(
        &mut self,
        task: Task,
        work: impl FnOnce(&mut dyn FnMut(&str)) -> Msg + Send + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        let repaint = self.ctx.clone();
        std::thread::spawn(move || {
            let log_tx = tx.clone();
            let log_ctx = repaint.clone();
            let mut log = move |line: &str| {
                let _ = log_tx.send(Msg::Log(line.to_owned()));
                log_ctx.request_repaint();
            };
            let _ = tx.send(work(&mut log));
            repaint.request_repaint();
        });
        self.log.clear();
        self.result = None;
        self.running = Some((rx, task));
    }

    /// The result folder and the tools — for actions on a finished mod.
    fn output_dir(&mut self) -> Option<config::Config> {
        match self.resolved_config() {
            Ok(c) => Some(c),
            Err(e) => {
                self.refuse(e);
                None
            }
        }
    }

    /// `validate`: `esmtool` and `niftest` on the result folder.
    fn run_validate(&mut self) {
        let Some(cfg) = self.output_dir() else { return };
        let lang = self.lang;
        let (Some(esmtool), Some(niftest)) = (cfg.esmtool, cfg.niftest) else {
            return self.refuse(
                lang.tr(
                    "Для проверки нужны пути к esmtool и niftest (OpenMW): укажите их в «Дополнительно».",
                    "Validation needs the paths to esmtool and niftest (OpenMW): set them under “Advanced”.",
                )
                .to_owned(),
            );
        };
        let dir = cfg.output_dir;
        self.spawn(Task::Validate, move |log| {
            log(&format!("validate: {}", dir.display()));
            Msg::Done(Outcome::Validate(
                validate::check_output(&dir, &esmtool, &niftest).map_err(|e| e.to_string()),
            ))
        });
    }

    /// `package [--bsa]`: a zip from the result folder.
    fn run_package(&mut self) {
        let Some(cfg) = self.output_dir() else { return };
        let dir = cfg.output_dir;
        let with_bsa = self.pack_bsa;
        self.spawn(Task::Package, move |log| {
            log(&format!("package: {}", dir.display()));
            Msg::Done(Outcome::Package(
                package::package(&dir, with_bsa).map_err(|e| format!("package: {e}")),
            ))
        });
    }

    /// `install`: the finished mod from the result folder into `Data Files`. `replace` holds other
    /// people's files that may be replaced; until the player has been asked, a conflict comes back
    /// as a question.
    fn run_install(&mut self, replace: Vec<String>) {
        let Some(target) = self.morrowind.data.clone() else {
            return;
        };
        let Some(cfg) = self.output_dir() else { return };
        let dir = cfg.output_dir;
        self.spawn(Task::Install, move |log| {
            log(&format!(
                "install: {} -> {}",
                dir.display(),
                target.display()
            ));
            match install::install_replacing(&dir, &target, &replace) {
                Err(InstallError::Conflicts(list)) if replace.is_empty() => Msg::Conflicts(list),
                result => Msg::Done(Outcome::Install(result.map_err(|e| e.to_string()))),
            }
        });
    }

    /// `uninstall`: rollback by the manifest.
    fn run_uninstall(&mut self) {
        let Some(target) = self.morrowind.data.clone() else {
            return;
        };
        self.spawn(Task::Uninstall, move |log| {
            log(&format!("uninstall: {}", target.display()));
            Msg::Done(Outcome::Uninstall(
                install::uninstall(&target).map_err(|e| e.to_string()),
            ))
        });
    }

    /// Collects messages from the background thread.
    fn poll(&mut self) {
        let Some((rx, _)) = &self.running else { return };
        let mut finished = false;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                Msg::Log(line) => self.log.push(line),
                Msg::Done(outcome) => {
                    self.result = Some(outcome);
                    finished = true;
                }
                Msg::Conflicts(list) => {
                    self.confirm = Some(Confirm::Install(list));
                    finished = true;
                }
            }
        }
        if finished {
            self.running = None;
        }
    }

    fn pick_folder(title: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title(title).pick_folder()
    }

    /// A "path to the game" row. Returns `true` if the path changed.
    fn game_row(
        ui: &mut egui::Ui,
        lang: Lang,
        row: &mut GameRow,
        valid: fn(&Path) -> Option<PathBuf>,
        enabled: bool,
    ) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.add_sized(
                [90.0, 20.0],
                egui::Label::new(egui::RichText::new(row.label).strong()),
            );
            let edit = ui.add_enabled(
                enabled,
                egui::TextEdit::singleline(&mut row.text)
                    .desired_width(ui.available_width() - 280.0)
                    .hint_text(lang.tr("папка с игрой", "game folder")),
            );
            if edit.changed() {
                row.data = valid(Path::new(row.text.trim()));
                changed = true;
            }
            let browse = ui
                .add_enabled(enabled, egui::Button::new(lang.tr("Обзор…", "Browse…")))
                .clicked();
            let title = lang.tr(
                format!("Папка {}", row.label),
                format!("{} folder", row.label),
            );
            if browse && let Some(dir) = Self::pick_folder(&title) {
                match valid(&dir) {
                    Some(data) => row.set(&dir, data),
                    None => {
                        row.text = dir.display().to_string();
                        row.data = None;
                    }
                }
                changed = true;
            }
            match &row.data {
                Some(_) => ui.colored_label(GREEN, lang.tr("✔ найдена", "✔ found")),
                None if row.text.trim().is_empty() => ui.colored_label(
                    AMBER,
                    lang.tr(
                        "✖ не найдена — укажите папку",
                        "✖ not found — choose the folder",
                    ),
                ),
                None => ui.colored_label(RED, lang.tr("✖ здесь игры нет", "✖ no game here")),
            };
        });
        changed
    }

    /// An "Advanced" row: label, field, "Browse…" (folder or file). `true` means the field changed.
    fn path_row(
        ui: &mut egui::Ui,
        lang: Lang,
        label: &str,
        text: &mut String,
        hint: &str,
        pick_file: bool,
        enabled: bool,
    ) -> bool {
        ui.label(label);
        // In a `Grid` the field does not know its width (`desired_width` has no effect there) — set
        // it explicitly.
        let mut changed = ui
            .add_enabled_ui(enabled, |ui| {
                ui.add_sized(FIELD, egui::TextEdit::singleline(text).hint_text(hint))
            })
            .inner
            .changed();
        if ui
            .add_enabled(enabled, egui::Button::new(lang.tr("Обзор…", "Browse…")))
            .clicked()
        {
            let dialog = rfd::FileDialog::new().set_title(label);
            let picked = if pick_file {
                dialog.pick_file()
            } else {
                dialog.pick_folder()
            };
            if let Some(p) = picked {
                *text = p.display().to_string();
                changed = true;
            }
        }
        ui.end_row();
        changed
    }

    /// A question window. `Some(true)` — yes, `Some(false)` — cancel, `None` — not answered yet.
    fn confirm_window(ctx: &egui::Context, lang: Lang, confirm: &Confirm) -> Option<bool> {
        let mut answer = None;
        let (title, yes) = match confirm {
            Confirm::Uninstall => (
                lang.tr("Удалить установку", "Uninstall"),
                lang.tr("Удалить", "Uninstall"),
            ),
            _ => (
                lang.tr("Файлы уже есть", "Files already exist"),
                lang.tr("Перезаписать", "Overwrite"),
            ),
        };
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                match confirm {
                    Confirm::Job(_, foreign) | Confirm::Install(foreign) => {
                        ui.label(lang.tr(
                            "В Data Files Morrowind уже лежат файлы с теми же именами, и поставил их не back2mw:",
                            "Morrowind's Data Files already has files with the same names, not installed by back2mw:",
                        ));
                        for f in foreign {
                            ui.monospace(format!("  {f}"));
                        }
                        ui.label(lang.tr(
                            "Перезаписать их? Прежнее содержимое пропадёт, а откат установки потом удалит и эти файлы.",
                            "Overwrite them? Their current content will be lost, and uninstalling will later remove these files too.",
                        ));
                    }
                    Confirm::Uninstall => {
                        ui.label(lang.tr(
                            "Удалить из Data Files всё, что поставил back2mw (по манифесту back2mw-install.tsv)?",
                            "Remove everything back2mw installed into Data Files (per the back2mw-install.tsv manifest)?",
                        ));
                        ui.label(lang.tr(
                            "Файлы, изменённые после установки, останутся на месте.",
                            "Files changed after installation are kept.",
                        ));
                    }
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(yes).clicked() {
                        answer = Some(true);
                    }
                    if ui.button(lang.tr("Отмена", "Cancel")).clicked() {
                        answer = Some(false);
                    }
                });
            });
        answer
    }

    /// Result of the last action, above the log.
    fn show_outcome(ui: &mut egui::Ui, lang: Lang, outcome: &Outcome) {
        let error = |ui: &mut egui::Ui, e: &str| {
            ui.colored_label(RED, lang.tr(format!("Ошибка: {e}"), format!("Error: {e}")));
        };
        let enable_hint = |ui: &mut egui::Ui| {
            ui.label(lang.tr(
                "Плагины включите вручную (лаунчер или Morrowind.ini), архивы .bsa — перечислите в [Archives].",
                "Enable the plugins yourself (launcher or Morrowind.ini) and list the .bsa archives under [Archives].",
            ));
        };
        match outcome {
            Outcome::Error(e) => error(ui, e),
            Outcome::Job(Err(e))
            | Outcome::Validate(Err(e))
            | Outcome::Package(Err(e))
            | Outcome::Install(Err(e))
            | Outcome::Uninstall(Err(e)) => error(ui, e),
            Outcome::Job(Ok(r)) => {
                let bsa = if r.packed_into_bsa {
                    lang.tr(
                        ", файлы собраны в архивы плагинов",
                        ", files packed into per-plugin archives",
                    )
                } else {
                    ""
                };
                ui.colored_label(
                    GREEN,
                    lang.tr(
                        format!(
                            "Готово: плагинов {}, записей STAT {}, оружия {}, брони {}, одежды {}, предметов {} (пропущено {}), мешей {}, текстур {}{bsa}.",
                            r.plugins.len(), r.statics_written, r.weapons_written, r.armor_written,
                            r.clothing_written, r.items_written, r.statics_skipped, r.meshes, r.textures,
                        ),
                        format!(
                            "Done: plugins {}, STAT records {}, weapons {}, armor {}, clothing {}, items {} (skipped {}), meshes {}, textures {}{bsa}.",
                            r.plugins.len(), r.statics_written, r.weapons_written, r.armor_written,
                            r.clothing_written, r.items_written, r.statics_skipped, r.meshes, r.textures,
                        ),
                    ),
                );
                if r.cells > 0 {
                    ui.label(lang.tr(
                        format!(
                            "Мир: внешних ячеек {}, интерьеров {}.",
                            r.cells, r.interiors
                        ),
                        format!(
                            "World: exterior cells {}, interiors {}.",
                            r.cells, r.interiors
                        ),
                    ));
                }
                match (&r.install, &r.built_into) {
                    (Some(i), _) => {
                        ui.label(lang.tr(
                            format!(
                                "Установлено в Data Files: записано {}, уже было такими же {}, удалено устаревших {}.",
                                i.written, i.already_present, i.removed_stale
                            ),
                            format!(
                                "Installed into Data Files: written {}, already identical {}, stale removed {}.",
                                i.written, i.already_present, i.removed_stale
                            ),
                        ));
                        let plugins = r.plugins.join(", ");
                        ui.label(lang.tr(
                            format!("Включите в лаунчере Morrowind: {plugins}."),
                            format!("Enable in the Morrowind launcher: {plugins}."),
                        ));
                        if !r.bsas.is_empty() {
                            let bsas = r.bsas.join(", ");
                            ui.label(lang.tr(
                                format!(
                                    "Добавьте архивы в Morrowind.ini, раздел [Archives]: {bsas}."
                                ),
                                format!(
                                    "Add the archives to Morrowind.ini, section [Archives]: {bsas}."
                                ),
                            ));
                        }
                    }
                    (None, Some(dir)) => {
                        let dir = dir.display();
                        ui.label(lang.tr(
                            format!("Мод собран в {dir}; в игру не устанавливался."),
                            format!(
                                "The mod was built into {dir}; nothing was installed into the game."
                            ),
                        ));
                    }
                    (None, None) => {}
                }
                if let Some(p) = &r.package {
                    Self::show_package(ui, lang, p);
                }
                if !r.validated {
                    ui.weak(lang.tr(
                        "Проверка esmtool/niftest не выполнялась: пути к ним не заданы.",
                        "esmtool/niftest validation was skipped: their paths are not set.",
                    ));
                }
                if !r.notes.is_empty() {
                    let n = r.notes.len();
                    ui.collapsing(
                        lang.tr(format!("Замечания ({n})"), format!("Notes ({n})")),
                        |ui| {
                            for note in &r.notes {
                                ui.label(note);
                            }
                        },
                    );
                }
            }
            Outcome::Validate(Ok(c)) => {
                let problems = c.problems();
                let plugins = c.plugins.len();
                if problems.is_empty() {
                    ui.colored_label(
                        GREEN,
                        lang.tr(
                            format!("Проверка пройдена: плагинов {plugins}, мешей {}.", c.meshes),
                            format!("Validation passed: plugins {plugins}, meshes {}.", c.meshes),
                        ),
                    );
                } else {
                    ui.colored_label(
                        RED,
                        lang.tr(
                            format!(
                                "Найден брак ({}); плагинов {plugins}, мешей {}:",
                                problems.len(),
                                c.meshes
                            ),
                            format!(
                                "Problems found ({}); plugins {plugins}, meshes {}:",
                                problems.len(),
                                c.meshes
                            ),
                        ),
                    );
                    for p in &problems {
                        ui.label(p);
                    }
                }
            }
            Outcome::Package(Ok(p)) => Self::show_package(ui, lang, p),
            Outcome::Install(Ok(i)) => {
                ui.colored_label(
                    GREEN,
                    lang.tr(
                        format!(
                            "Установлено: записано {}, уже было такими же {}, удалено устаревших {}; манифест {}.",
                            i.written, i.already_present, i.removed_stale, install::MANIFEST_NAME
                        ),
                        format!(
                            "Installed: written {}, already identical {}, stale removed {}; manifest {}.",
                            i.written, i.already_present, i.removed_stale, install::MANIFEST_NAME
                        ),
                    ),
                );
                enable_hint(ui);
            }
            Outcome::Uninstall(Ok(u)) => {
                ui.colored_label(
                    GREEN,
                    lang.tr(
                        format!(
                            "Установка удалена: файлов {}, уже не было {}, каталогов убрано {}.",
                            u.removed, u.missing, u.dirs_removed
                        ),
                        format!(
                            "Uninstalled: files removed {}, already missing {}, folders removed {}.",
                            u.removed, u.missing, u.dirs_removed
                        ),
                    ),
                );
                for kept in &u.kept_changed {
                    ui.label(lang.tr(
                        format!("Оставлен (изменён после установки): {kept}"),
                        format!("Kept (changed after installation): {kept}"),
                    ));
                }
            }
        }
    }

    fn show_package(ui: &mut egui::Ui, lang: Lang, p: &PackageReport) {
        let zip = p.loose_zip.display();
        ui.colored_label(
            GREEN,
            lang.tr(
                format!("Zip для раздачи: {zip} (файлов {}).", p.files),
                format!("Distribution zip: {zip} ({} files).", p.files),
            ),
        );
        if let Some((bsas, zip)) = &p.bsa {
            let zip = zip.display();
            ui.label(lang.tr(
                format!("С архивами BSA: {zip} (архивов {}).", bsas.len()),
                format!("With BSA archives: {zip} ({} archives).", bsas.len()),
            ));
        }
    }
}

impl eframe::App for App {
    #[expect(
        clippy::too_many_lines,
        reason = "window layout from top to bottom; the parts are already split into methods"
    )]
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        self.poll_worlds();
        let lang = self.lang;

        // The question is on top of the window; while it is open, the rest is inactive.
        if let Some(confirm) = &self.confirm {
            match (
                Self::confirm_window(ctx, lang, confirm),
                self.confirm.take(),
            ) {
                (Some(true), Some(Confirm::Job(mut job, foreign))) => {
                    job.replace = foreign;
                    self.launch_job(*job);
                }
                (Some(true), Some(Confirm::Install(foreign))) => self.run_install(foreign),
                (Some(true), Some(Confirm::Uninstall)) => self.run_uninstall(),
                (Some(false), _) => {}
                (None, pending) => self.confirm = pending,
                (Some(true), None) => {}
            }
        }
        let busy = self.busy();
        let mut changed = false;
        let mut reload = false;
        let mut switch_config = None;
        // The chosen world disappeared from the ticked files (unticked, file missing) — reset it.
        // While the search is running, leave it alone: the list is still incomplete.
        if self.world_scan.is_none()
            && let Some(w) = &self.worldspace
            && !self
                .available_worlds()
                .iter()
                .any(|a| a.editor_id.eq_ignore_ascii_case(w))
        {
            self.worldspace = None;
            changed = true;
        }

        // The top bar: the language switch in the right corner.
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong("back2mw");
                // The egui font has no "→" — it shows a box; the arrow exists only in the window
                // title.
                ui.weak("Oblivion - Morrowind");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    for (code, value) in [("EN", Lang::En), ("RU", Lang::Ru)] {
                        let hint = value.tr("Русский язык", "English language");
                        if ui
                            .selectable_label(self.lang == value, code)
                            .on_hover_text(hint)
                            .clicked()
                            && self.lang != value
                        {
                            self.lang = value;
                            // Log lines already shown stay in the previous language.
                            lang::set(value);
                            changed = true;
                        }
                    }
                });
            });
        });

        // The bottom row: how to build, the "Convert" button, rolling back the installation.
        egui::TopBottomPanel::bottom("actions").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    changed |= ui
                        .checkbox(
                            &mut self.pack_bsa,
                            lang.tr(
                                "Собрать файлы в BSA (свой архив на каждый плагин)",
                                "Pack files into BSA (one archive per plugin)",
                            ),
                        )
                        .changed();
                    changed |= ui
                        .checkbox(
                            &mut self.morrowind_prices,
                            lang.tr(
                                "Подтянуть цены к Morrowind",
                                "Raise prices to Morrowind levels",
                            ),
                        )
                        .on_hover_text(lang.tr(
                            "Оружие и броня, которые в Oblivion дешевле своих аналогов в Morrowind (стекло, эбонит, даэдрика), подорожают до цен Morrowind. Дешевле не станет ничего.",
                            "Weapons and armor that are cheaper in Oblivion than their Morrowind counterparts (glass, ebony, daedric) get Morrowind prices. Nothing gets cheaper.",
                        ))
                        .changed();
                    changed |= ui
                        .checkbox(
                            &mut self.build_only,
                            lang.tr(
                                "Только собрать в папку результата, не устанавливать",
                                "Only build into the output folder, don't install",
                            ),
                        )
                        .changed();
                    changed |= ui
                        .checkbox(
                            &mut self.package_zip,
                            lang.tr("Ещё zip-архив для раздачи", "Also make a distribution zip"),
                        )
                        .changed();
                });
            });
            ui.horizontal(|ui| {
                let can = !busy
                    && self.morrowind.data.is_some()
                    && self.oblivion.data.is_some()
                    && self.rows.iter().any(|r| r.checked);
                if ui
                    .add_enabled(
                        can,
                        egui::Button::new(
                            egui::RichText::new(lang.tr("Конвертировать", "Convert")).strong(),
                        )
                        .min_size(egui::vec2(160.0, 30.0)),
                    )
                    .clicked()
                {
                    self.start();
                }
                if self.confirm.is_some() {
                    ui.label(lang.tr("Ответьте на вопрос", "Please answer the question"));
                } else if let Some((_, task)) = &self.running {
                    ui.spinner();
                    ui.label(task.label(lang));
                } else if self.morrowind.data.is_none() || self.oblivion.data.is_none() {
                    ui.label(lang.tr(
                        "Укажите папки обеих игр",
                        "Choose the folders of both games",
                    ));
                } else if !self.rows.iter().any(|r| r.checked) {
                    ui.label(lang.tr("Отметьте хотя бы один файл", "Check at least one file"));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let installed = self
                        .morrowind
                        .data
                        .as_ref()
                        .is_some_and(|d| d.join(install::MANIFEST_NAME).is_file());
                    if ui
                        .add_enabled(
                            !busy && installed,
                            egui::Button::new(lang.tr("Удалить из игры…", "Uninstall…")),
                        )
                        .on_hover_text(lang.tr(
                            "Откатить установку back2mw в Data Files по манифесту",
                            "Roll back the back2mw installation in Data Files using its manifest",
                        ))
                        .on_disabled_hover_text(lang.tr(
                            "В Data Files нет установки back2mw",
                            "No back2mw installation in Data Files",
                        ))
                        .clicked()
                    {
                        self.confirm = Some(Confirm::Uninstall);
                    }
                });
            });
            ui.add_space(6.0);
        });

        // The log and the result.
        if busy || !self.log.is_empty() || self.result.is_some() {
            egui::TopBottomPanel::bottom("log")
                .resizable(true)
                .default_height(170.0)
                .show(ctx, |ui| {
                    if let Some(outcome) = &self.result {
                        ui.add_space(4.0);
                        Self::show_outcome(ui, lang, outcome);
                    }
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for line in &self.log {
                                ui.monospace(line);
                            }
                        });
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(4.0);
            if Self::game_row(ui, lang, &mut self.morrowind, detect::morrowind_data, !busy) {
                changed = true;
            }
            if Self::game_row(ui, lang, &mut self.oblivion, detect::oblivion_data, !busy) {
                changed = true;
                reload = true;
            }
            if self.auto_detected {
                ui.weak(lang.tr(
                    "Игры найдены автоматически в стандартных папках Steam/GOG; путь можно изменить.",
                    "The games were found automatically in the standard Steam/GOG folders; you can change the paths.",
                ));
            }
            if let Some(e) = &self.save_error {
                ui.colored_label(RED, lang.tr(format!("Настройки: {e}"), format!("Settings: {e}")));
            }
            ui.separator();

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(lang.tr("Плагины Oblivion", "Oblivion plugins")).strong());
                if ui.add_enabled(!busy, egui::Button::new(lang.tr("Отметить все", "Check all"))).clicked() {
                    for r in self.rows.iter_mut().filter(|r| r.info.error.is_none()) {
                        r.checked = true;
                    }
                    changed = true;
                }
                if ui.add_enabled(!busy, egui::Button::new(lang.tr("Снять все", "Uncheck all"))).clicked() {
                    for r in &mut self.rows {
                        r.checked = false;
                    }
                    changed = true;
                }
                if ui.add_enabled(!busy, egui::Button::new(lang.tr("Обновить список", "Refresh list"))).clicked() {
                    reload = true;
                }
            });
            ui.weak(lang.tr(
                "Из плагинов берутся модели (меши, текстуры, коллизия) статики, предметов, оружия, брони и т. п. — как статические объекты Morrowind.",
                "Models (meshes, textures, collision) of statics, items, weapons, armor etc. are taken from the plugins as Morrowind static objects.",
            ));

            // World choice: only from the ticked files.
            let options: Vec<(String, String)> = self
                .available_worlds()
                .iter()
                .map(|w| (w.editor_id.clone(), world_label(w, lang)))
                .collect();
            let no_world = lang.tr("Не переносить — только объекты", "Don't convert — objects only");
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(lang.tr("Мир (worldspace)", "World (worldspace)")).strong());
                let shown = match &self.worldspace {
                    None => no_world.to_owned(),
                    Some(w) => options
                        .iter()
                        .find(|(e, _)| e.eq_ignore_ascii_case(w))
                        .map_or_else(|| w.clone(), |(_, label)| label.clone()),
                };
                ui.add_enabled_ui(!busy, |ui| {
                    // `Oblivion.esm` has 54 worlds: by default about 9 rows are visible, we make it
                    // about 20.
                    let row = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
                    egui::ComboBox::from_id_salt("worldspace")
                        .width(460.0)
                        .height(row * 20.0 + 8.0)
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            changed |= ui
                                .selectable_value(&mut self.worldspace, None, no_world)
                                .changed();
                            for (edid, label) in &options {
                                changed |= ui
                                    .selectable_value(
                                        &mut self.worldspace,
                                        Some(edid.clone()),
                                        label,
                                    )
                                    .changed();
                            }
                        });
                });
                if self.world_scan.is_some() {
                    ui.spinner();
                    ui.weak(lang.tr("ищу worldspace в файлах…", "looking for worldspaces…"));
                } else if options.is_empty() && self.rows.iter().any(|r| r.checked) {
                    ui.weak(lang.tr(
                        "в отмеченных файлах worldspace нет",
                        "the checked files have no worldspace",
                    ));
                }
            });
            if let Some(w) = self
                .worldspace
                .as_ref()
                .and_then(|w| self.worlds.iter().find(|a| a.editor_id.eq_ignore_ascii_case(w)))
            {
                let n = w.cities.len();
                let cities = match n {
                    0 => String::new(),
                    _ => lang.tr(
                        format!(", города ({n}) — в тех же ячейках"),
                        format!(", cities ({n}) in the same cells"),
                    ),
                };
                ui.weak(lang.tr(
                    format!(
                        "Будет собран {}.esp: ландшафт, регионы{cities}, интерьеры с дверями. Из {} возьмутся только объекты этого мира; остальные отмеченные файлы конвертируются как обычно.",
                        w.editor_id, w.plugin
                    ),
                    format!(
                        "{}.esp will be built: landscape, regions{cities}, interiors behind doors. Only this world's objects are taken from {}; the other checked files are converted as usual.",
                        w.editor_id, w.plugin
                    ),
                ));
                // Offset (`--offset`).
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!busy, |ui| {
                        changed |= ui
                            .checkbox(
                                &mut self.custom_offset,
                                lang.tr("Своё смещение, ячеек Morrowind:", "Custom offset, Morrowind cells:"),
                            )
                            .changed();
                        ui.add_enabled_ui(self.custom_offset, |ui| {
                            ui.label("dx");
                            changed |= ui.add(egui::DragValue::new(&mut self.offset.0)).changed();
                            ui.label("dy");
                            changed |= ui.add(egui::DragValue::new(&mut self.offset.1)).changed();
                        });
                    });
                });
                ui.weak(if self.custom_offset {
                    lang.tr(
                        "Ячейка мира (x, y) станет ячейкой (x + dx, y + dy). Меняйте смещение только до начала игры: иначе мир в сохранениях окажется не там.",
                        "World cell (x, y) becomes cell (x + dx, y + dy). Change the offset only before you start playing, or the world in your saves ends up elsewhere.",
                    )
                } else {
                    lang.tr(
                        "По умолчанию западный край мира — в 8 ячейках восточнее Вварденфелла, южный — вровень с его южным краем.",
                        "By default the world's west edge is 8 cells east of Vvardenfell and its south edge is level with Vvardenfell's.",
                    )
                });
                // World map for MWSE (`--lua-map`).
                ui.add_enabled_ui(!busy, |ui| {
                    changed |= ui
                        .checkbox(
                            &mut self.lua_map,
                            lang.tr("Карта мира для MWSE", "World map for MWSE"),
                        )
                        .on_hover_text(lang.tr(
                            "Карта мира Morrowind кончается раньше, чем начинается перенесённый мир. С этой галочкой рядом с плагином ложатся рисованная карта мира из Oblivion и мод MWSE-Lua, который показывает её в окне карты, пока вы в этом мире. Нужен MWSE; без него файлы просто лежат.",
                            "Morrowind's world map ends before the converted world begins. With this option the drawn world map from Oblivion and an MWSE-Lua mod are placed next to the plugin; the mod shows that map in the map window while you are in this world. Needs MWSE; without it the files are simply unused.",
                        ))
                        .changed();
                });
            }
            if !self.world_errors.is_empty() {
                let errors = self.world_errors.join("; ");
                ui.colored_label(
                    AMBER,
                    lang.tr(
                        format!("Не удалось прочитать миры: {errors}"),
                        format!("Could not read worlds: {errors}"),
                    ),
                );
            }

            // Everything else the console can do.
            egui::CollapsingHeader::new(lang.tr("Дополнительно", "Advanced"))
                .id_salt("advanced")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(lang.tr("Файл настроек:", "Settings file:"));
                        if ui
                            .add_enabled(!busy, egui::Button::new(lang.tr("Другой…", "Other…")))
                            .clicked()
                            && let Some(p) = rfd::FileDialog::new()
                                .set_title(lang.tr("Файл настроек", "Settings file"))
                                .add_filter("TOML", &["toml"])
                                .pick_file()
                        {
                            switch_config = Some(p);
                        }
                        // A long path is truncated; the full one is in the tooltip.
                        let shown = self.config_path.display().to_string();
                        ui.add(egui::Label::new(egui::RichText::new(&shown).monospace()).truncate())
                            .on_hover_text(shown);
                    });
                    let base = self
                        .config_path
                        .parent()
                        .unwrap_or_else(|| Path::new(""))
                        .to_owned();
                    let default_in = |name: &str| {
                        let game = if name == "work" {
                            self.oblivion.data.as_deref().and_then(config::default_work_dir)
                        } else {
                            self.morrowind.data.as_deref().and_then(config::default_output_dir)
                        };
                        let shown = game.unwrap_or_else(|| base.join(name));
                        lang.tr(
                            format!("по умолчанию {}", shown.display()),
                            format!("default {}", shown.display()),
                        )
                    };
                    let tool_hint = |var: &str| match std::env::var_os(var) {
                        Some(v) if !v.is_empty() => {
                            let v = PathBuf::from(v);
                            lang.tr(
                                format!("из {var}: {}", v.display()),
                                format!("from {var}: {}", v.display()),
                            )
                        }
                        _ => lang
                            .tr("не задан — проверка пропускается", "not set — validation is skipped")
                            .to_owned(),
                    };
                    egui::Grid::new("advanced_grid")
                        .num_columns(3)
                        .spacing([10.0, 6.0])
                        .show(ui, |ui| {
                            changed |= Self::path_row(
                                ui,
                                lang,
                                lang.tr("Рабочая папка", "Work folder"),
                                &mut self.work_text,
                                &default_in("work"),
                                false,
                                !busy,
                            );
                            changed |= Self::path_row(
                                ui,
                                lang,
                                lang.tr("Папка результата", "Output folder"),
                                &mut self.output_text,
                                &default_in("output"),
                                false,
                                !busy,
                            );
                            changed |= Self::path_row(
                                ui,
                                lang,
                                "esmtool",
                                &mut self.esmtool_text,
                                &tool_hint("BACK2MW_ESMTOOL"),
                                true,
                                !busy,
                            );
                            changed |= Self::path_row(
                                ui,
                                lang,
                                "niftest",
                                &mut self.niftest_text,
                                &tool_hint("BACK2MW_NIFTEST"),
                                true,
                                !busy,
                            );
                            ui.label(lang.tr("Только записи (EDID)", "Only records (EDID)"));
                            ui.add_enabled_ui(!busy, |ui| {
                                ui.add_sized(
                                    FIELD,
                                    egui::TextEdit::singleline(&mut self.only_text).hint_text(lang.tr(
                                        "через запятую; пусто — все записи",
                                        "comma-separated; empty — all records",
                                    )),
                                );
                            });
                            ui.end_row();
                        });
                    ui.add_space(4.0);
                    ui.label(lang.tr(
                        "Готовый мод в папке результата (собранный с галочкой «Только собрать» или консольным all):",
                        "The finished mod in the output folder (built with “Only build” or the console's all):",
                    ));
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!busy, egui::Button::new(lang.tr("Проверить", "Validate")))
                            .on_hover_text(lang.tr(
                                "esmtool по плагинам и niftest по мешам",
                                "esmtool on the plugins and niftest on the meshes",
                            ))
                            .clicked()
                        {
                            self.run_validate();
                        }
                        if ui
                            .add_enabled(!busy, egui::Button::new(lang.tr("Упаковать в zip", "Pack into zip")))
                            .on_hover_text(lang.tr(
                                "zip в <папка результата>\\package; с галочкой BSA — ещё архивы плагинов",
                                "zip into <output folder>\\package; with the BSA box — also per-plugin archives",
                            ))
                            .clicked()
                        {
                            self.run_package();
                        }
                        if ui
                            .add_enabled(
                                !busy && self.morrowind.data.is_some(),
                                egui::Button::new(lang.tr("Установить в Data Files", "Install into Data Files")),
                            )
                            .clicked()
                        {
                            self.run_install(Vec::new());
                        }
                    });
                });
            ui.separator();

            if let Some(e) = &self.list_error {
                ui.colored_label(RED, e);
            } else if self.oblivion.data.is_none() {
                ui.label(lang.tr(
                    "Укажите папку Oblivion — здесь появится список плагинов.",
                    "Choose the Oblivion folder — the plugin list will appear here.",
                ));
            } else if self.rows.is_empty() {
                ui.label(lang.tr(
                    "В папке Data нет файлов .esm и .esp.",
                    "The Data folder has no .esm or .esp files.",
                ));
            }

            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                egui::Grid::new("plugins")
                    .striped(true)
                    .num_columns(6)
                    .spacing([16.0, 6.0])
                    .show(ui, |ui| {
                        let headers = lang.tr(
                            ["", "Файл", "Тип", "Источник", "Размер", "Мастера"],
                            ["", "File", "Type", "Source", "Size", "Masters"],
                        );
                        for h in headers {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for row in &mut self.rows {
                            let ok = row.info.error.is_none();
                            let cb = ui.add_enabled(ok && !busy, egui::Checkbox::without_text(&mut row.checked));
                            changed |= cb.changed();
                            let name = ui.label(&row.info.name);
                            if let Some(e) = &row.info.error {
                                name.on_hover_text(lang.tr(
                                    format!("Заголовок не читается: {e}"),
                                    format!("Unreadable header: {e}"),
                                ));
                            }
                            ui.label(row.info.kind.to_string());
                            ui.label(if row.info.official {
                                lang.tr("Оригинал", "Official")
                            } else {
                                lang.tr("Мод", "Mod")
                            });
                            ui.label(format_size(row.info.size, lang));
                            ui.label(row.info.masters.join(", "));
                            ui.end_row();
                        }
                    });
            });
        });

        if reload {
            let keep = self.selected_names();
            self.reload_plugins(&keep);
        }
        if changed {
            self.save();
        }
        if let Some(path) = switch_config {
            *self = Self::new(ctx.clone(), path, self.lang);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Lang, config_arg, format_size, parse_only, sort_worlds};
    use back2mw::worlds::WorldInfo;
    use std::path::PathBuf;

    fn world(plugin: &str, editor_id: &str) -> WorldInfo {
        WorldInfo {
            plugin: plugin.to_owned(),
            editor_id: editor_id.to_owned(),
            name: None,
            cities: Vec::new(),
        }
    }

    #[test]
    fn worlds_are_listed_alphabetically_ignoring_case() {
        let list = [
            world("Oblivion.esm", "Tamriel"),
            world("Mod.esp", "arena"),
            world("Oblivion.esm", "SEWorld"),
            world("B.esp", "Same"),
            world("a.esp", "Same"),
        ];
        let mut refs: Vec<&WorldInfo> = list.iter().collect();
        sort_worlds(&mut refs);
        let got: Vec<_> = refs
            .iter()
            .map(|w| (w.editor_id.as_str(), w.plugin.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("arena", "Mod.esp"),
                ("Same", "a.esp"),
                ("Same", "B.esp"),
                ("SEWorld", "Oblivion.esm"),
                ("Tamriel", "Oblivion.esm"),
            ]
        );
    }

    #[test]
    fn sizes_are_shown_in_readable_units() {
        assert_eq!(format_size(0, Lang::Ru), "0 Б");
        assert_eq!(format_size(1023, Lang::Ru), "1023 Б");
        assert_eq!(format_size(1536, Lang::Ru), "1.5 КБ");
        assert_eq!(format_size(5 * 1024 * 1024, Lang::Ru), "5.0 МБ");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024, Lang::Ru), "3.0 ГБ");
        assert_eq!(format_size(1023, Lang::En), "1023 B");
        assert_eq!(format_size(1536, Lang::En), "1.5 KB");
    }

    #[test]
    fn only_accepts_commas_semicolons_and_spaces() {
        assert_eq!(
            parse_only(" ChairA, TableB;Lamp  Rug ,,"),
            ["ChairA", "TableB", "Lamp", "Rug"]
        );
        assert!(parse_only("  ").is_empty());
    }

    #[test]
    fn the_config_flag_is_read_like_the_console_one() {
        let args = |a: &[&str]| a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            config_arg(args(&["--config", "my.toml"]).into_iter()),
            Some(PathBuf::from("my.toml"))
        );
        assert_eq!(config_arg(args(&["--config"]).into_iter()), None);
        assert_eq!(config_arg(args(&[]).into_iter()), None);
    }
}
