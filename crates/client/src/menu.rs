//! The title screen and the pause menu, in the HUD's constructivist style
//! (DESIGN §8): a big display-type title over a red slanted rule, a column
//! of menu entries with red tabs, and sub-pages (new game, load, settings,
//! quit) in panels beside it. Esc steps back a page; from the pause menu's
//! first page it resumes.

use std::path::PathBuf;

use egui::{Align2, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use crate::hud::{
    ALARM, AMBER, CREAM, CREAM_DIM, INK, Panel, RED, RED_DARK, RULE, VFD, VFD_DIM, credits,
    display, glow_label, mono, panel_filled, section, title_card,
};
use crate::save::{self, Entry, GameId, Kind};
use crate::settings::{SENSITIVITY_RANGE, Settings, UI_SCALE_RANGE};
use crate::world::World;

/// How long a toast stays up, seconds.
pub const TOAST_SECONDS: f32 = 3.0;
const COLUMN_X: f32 = 72.0;
const COLUMN_W: f32 = 420.0;
const PAGE_W: f32 = 600.0;
const HANDBOOK_W: f32 = 720.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Main,
    NewGame,
    Load,
    Settings,
    /// The pilot's handbook: how to start, the controls, making a living.
    Handbook,
    /// "Leave this game?" — from the pause menu.
    ConfirmQuit,
}

#[derive(Default)]
pub struct Menu {
    page: Page,
    /// Why the last thing tried failed (a save that wouldn't load).
    pub error: Option<String>,
    /// The new-game page's seed field.
    seed_text: String,
    /// The menu entry under the mouse or picked with the arrow keys.
    cursor: Option<usize>,
    /// The handbook's open chapter.
    chapter: usize,
}

/// Keyboard navigation of the menu column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Enter,
}

/// An entry in the menu column.
struct Item {
    title: &'static str,
    note: String,
    enabled: bool,
    act: Act,
}

enum Act {
    Do(MenuAction),
    /// Open or close a page beside the column.
    Toggle(Page),
}

impl Item {
    fn action(title: &'static str, note: &str, action: MenuAction) -> Self {
        Self {
            title,
            note: note.to_owned(),
            enabled: true,
            act: Act::Do(action),
        }
    }

    fn page(title: &'static str, note: &str, page: Page) -> Self {
        Self {
            title,
            note: note.to_owned(),
            enabled: true,
            act: Act::Toggle(page),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuAction {
    Resume,
    Pause,
    NewGame(u64),
    Load(PathBuf),
    Save,
    QuitToTitle {
        save: bool,
    },
    Quit,
    /// A setting changed: apply and store them.
    SettingsChanged,
}

/// The game being played, for the pause menu.
pub struct GameInfo<'a> {
    pub world: &'a World,
    pub id: &'a GameId,
    pub played_s: f64,
    pub last_save: Option<u64>,
    pub last_checkpoint: Option<u64>,
}

pub struct MenuInput<'a> {
    /// `None` on the title screen.
    pub game: Option<GameInfo<'a>>,
    pub saves: &'a [Entry],
    pub settings: &'a mut Settings,
    pub time: f32,
    /// Arrow keys and Enter pressed this frame.
    pub nav: &'a [Nav],
}

impl Menu {
    pub fn open(&mut self, page: Page) {
        self.page = page;
        self.error = None;
        if page == Page::NewGame && self.seed_text.is_empty() {
            self.seed_text = random_seed().to_string();
        }
    }

    /// Which handbook chapter is open.
    pub fn set_chapter(&mut self, chapter: usize) {
        self.chapter = chapter.min(crate::handbook::CHAPTERS.len() - 1);
    }

    /// Esc: back a page, or out of the pause menu.
    pub fn back(&mut self, in_game: bool) -> Option<MenuAction> {
        if self.page != Page::Main {
            self.page = Page::Main;
            self.error = None;
            None
        } else if in_game {
            Some(MenuAction::Resume)
        } else {
            None
        }
    }

    pub fn draw(&mut self, ctx: &egui::Context, input: MenuInput) -> Vec<MenuAction> {
        let mut actions = Vec::new();
        // Typing in a field: the keys are the field's.
        let mut nav: Vec<Nav> = if ctx.egui_wants_keyboard_input() {
            Vec::new()
        } else {
            input.nav.to_vec()
        };
        // On the new-game page, Enter starts the game.
        if self.page == Page::NewGame && nav.contains(&Nav::Enter) {
            nav.retain(|n| *n != Nav::Enter);
            actions.push(MenuAction::NewGame(seed_from_text(&self.seed_text)));
        }
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::background());
        if input.game.is_some() {
            // Dim the frozen game behind the pause menu.
            painter.rect_filled(screen, 0.0, INK.gamma_multiply(0.55));
        }
        // Shade the left side so the type reads over anything.
        let shade = Rect::from_min_max(
            screen.min,
            Pos2::new(screen.left() + screen.width() * 0.62, screen.bottom()),
        );
        hgradient(
            &painter,
            shade,
            INK.gamma_multiply(0.85),
            Color32::TRANSPARENT,
        );

        // On the quit page, Enter is its red button: save and quit.
        if self.page == Page::ConfirmQuit
            && nav.contains(&Nav::Enter)
            && let Some(g) = &input.game
        {
            nav.retain(|n| *n != Nav::Enter);
            if save::can_save(g.world).is_ok() {
                actions.push(MenuAction::QuitToTitle { save: true });
            }
        }

        let top = screen.top() + (screen.height() * 0.09).max(40.0);
        let masthead_bottom = match &input.game {
            None => masthead(&painter, Pos2::new(COLUMN_X, top), input.time),
            Some(g) => pause_masthead(&painter, Pos2::new(COLUMN_X, top), g),
        };

        let column_top = masthead_bottom + 34.0;
        egui::Area::new(egui::Id::unique("menu-column"))
            .fixed_pos(Pos2::new(COLUMN_X, column_top))
            .show(ctx, |ui| {
                ui.set_width(COLUMN_W);
                ui.spacing_mut().item_spacing.y = 8.0;
                let items = match &input.game {
                    None => self.title_entries(input.saves),
                    Some(g) => self.pause_entries(g),
                };
                self.column(ui, items, &nav, &mut actions);
                if let Some(error) = &self.error {
                    ui.add_space(8.0);
                    ui.label(RichText::new(error).color(ALARM));
                }
            });

        let page_pos = [COLUMN_X + COLUMN_W + 36.0, column_top];
        match self.page {
            Page::Main => {}
            Page::NewGame => self.new_game_page(ctx, page_pos, &mut actions),
            Page::Load => load_page(ctx, page_pos, &input, &mut actions),
            Page::Settings => settings_page(ctx, page_pos, input.settings, &mut actions),
            Page::Handbook => {
                let max_h = (screen.bottom() - page_pos[1] - 150.0).max(240.0);
                let chapter = &mut self.chapter;
                let invert_pitch = input.settings.invert_pitch;
                panel_filled(
                    ctx,
                    Panel {
                        id: "handbook",
                        title: "Pilot's handbook",
                        anchor: Align2::LEFT_TOP,
                        offset: page_pos,
                        width: HANDBOOK_W,
                        collapsible: None,
                    },
                    crate::hud::PAGE_FILL,
                    |ui| crate::handbook::draw(ui, chapter, max_h, invert_pitch),
                );
            }
            Page::ConfirmQuit => {
                if let Some(g) = &input.game {
                    self.confirm_quit_page(ctx, page_pos, g, &mut actions);
                }
            }
        }

        // Footer.
        let footer = match (self.page, input.game.is_some()) {
            (Page::Main, true) => "ESC resume",
            (Page::Main, false) => "",
            _ => "ESC back",
        };
        painter.text(
            Pos2::new(COLUMN_X, screen.bottom() - 18.0),
            Align2::LEFT_BOTTOM,
            format!("retrox {}    {footer}", env!("CARGO_PKG_VERSION")),
            mono(11.0),
            CREAM_DIM,
        );
        actions
    }

    fn title_entries(&self, saves: &[Entry]) -> Vec<Item> {
        let latest = saves.iter().find(|e| e.header.is_ok());
        let continue_note = match latest.and_then(|e| e.header.as_ref().ok()) {
            Some(h) => format!("{} · {} · {}", h.system, h.date, save::ago(h.saved)),
            None => "no saved game yet".to_owned(),
        };
        let load_note = match count_games(saves) {
            0 => "nothing saved yet".to_owned(),
            1 => "1 game saved".to_owned(),
            n => format!("{n} games saved"),
        };
        vec![
            Item {
                title: "Continue",
                note: continue_note,
                enabled: latest.is_some(),
                act: Act::Do(
                    latest.map_or(MenuAction::Resume, |e| MenuAction::Load(e.path.clone())),
                ),
            },
            Item::page("New game", "a fresh galaxy", Page::NewGame),
            Item {
                enabled: !saves.is_empty(),
                ..Item::page("Load", &load_note, Page::Load)
            },
            Item::page("Handbook", "how to fly and get started", Page::Handbook),
            Item::page("Settings", "controls, display, sound", Page::Settings),
            Item::action("Quit", "back to the desktop", MenuAction::Quit),
        ]
    }

    fn pause_entries(&self, g: &GameInfo) -> Vec<Item> {
        let (can_save, save_note) = match save::can_save(g.world) {
            Ok(()) => (
                true,
                match g.last_save {
                    Some(t) => format!("last saved {}", save::ago(t)),
                    None => "not saved yet".to_owned(),
                },
            ),
            Err(why) => (false, why.to_owned()),
        };
        vec![
            Item::action("Resume", "back to the game", MenuAction::Resume),
            Item {
                enabled: can_save,
                ..Item::action("Save", &save_note, MenuAction::Save)
            },
            Item::page("Load", "a save or checkpoint", Page::Load),
            Item::page(
                "Handbook",
                "controls and how to get started",
                Page::Handbook,
            ),
            Item::page("Settings", "controls, display, sound", Page::Settings),
            Item::page("Quit to title", "leave this game", Page::ConfirmQuit),
        ]
    }

    /// The menu column, by mouse or keyboard.
    fn column(
        &mut self,
        ui: &mut egui::Ui,
        items: Vec<Item>,
        nav: &[Nav],
        actions: &mut Vec<MenuAction>,
    ) {
        let enabled: Vec<usize> = (0..items.len()).filter(|&i| items[i].enabled).collect();
        let mut chosen = None;
        for key in nav {
            let at = self
                .cursor
                .and_then(|c| enabled.iter().position(|&i| i == c));
            match key {
                Nav::Down => {
                    self.cursor = Some(match at {
                        Some(p) => enabled[(p + 1) % enabled.len()],
                        None => enabled[0],
                    });
                }
                Nav::Up => {
                    self.cursor = Some(match at {
                        Some(p) => enabled[(p + enabled.len() - 1) % enabled.len()],
                        None => enabled[enabled.len() - 1],
                    });
                }
                Nav::Enter => chosen = at.map(|p| enabled[p]),
            }
        }
        for (i, item) in items.iter().enumerate() {
            let open = matches!(item.act, Act::Toggle(p) if p == self.page);
            let focused = self.cursor == Some(i);
            let (clicked, hovered) = entry(ui, item.title, &item.note, item.enabled, open, focused);
            if hovered {
                self.cursor = Some(i);
            }
            if clicked {
                chosen = Some(i);
            }
        }
        if let Some(item) = chosen.and_then(|i| items.into_iter().nth(i)) {
            match item.act {
                Act::Do(action) => {
                    self.error = None;
                    actions.push(action);
                }
                Act::Toggle(page) => self.toggle(page),
            }
        }
    }

    /// Open a page, or close it if it's already open.
    fn toggle(&mut self, page: Page) {
        if self.page == page {
            self.page = Page::Main;
        } else {
            self.open(page);
        }
    }

    fn new_game_page(&mut self, ctx: &egui::Context, pos: [f32; 2], actions: &mut Vec<MenuAction>) {
        page_panel(ctx, "new-game", "New game", pos, |ui| {
            ui.label(
                RichText::new(
                    "A galaxy of 5000 star systems, generated from a seed. The same seed \
                     always makes the same galaxy.",
                )
                .color(CREAM_DIM),
            );
            ui.add_space(10.0);
            section(ui, "Seed");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let edit = egui::TextEdit::singleline(&mut self.seed_text)
                    .font(mono(22.0))
                    .text_color(crate::hud::VFD)
                    .desired_width(300.0)
                    .margin(Vec2::new(10.0, 6.0));
                let response = ui.add(edit);
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    actions.push(MenuAction::NewGame(seed_from_text(&self.seed_text)));
                }
                let reroll = crate::widgets::push(
                    ui,
                    "ROLL",
                    display(22.0),
                    crate::widgets::Look::Plain,
                    true,
                    Vec2::new(0.0, 38.0),
                );
                if reroll.on_hover_text("a random seed").clicked() {
                    self.seed_text = random_seed().to_string();
                }
            });
            ui.add_space(2.0);
            let note = if self.seed_text.trim().parse::<u64>().is_ok() {
                "any number or word".to_owned()
            } else if self.seed_text.trim().is_empty() {
                "empty: a random seed".to_owned()
            } else {
                format!("the word makes seed {}", seed_from_text(&self.seed_text))
            };
            ui.label(RichText::new(note).size(12.0).color(CREAM_DIM));
            ui.add_space(10.0);
            ui.label(
                RichText::new(
                    "New here? The HANDBOOK explains how to get started, and in the game                      H shows the controls.",
                )
                .size(12.0)
                .color(AMBER),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if primary(ui, "Start").clicked() {
                    actions.push(MenuAction::NewGame(seed_from_text(&self.seed_text)));
                }
            });
        });
    }

    fn confirm_quit_page(
        &mut self,
        ctx: &egui::Context,
        pos: [f32; 2],
        g: &GameInfo,
        actions: &mut Vec<MenuAction>,
    ) {
        page_panel(ctx, "confirm-quit", "Quit to title", pos, |ui| {
            ui.label(
                RichText::new("LEAVE THIS GAME?")
                    .font(display(34.0))
                    .color(CREAM),
            );
            ui.add_space(4.0);
            let when = |t: Option<u64>| t.map_or("never".to_owned(), save::ago);
            kv(ui, "last save", &when(g.last_save));
            kv(ui, "last checkpoint", &when(g.last_checkpoint));
            ui.add_space(6.0);
            ui.label(
                RichText::new("Anything since then is lost unless you save.").color(CREAM_DIM),
            );
            ui.add_space(14.0);
            let can_save = save::can_save(g.world);
            ui.horizontal(|ui| {
                let saved = ui
                    .add_enabled_ui(can_save.is_ok(), |ui| primary(ui, "Save and quit"))
                    .inner;
                if saved.clicked() {
                    actions.push(MenuAction::QuitToTitle { save: true });
                }
                if secondary(ui, "Quit without saving").clicked() {
                    actions.push(MenuAction::QuitToTitle { save: false });
                }
            });
            if let Err(why) = can_save {
                ui.add_space(4.0);
                ui.label(RichText::new(why).size(12.0).color(AMBER));
            }
        });
    }
}

fn load_page(ctx: &egui::Context, pos: [f32; 2], input: &MenuInput, actions: &mut Vec<MenuAction>) {
    let max_h = (ctx.content_rect().height() - pos[1] - 90.0).max(200.0);
    page_panel(ctx, "load", "Load", pos, |ui| {
        if input.game.is_some() {
            ui.label(
                RichText::new("Loading leaves this game; anything unsaved is lost.").color(AMBER),
            );
            ui.add_space(6.0);
        }
        if input.saves.is_empty() {
            ui.label(RichText::new("Nothing saved yet.").color(CREAM_DIM));
            return;
        }
        // Games in order of their newest save.
        let mut games: Vec<&str> = Vec::new();
        for e in input.saves {
            let id = game_of(e);
            if !games.contains(&id) {
                games.push(id);
            }
        }
        egui::ScrollArea::vertical()
            .max_height(max_h)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                for game in games {
                    let entries: Vec<&Entry> =
                        input.saves.iter().filter(|e| game_of(e) == game).collect();
                    let heading = entries
                        .iter()
                        .find_map(|e| e.header.as_ref().ok())
                        .map_or_else(
                            || "Unreadable".to_owned(),
                            |h| {
                                let current =
                                    input.game.as_ref().is_some_and(|g| g.id.id == h.game.id);
                                let tail = if current {
                                    "this game".to_owned()
                                } else {
                                    format!("started {}", save::ago(h.game.started))
                                };
                                format!("Seed {} · {tail}", h.game.seed)
                            },
                        );
                    ui.add_space(4.0);
                    section(ui, &heading);
                    for e in entries {
                        if save_row(ui, e) {
                            actions.push(MenuAction::Load(e.path.clone()));
                        }
                    }
                }
            });
    });
}

fn game_of(e: &Entry) -> &str {
    e.path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("")
}

fn count_games(saves: &[Entry]) -> usize {
    let mut games: Vec<&str> = saves.iter().map(game_of).collect();
    games.sort_unstable();
    games.dedup();
    games.len()
}

/// One save on the load page: a clickable row. Returns whether clicked.
fn save_row(ui: &mut egui::Ui, e: &Entry) -> bool {
    let width = ui.available_width();
    let header = e.header.as_ref();
    let sense = if header.is_ok() {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 62.0), sense);
    let painter = ui.painter();
    let hot = header.is_ok() && response.hovered();
    painter.rect_filled(
        rect,
        0.0,
        if hot {
            RED_DARK
        } else {
            crate::widgets::CARD_FILL
        },
    );
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, if hot { CREAM } else { RULE }),
        egui::StrokeKind::Inside,
    );
    match header {
        Ok(h) => {
            // The kind, as a tag down the left.
            let (label, color) = match h.kind {
                Kind::Save => ("SAVE", RED),
                Kind::Checkpoint => ("AUTO", VFD),
            };
            let tag = Rect::from_min_size(rect.min, Vec2::new(54.0, rect.height()));
            let fill = if h.kind == Kind::Save { color } else { VFD_DIM };
            painter.rect_filled(tag, 0.0, fill);
            painter.text(
                tag.center(),
                Align2::CENTER_CENTER,
                label,
                display(20.0),
                if h.kind == Kind::Save { CREAM } else { color },
            );
            let x = tag.right() + 14.0;
            let title = painter.text(
                Pos2::new(x, rect.top() + 7.0),
                Align2::LEFT_TOP,
                h.system.to_uppercase(),
                display(26.0),
                CREAM,
            );
            painter.text(
                Pos2::new(title.right() + 10.0, title.bottom() - 4.0),
                Align2::LEFT_BOTTOM,
                &h.place,
                mono(12.0),
                if hot { CREAM } else { CREAM_DIM },
            );
            painter.text(
                Pos2::new(x, rect.bottom() - 7.0),
                Align2::LEFT_BOTTOM,
                format!(
                    "{}   {}   {}   {} played",
                    h.date,
                    credits(h.credits),
                    h.ship,
                    save::played(h.played_s)
                ),
                mono(12.0),
                if hot { CREAM } else { CREAM_DIM },
            );
            painter.text(
                Pos2::new(rect.right() - 12.0, rect.top() + 9.0),
                Align2::RIGHT_TOP,
                save::ago(h.saved),
                mono(12.0),
                AMBER,
            );
        }
        Err(why) => {
            painter.text(
                Pos2::new(rect.left() + 14.0, rect.top() + 8.0),
                Align2::LEFT_TOP,
                "CAN'T LOAD",
                display(22.0),
                RULE,
            );
            let galley = painter.layout(
                why.clone(),
                mono(11.0),
                ALARM.gamma_multiply(0.8),
                rect.width() - 28.0,
            );
            painter.galley(
                Pos2::new(rect.left() + 14.0, rect.bottom() - 6.0 - galley.size().y),
                galley,
                ALARM,
            );
        }
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn settings_page(
    ctx: &egui::Context,
    pos: [f32; 2],
    settings: &mut Settings,
    actions: &mut Vec<MenuAction>,
) {
    page_panel(ctx, "settings", "Settings", pos, |ui| {
        let mut changed = false;
        section(ui, "Controls");
        setting_row(ui, "Mouse sensitivity", |ui| {
            let (moved, _) = vfd_slider(
                ui,
                &mut settings.mouse_sensitivity,
                SENSITIVITY_RANGE,
                0.05,
                times,
                Some(1.0),
            );
            changed |= moved;
        });
        setting_row(ui, "Invert pitch", |ui| {
            changed |= switch(ui, &mut settings.invert_pitch);
        });
        ui.label(
            RichText::new("inverted: push the mouse forward, or press ↑, to pull the nose up")
                .size(11.0)
                .color(CREAM_DIM),
        );
        ui.add_space(10.0);
        section(ui, "Help");
        setting_row(ui, "First-steps notes", |ui| {
            changed |= switch(ui, &mut settings.first_steps);
        });
        ui.label(
            RichText::new("a checklist with hints in flight, until you've made your first credits")
                .size(11.0)
                .color(CREAM_DIM),
        );
        ui.add_space(10.0);
        section(ui, "Display");
        setting_row(ui, "Fullscreen", |ui| {
            changed |= switch(ui, &mut settings.fullscreen);
        });
        setting_row(ui, "Interface size", |ui| {
            let mut scale = f64::from(settings.ui_scale);
            let range = f64::from(*UI_SCALE_RANGE.start())..=f64::from(*UI_SCALE_RANGE.end());
            let (_, released) = vfd_slider(ui, &mut scale, range, 0.05, times, Some(1.0));
            settings.ui_scale = scale as f32;
            // Resize once the slider is let go, so it stays under the mouse.
            changed |= released;
        });
        ui.add_space(10.0);
        section(ui, "Sound");
        for (label, volume) in [
            ("Volume", &mut settings.master_volume),
            ("Music", &mut settings.music_volume),
            ("Effects", &mut settings.effects_volume),
        ] {
            setting_row(ui, label, |ui| {
                let mut v = f64::from(*volume);
                // Heard while dragging (the game reads the setting every
                // frame); written to disk when let go.
                let (_, released) = vfd_slider(ui, &mut v, 0.0..=1.0, 0.05, percent, None);
                *volume = v as f32;
                changed |= released;
            });
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if secondary(ui, "Defaults").clicked() {
                *settings = Settings::default();
                changed = true;
            }
            ui.add_space(8.0);
            ui.label(
                RichText::new("changes are kept between runs")
                    .size(11.0)
                    .color(CREAM_DIM),
            );
        });
        if changed {
            actions.push(MenuAction::SettingsChanged);
        }
    });
}

fn setting_row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(32.0);
        ui.label(RichText::new(label).color(CREAM));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
    });
}

fn times(v: f64) -> String {
    format!("{v:.2}×")
}

fn percent(v: f64) -> String {
    format!("{:>4.0}%", v * 100.0)
}

/// A segmented VFD bar to drag, with its value beside it (as `readout`
/// writes it) and a tick at `mark`. Returns whether the value moved, and
/// whether it was just let go (or clicked).
fn vfd_slider(
    ui: &mut egui::Ui,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    step: f64,
    readout: fn(f64) -> String,
    mark: Option<f64>,
) -> (bool, bool) {
    let (lo, hi) = (*range.start(), *range.end());
    // Laid out right to left: the readout, then the bar.
    glow_label(ui, &readout(*value), mono(15.0), VFD);
    ui.add_space(6.0);
    let n = 30;
    let width = 240.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::click_and_drag());
    let before = *value;
    if let Some(p) = response.interact_pointer_pos() {
        let t = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
        let v = lo + t * (hi - lo);
        *value = ((v / step).round() * step).clamp(lo, hi);
    }
    let painter = ui.painter();
    let hot = response.hovered() || response.dragged();
    let gap = 2.0;
    let seg = (width - gap * (n - 1) as f32) / n as f32;
    let lit = (((*value - lo) / (hi - lo)) * n as f64).round() as usize;
    let bar = Rect::from_min_size(
        Pos2::new(rect.left(), rect.center().y - 6.0),
        Vec2::new(width, 12.0),
    );
    for i in 0..n {
        let r = Rect::from_min_size(
            Pos2::new(bar.left() + i as f32 * (seg + gap), bar.top()),
            Vec2::new(seg, bar.height()),
        );
        if i < lit.max(usize::from(mark.is_some())) {
            painter.rect_filled(r.expand(1.0), 0.0, VFD.gamma_multiply(0.18));
            painter.rect_filled(r, 0.0, VFD);
        } else {
            painter.rect_filled(r, 0.0, VFD_DIM);
        }
    }
    // Where 1.0× (as tuned) sits.
    if let Some(mark) = mark {
        let at = rect.left() + ((mark - lo) / (hi - lo)) as f32 * width;
        painter.line_segment(
            [
                Pos2::new(at, bar.bottom() + 2.0),
                Pos2::new(at, rect.bottom()),
            ],
            Stroke::new(1.0, if hot { CREAM } else { RULE }),
        );
    }
    let response = response.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    let released = response.drag_stopped() || response.clicked();
    (*value != before, released)
}

/// An ON/OFF switch as a pair of indicator lamps. Returns whether toggled.
fn switch(ui: &mut egui::Ui, value: &mut bool) -> bool {
    let size = Vec2::new(116.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    let hot = response.hovered();
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, if hot { CREAM } else { RULE }),
        egui::StrokeKind::Inside,
    );
    let half = rect.width() / 2.0;
    for (i, (label, lit)) in [("OFF", !*value), ("ON", *value)].into_iter().enumerate() {
        let r = Rect::from_min_size(
            Pos2::new(rect.left() + half * i as f32, rect.top()),
            Vec2::new(half, rect.height()),
        )
        .shrink(3.0);
        let color = if i == 1 { VFD } else { CREAM_DIM };
        if lit {
            painter.rect_filled(r.expand(1.5), 0.0, color.gamma_multiply(0.2));
            painter.rect_filled(r, 0.0, color);
        }
        painter.text(
            r.center(),
            Align2::CENTER_CENTER,
            label,
            mono(13.0),
            if lit { INK } else { color.gamma_multiply(0.5) },
        );
    }
    if response.clicked() {
        *value = !*value;
        true
    } else {
        false
    }
}

/// "label ........ value" in the HUD's readout style.
fn kv(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).color(CREAM_DIM));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            glow_label(ui, value, mono(15.0), VFD);
        });
    });
}

/// A sub-page: a HUD panel beside the menu column.
fn page_panel(
    ctx: &egui::Context,
    id: &'static str,
    title: &str,
    pos: [f32; 2],
    add: impl FnOnce(&mut egui::Ui),
) {
    panel_filled(
        ctx,
        Panel {
            id,
            title,
            anchor: Align2::LEFT_TOP,
            offset: pos,
            width: PAGE_W,
            collapsible: None,
        },
        crate::hud::PAGE_FILL,
        add,
    );
}

/// The big red button.
fn primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    crate::widgets::push(
        ui,
        &text.to_uppercase(),
        display(26.0),
        crate::widgets::Look::Primary,
        true,
        Vec2::new(170.0, 40.0),
    )
}

fn secondary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    crate::widgets::push(
        ui,
        &text.to_uppercase(),
        display(22.0),
        crate::widgets::Look::Plain,
        true,
        Vec2::new(0.0, 40.0),
    )
}

/// A menu entry: title in display type, a note, a red tab; lit while its
/// page is open or it has the keyboard's focus. Returns whether it was
/// clicked and whether the mouse is over it.
fn entry(
    ui: &mut egui::Ui,
    title: &str,
    note: &str,
    enabled: bool,
    selected: bool,
    focused: bool,
) -> (bool, bool) {
    let width = ui.available_width();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 60.0), sense);
    let painter = ui.painter();
    let hovered = response.hovered();
    let hot = enabled && (hovered || focused);
    let (fill, text, sub) = match (enabled, hot || selected) {
        (false, _) => (
            Color32::from_rgba_premultiplied(12, 10, 16, 150),
            RULE,
            RULE,
        ),
        (true, true) => (RED_DARK, CREAM, CREAM),
        (true, false) => (crate::widgets::CARD_FILL, CREAM, CREAM_DIM),
    };
    painter.rect_filled(rect, 0.0, fill);
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, if hot || selected { CREAM } else { RULE }),
        egui::StrokeKind::Inside,
    );
    let tab_w = if selected { 10.0 } else { 6.0 };
    painter.rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(tab_w, rect.height())),
        0.0,
        if enabled { RED } else { RULE },
    );
    painter.text(
        Pos2::new(rect.left() + 22.0, rect.top() + 6.0),
        Align2::LEFT_TOP,
        title.to_uppercase(),
        display(34.0),
        text,
    );
    let note = crate::widgets::truncate(note, 52);
    painter.text(
        Pos2::new(rect.left() + 22.0, rect.bottom() - 6.0),
        Align2::LEFT_BOTTOM,
        note,
        mono(12.0),
        sub,
    );
    if enabled {
        let c = Pos2::new(rect.right() - 22.0, rect.center().y);
        painter.add(egui::Shape::convex_polygon(
            vec![
                c + Vec2::new(-6.0, -9.0),
                c + Vec2::new(6.0, 0.0),
                c + Vec2::new(-6.0, 9.0),
            ],
            if hot || selected { CREAM } else { CREAM_DIM },
            Stroke::NONE,
        ));
    }
    if enabled {
        let clicked = response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked();
        (clicked, hovered)
    } else {
        (false, false)
    }
}

/// The title: RETROX in display type over a slanted red rule, a tagline,
/// and slashes. Returns the bottom of what it drew.
fn masthead(painter: &egui::Painter, at: Pos2, time: f32) -> f32 {
    let title = painter.text(at, Align2::LEFT_TOP, "RETROX", display(168.0), CREAM);
    // Bebas sits high in its line box: hang the rule just under the glyphs.
    let rule_y = title.bottom() - 22.0;
    let rule_l = 0.0;
    let rule_r = title.right() + 120.0;
    let h = 16.0;
    let slant = 12.0;
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(rule_l, rule_y),
            Pos2::new(rule_r + slant, rule_y),
            Pos2::new(rule_r, rule_y + h),
            Pos2::new(rule_l, rule_y + h),
        ],
        RED,
        Stroke::NONE,
    ));
    // Slashes trailing off the rule.
    for k in 0..4 {
        let x = rule_r + 26.0 + k as f32 * 16.0;
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(x, rule_y + h),
                Pos2::new(x + 7.0, rule_y + h),
                Pos2::new(x + 7.0 + slant, rule_y),
                Pos2::new(x + slant, rule_y),
            ],
            RED.gamma_multiply(0.8 - k as f32 * 0.18),
            Stroke::NONE,
        ));
    }
    let tagline = painter.text(
        Pos2::new(at.x + 4.0, rule_y + h + 14.0),
        Align2::LEFT_TOP,
        "A  SMALL  SHIP  IN  A  LIVING  GALAXY",
        mono(16.0),
        CREAM_DIM,
    );
    // A blinking cursor after the tagline, like a teleprinter waiting.
    if (time * 1.6).fract() < 0.6 {
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(tagline.right() + 8.0, tagline.top() + 2.0),
                Vec2::new(9.0, tagline.height() - 4.0),
            ),
            0.0,
            VFD,
        );
    }
    tagline.bottom()
}

/// The pause menu's head: PAUSED, then where and when.
fn pause_masthead(painter: &egui::Painter, at: Pos2, g: &GameInfo) -> f32 {
    let title = painter.text(at, Align2::LEFT_TOP, "PAUSED", display(110.0), CREAM);
    let rule_y = title.bottom() - 14.0;
    let h = 12.0;
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(0.0, rule_y),
            Pos2::new(title.right() + 70.0, rule_y),
            Pos2::new(title.right() + 61.0, rule_y + h),
            Pos2::new(0.0, rule_y + h),
        ],
        RED,
        Stroke::NONE,
    ));
    let w = g.world;
    let system = &w.galaxy.systems[w.system].name;
    let line1 = painter.text(
        Pos2::new(at.x + 2.0, rule_y + h + 14.0),
        Align2::LEFT_TOP,
        format!("{} · {}", system.to_uppercase(), save::place_text(w)),
        mono(16.0),
        CREAM,
    );
    let date = w.date().to_string();
    let galley = painter.layout_no_wrap(date, mono(20.0), VFD);
    let pos = Pos2::new(at.x + 2.0, line1.bottom() + 8.0);
    let date_w = galley.size().x;
    let date_h = galley.size().y;
    for (dx, dy) in [(-1.5, 0.0), (1.5, 0.0), (0.0, -1.5), (0.0, 1.5)] {
        painter.galley_with_override_text_color(
            pos + Vec2::new(dx, dy),
            galley.clone(),
            VFD.gamma_multiply(0.12),
        );
    }
    painter.galley(pos, galley, VFD);
    painter.text(
        Pos2::new(pos.x + date_w + 18.0, pos.y),
        Align2::LEFT_TOP,
        credits(w.player.credits),
        mono(20.0),
        AMBER,
    );
    let played = painter.text(
        Pos2::new(at.x + 2.0, pos.y + date_h + 6.0),
        Align2::LEFT_TOP,
        format!("seed {} · {} played", g.id.seed, save::played(g.played_s)),
        mono(12.0),
        CREAM_DIM,
    );
    played.bottom()
}

/// The loading card while a galaxy is generated or a save read.
pub fn loading(ctx: &egui::Context, title: &str, subtitle: &str, time: f32) {
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("loading"),
    ));
    painter.rect_filled(screen, 0.0, INK.gamma_multiply(0.8));
    let centre = screen.center() - Vec2::new(0.0, 30.0);
    title_card(&painter, centre, title, subtitle, CREAM);
    // A segmented bar with a lit window sweeping along it.
    let n = 40;
    let width = 420.0;
    let seg = (width - (n - 1) as f32 * 2.0) / n as f32;
    let left = centre.x - width / 2.0;
    let y = centre.y + 70.0;
    let head = (time * 22.0) % (n as f32 + 8.0);
    for i in 0..n {
        let r = Rect::from_min_size(
            Pos2::new(left + i as f32 * (seg + 2.0), y),
            Vec2::new(seg, 12.0),
        );
        let d = head - i as f32;
        if (0.0..8.0).contains(&d) {
            let k = 1.0 - d / 8.0;
            painter.rect_filled(r.expand(1.0), 0.0, VFD.gamma_multiply(0.18 * k));
            painter.rect_filled(r, 0.0, VFD.gamma_multiply(0.3 + 0.7 * k));
        } else {
            painter.rect_filled(r, 0.0, VFD_DIM);
        }
    }
}

/// A short note in the bottom right, fading out. Drawn after everything
/// else, so it can stack above whatever panel holds that corner (#177).
pub fn toast(ctx: &egui::Context, text: &str, ok: bool, age: f32) {
    let alpha = ((TOAST_SECONDS - age) / 0.5).clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return;
    }
    let color = if ok { VFD } else { ALARM };
    let id = egui::Id::unique("toast");
    let offset = toast_offset(ctx, id);
    egui::Area::new(id)
        .order(egui::Order::Foreground)
        .anchor(Align2::RIGHT_BOTTOM, offset)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(crate::hud::PANEL.gamma_multiply(alpha))
                .stroke(Stroke::new(1.0, color.gamma_multiply(0.6 * alpha)))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                        ui.painter()
                            .rect_filled(r, 0.0, color.gamma_multiply(alpha));
                        ui.label(
                            RichText::new(text.to_uppercase())
                                .font(mono(14.0))
                                .color(color.gamma_multiply(alpha)),
                        );
                    });
                });
        });
}

/// Where the toast sits, from the bottom-right corner: its own spot when
/// that corner is free, else just above the panels stacked there (FIRST
/// STEPS beside the station menu, HYPERLANES in flight), right edges lined
/// up.
fn toast_offset(ctx: &egui::Context, toast: egui::Id) -> [f32; 2] {
    /// How near the edges a panel's corner must come to hold the corner.
    const CORNER: f32 = 64.0;
    /// Room above a panel for its corner brackets.
    const GAP: f32 = 10.0;
    let screen = ctx.content_rect();
    let held = ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|l| l.id != toast && l.order != egui::Order::Background)
            .filter_map(|l| m.area_rect(l.id))
            .filter(|r| {
                r.right() >= screen.right() - CORNER && r.bottom() >= screen.bottom() - CORNER
            })
            .reduce(|a, b| a.union(b))
    });
    match held {
        Some(r) => [r.right() - screen.right(), r.top() - screen.bottom() - GAP],
        None => [-24.0, -22.0],
    }
}

/// A left-to-right fade.
fn hgradient(painter: &egui::Painter, rect: Rect, left: Color32, right: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), left);
    mesh.colored_vertex(rect.right_top(), right);
    mesh.colored_vertex(rect.right_bottom(), right);
    mesh.colored_vertex(rect.left_bottom(), left);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// A seed from the clock, short enough to read out.
fn random_seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    // splitmix64, to spread nearby clock readings.
    let mut z = nanos.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (z ^ (z >> 31)) % 1_000_000_000
}

/// A number is its own seed; a word hashes to one (FNV-1a); nothing is a
/// random one.
pub fn seed_from_text(text: &str) -> u64 {
    let text = text.trim();
    if text.is_empty() {
        return random_seed();
    }
    if let Ok(n) = text.parse() {
        return n;
    }
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in text.to_lowercase().bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h % 1_000_000_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_from_text() {
        assert_eq!(seed_from_text(" 42 "), 42);
        assert_eq!(seed_from_text("Zarya"), seed_from_text("zarya"));
        assert_ne!(seed_from_text("zarya"), seed_from_text("polaris"));
        assert!(seed_from_text("") < 1_000_000_000);
    }

    #[test]
    fn esc_steps_back_then_resumes() {
        let mut menu = Menu::default();
        menu.open(Page::Settings);
        assert_eq!(menu.back(true), None);
        assert_eq!(menu.page, Page::Main);
        assert_eq!(menu.back(true), Some(MenuAction::Resume));
        assert_eq!(menu.back(false), None, "the title has nothing to resume");
    }
}
