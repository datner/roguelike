//! Title menu: new game, continue, load, and save management (rename, delete). Pure state; `view` says what to draw.

use game::OriginView;

use crate::input::MenuInput;

/// What the menu asks the session to do. The menu never touches files
/// itself, so it can be driven by plain inputs in tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Load the character's save, or start a new game if there is none.
    Play(String),
    /// Start a new game for a character with no save, as `origin` (a key;
    /// `None` for the first).
    New {
        character: String,
        origin: Option<String>,
    },
    /// Delete the character's outdated save and start a new game.
    Replace {
        character: String,
        origin: Option<String>,
    },
    Delete(String),
    Rename {
        from: String,
        to: String,
    },
    Quit,
}

const MAIN: [&str; 4] = ["New game", "Continue", "Load", "Fallen"];
const NEW: usize = 0;
const CONTINUE: usize = 1;
const LOAD: usize = 2;
const FALLEN: usize = 3;
/// Save files are named after characters; a short cap keeps them tidy.
const MAX_NAME: usize = 24;
/// Rows of the save list shown at once; longer lists scroll.
const VISIBLE_SAVES: usize = 10;
/// Where an origin's description wraps, inside the menu's box.
const DETAIL_WIDTH: usize = 44;

enum Screen {
    Main {
        selected: usize,
    },
    Load {
        selected: usize,
    },
    ConfirmDelete {
        selected: usize,
    },
    Name {
        naming: Naming,
        text: String,
    },
    /// Choosing what a new character starts as; `replace` when its name
    /// has an outdated save.
    Origin {
        character: String,
        replace: bool,
        selected: usize,
    },
    /// The lineage's past lives, read-only (docs/plans/legacy.md).
    Fallen,
}

enum Naming {
    New,
    Rename { selected: usize },
}

pub struct Menu {
    screen: Screen,
    /// Most recently saved first, as `saves::list` returns them.
    saves: Vec<String>,
    /// Saves from another save format, which a new game may replace.
    outdated: Vec<String>,
    /// What a new character can start as; none skips the choice.
    origins: Vec<OriginView>,
    /// The lineage's past lives, newest first, a line each.
    fallen: Vec<String>,
    message: Option<String>,
}

/// One screen of the menu, ready to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuView {
    pub title: &'static str,
    pub body: Body,
    pub message: Option<String>,
    /// The keys this screen takes, for a hint line.
    pub hint: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    /// Rows to pick from, already scrolled to keep the selection in view.
    List(Vec<Row>),
    /// Plain lines, such as a question or "No saves.".
    Text(Vec<String>),
    /// Rows to pick from and, below them, lines about the selected one.
    Choice {
        rows: Vec<Row>,
        details: Vec<String>,
    },
    /// A text field after a prompt.
    Field { prompt: &'static str, text: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub text: String,
    pub selected: bool,
    pub disabled: bool,
}

impl Menu {
    pub fn new(saves: Vec<String>) -> Self {
        Self {
            screen: Screen::Main { selected: NEW },
            saves,
            outdated: Vec::new(),
            origins: Vec::new(),
            fallen: Vec::new(),
            message: None,
        }
    }

    /// The lineage's past lives, newest first, a line each (`text::life`).
    pub fn set_fallen(&mut self, fallen: Vec<String>) {
        self.fallen = fallen;
    }

    /// The character still alive, if any: one living character per
    /// lineage (docs/plans/legacy.md), so a new one waits for its end. An
    /// outdated save can't be played and doesn't count.
    fn living(&self) -> Option<&String> {
        self.saves.iter().find(|name| !self.outdated.contains(name))
    }

    /// What a new character can start as (`game::origins`), in order.
    pub fn set_origins(&mut self, origins: Vec<OriginView>) {
        self.origins = origins;
    }

    /// Replaces the save list after the session changed it, keeping any
    /// selection inside the new list.
    pub fn set_saves(&mut self, saves: Vec<String>) {
        self.saves = saves;
        let last = self.saves.len().saturating_sub(1);
        if let Screen::Load { selected } | Screen::ConfirmDelete { selected } = &mut self.screen {
            *selected = (*selected).min(last);
        }
    }

    pub fn set_outdated(&mut self, outdated: Vec<String>) {
        self.outdated = outdated;
    }

    pub fn show(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
    }

    /// Whether a text field has focus, so letters type instead of navigate.
    pub fn typing(&self) -> bool {
        matches!(self.screen, Screen::Name { .. })
    }

    pub fn handle(&mut self, input: MenuInput) -> Option<Action> {
        self.message = None;
        match &mut self.screen {
            Screen::Main { selected } => match input {
                MenuInput::Up => *selected = selected.saturating_sub(1),
                MenuInput::Down => *selected = (*selected + 1).min(MAIN.len() - 1),
                MenuInput::Back => return Some(Action::Quit),
                MenuInput::Select => return self.choose_main(),
                _ => {}
            },
            Screen::Load { selected } => match input {
                MenuInput::Up => *selected = selected.saturating_sub(1),
                MenuInput::Down => {
                    *selected = (*selected + 1).min(self.saves.len().saturating_sub(1));
                }
                MenuInput::Back => self.screen = Screen::Main { selected: LOAD },
                MenuInput::Select => {
                    let name = self.saves.get(*selected)?.clone();
                    self.screen = Screen::Main { selected: CONTINUE };
                    return Some(Action::Play(name));
                }
                MenuInput::Delete if *selected < self.saves.len() => {
                    self.screen = Screen::ConfirmDelete {
                        selected: *selected,
                    };
                }
                MenuInput::Rename => {
                    let text = self.saves.get(*selected)?.clone();
                    let naming = Naming::Rename {
                        selected: *selected,
                    };
                    self.screen = Screen::Name { naming, text };
                }
                _ => {}
            },
            Screen::ConfirmDelete { selected } => {
                let selected = *selected;
                match input {
                    MenuInput::Select => {
                        self.screen = Screen::Load { selected };
                        return self.saves.get(selected).cloned().map(Action::Delete);
                    }
                    MenuInput::Back => self.screen = Screen::Load { selected },
                    _ => {}
                }
            }
            Screen::Name { naming, text } => match input {
                MenuInput::Char(c) if is_name_char(c) && text.chars().count() < MAX_NAME => {
                    text.push(c);
                }
                MenuInput::Backspace => {
                    text.pop();
                }
                MenuInput::Back => {
                    self.screen = match naming {
                        Naming::New => Screen::Main { selected: NEW },
                        Naming::Rename { selected } => Screen::Load {
                            selected: *selected,
                        },
                    };
                }
                MenuInput::Select => return self.submit_name(),
                _ => {}
            },
            Screen::Origin {
                character,
                replace,
                selected,
            } => match input {
                MenuInput::Up => *selected = selected.saturating_sub(1),
                MenuInput::Down => {
                    *selected = (*selected + 1).min(self.origins.len().saturating_sub(1));
                }
                MenuInput::Back => {
                    self.screen = Screen::Name {
                        naming: Naming::New,
                        text: std::mem::take(character),
                    };
                }
                MenuInput::Select => {
                    let origin = self.origins.get(*selected).map(|o| o.key.clone());
                    let (character, replace) = (std::mem::take(character), *replace);
                    return Some(self.start(character, replace, origin));
                }
                _ => {}
            },
            Screen::Fallen => {
                if input == MenuInput::Back {
                    self.screen = Screen::Main { selected: FALLEN };
                }
            }
        }
        None
    }

    /// A new game for `character`, back on the main menu.
    fn start(&mut self, character: String, replace: bool, origin: Option<String>) -> Action {
        self.screen = Screen::Main { selected: CONTINUE };
        if replace {
            Action::Replace { character, origin }
        } else {
            Action::New { character, origin }
        }
    }

    fn choose_main(&mut self) -> Option<Action> {
        let Screen::Main { selected } = self.screen else {
            return None;
        };
        match selected {
            NEW => match self.living() {
                Some(name) => self.show(format!(
                    "{name} still lives: continue them, or delete them to start anew."
                )),
                None => {
                    self.screen = Screen::Name {
                        naming: Naming::New,
                        text: String::new(),
                    }
                }
            },
            CONTINUE => match self.saves.first() {
                Some(name) => return Some(Action::Play(name.clone())),
                None => self.show("No saves yet."),
            },
            FALLEN => self.screen = Screen::Fallen,
            _ => self.screen = Screen::Load { selected: 0 },
        }
        None
    }

    fn submit_name(&mut self) -> Option<Action> {
        let Screen::Name { naming, text } = &self.screen else {
            return None;
        };
        if text.is_empty() {
            return None;
        }
        let name = text.clone();
        match *naming {
            Naming::New if self.saves.contains(&name) && !self.outdated.contains(&name) => {
                self.show(format!("{name} already has a save; load or delete it."));
                None
            }
            Naming::New => {
                let replace = self.outdated.contains(&name);
                if self.origins.is_empty() {
                    return Some(self.start(name, replace, None));
                }
                self.screen = Screen::Origin {
                    character: name,
                    replace,
                    selected: 0,
                };
                None
            }
            Naming::Rename { selected } => {
                let from = self.saves.get(selected)?.clone();
                self.screen = Screen::Load { selected };
                (from != name).then_some(Action::Rename { from, to: name })
            }
        }
    }

    pub fn view(&self) -> MenuView {
        let (title, body, hint) = match &self.screen {
            Screen::Main { selected } => {
                let rows = MAIN
                    .iter()
                    .enumerate()
                    .map(|(i, label)| Row {
                        text: match (i, self.saves.first()) {
                            (CONTINUE, Some(name)) => format!("{label} ({name})"),
                            _ => (*label).to_string(),
                        },
                        selected: i == *selected,
                        disabled: match i {
                            NEW => self.living().is_some(),
                            FALLEN => self.fallen.is_empty(),
                            _ => self.saves.is_empty(),
                        },
                    })
                    .collect();
                (
                    "Roguelike",
                    Body::List(rows),
                    "↑↓ select  Enter choose  Esc quit",
                )
            }
            Screen::Load { selected } => (
                "Load",
                save_list(&self.saves, &self.outdated, *selected),
                "Enter load  r rename  d delete  Esc back",
            ),
            Screen::ConfirmDelete { selected } => {
                let name = self.saves.get(*selected).map_or("", String::as_str);
                (
                    "Delete",
                    Body::Text(vec![
                        format!("Delete {name}?"),
                        "Its save cannot be recovered.".to_string(),
                    ]),
                    "Enter delete  Esc cancel",
                )
            }
            Screen::Name { naming, text } => {
                let (title, prompt) = match naming {
                    Naming::New => ("New game", "Character name:"),
                    Naming::Rename { .. } => ("Rename", "New name:"),
                };
                let text = text.clone();
                (
                    title,
                    Body::Field { prompt, text },
                    "Enter confirm  Esc back",
                )
            }
            Screen::Origin { selected, .. } => {
                let rows = self
                    .origins
                    .iter()
                    .enumerate()
                    .map(|(i, origin)| Row {
                        text: origin.name.clone(),
                        selected: i == *selected,
                        disabled: false,
                    })
                    .collect();
                let details = self.origins.get(*selected).map_or_else(Vec::new, |origin| {
                    let mut lines = vec![String::new()];
                    lines.extend(wrap(&origin.summary, DETAIL_WIDTH, ""));
                    lines.push(String::new());
                    for line in &origin.starts_with {
                        lines.extend(wrap(line, DETAIL_WIDTH, "  "));
                    }
                    lines
                });
                (
                    "Origin",
                    Body::Choice { rows, details },
                    "↑↓ select  Enter start  Esc back",
                )
            }
            Screen::Fallen => {
                let lines = self
                    .fallen
                    .iter()
                    .flat_map(|life| {
                        let mut lines = wrap(life, DETAIL_WIDTH, "  ");
                        lines.push(String::new());
                        lines
                    })
                    .collect();
                ("Fallen", Body::Text(lines), "Esc back")
            }
        };
        MenuView {
            title,
            body,
            message: self.message.clone(),
            hint,
        }
    }
}

/// `text` in lines of at most `width` characters, broken between words; a
/// line after the first starts with `indent`.
fn wrap(text: &str, width: usize, indent: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let indent = if lines.is_empty() { 0 } else { indent.len() };
        if !line.is_empty() && indent + line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    lines.push(line);
    let mut lines = lines.into_iter();
    let first = lines.next().unwrap_or_default();
    std::iter::once(first)
        .chain(lines.map(|line| format!("{indent}{line}")))
        .collect()
}

/// Names become file names (`saves::path`); this keeps them portable.
fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_'
}

/// A window of at most `VISIBLE_SAVES` rows that keeps the selection in view.
/// An outdated save is listed dimmed, with how to replace it.
fn save_list(saves: &[String], outdated: &[String], selected: usize) -> Body {
    if saves.is_empty() {
        return Body::Text(vec!["No saves.".to_string()]);
    }
    let first = selected.saturating_sub(VISIBLE_SAVES - 1);
    let rows = saves
        .iter()
        .enumerate()
        .skip(first)
        .take(VISIBLE_SAVES)
        .map(|(i, name)| {
            let old = outdated.contains(name);
            Row {
                text: if old {
                    format!("{name} (old version: a new game replaces it)")
                } else {
                    name.clone()
                },
                selected: i == selected,
                disabled: old,
            }
        })
        .collect();
    Body::List(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu(saves: &[&str]) -> Menu {
        Menu::new(saves.iter().map(|s| s.to_string()).collect())
    }

    fn press(menu: &mut Menu, inputs: &[MenuInput]) -> Option<Action> {
        let mut last = None;
        for &input in inputs {
            last = menu.handle(input);
        }
        last
    }

    fn type_text(menu: &mut Menu, text: &str) {
        for c in text.chars() {
            menu.handle(MenuInput::Char(c));
        }
    }

    /// Everything the view says, one line per row or text line.
    fn text(menu: &Menu) -> String {
        let view = menu.view();
        let mut lines = match view.body {
            Body::List(rows) => rows
                .into_iter()
                .map(|row| {
                    let marker = if row.selected { "> " } else { "  " };
                    format!("{marker}{}", row.text)
                })
                .collect(),
            Body::Choice { rows, details } => rows
                .into_iter()
                .map(|row| {
                    let marker = if row.selected { "> " } else { "  " };
                    format!("{marker}{}", row.text)
                })
                .chain(details)
                .collect(),
            Body::Text(lines) => lines,
            Body::Field { prompt, text } => vec![prompt.to_string(), text],
        };
        lines.extend(view.message);
        lines.join("\n")
    }

    use MenuInput::*;

    #[test]
    fn new_game_asks_for_a_name() {
        let mut menu = menu(&[]);
        assert_eq!(press(&mut menu, &[Select]), None);
        assert!(menu.typing());
        type_text(&mut menu, "hero/..");
        assert_eq!(
            press(&mut menu, &[Backspace, Select]),
            Some(Action::New {
                character: "her".into(),
                origin: None
            }),
            "no origins to choose from: the default"
        );
        assert!(!menu.typing());
    }

    fn origin(key: &str, summary: &str) -> OriginView {
        OriginView {
            key: key.into(),
            name: key.into(),
            summary: summary.into(),
            starts_with: vec![format!("Carries: {key}'s things")],
        }
    }

    fn with_origins(saves: &[&str]) -> Menu {
        let mut menu = menu(saves);
        menu.set_origins(vec![
            origin("Wanderer", "A traveller."),
            origin("Duelist", "A fencer."),
        ]);
        menu
    }

    #[test]
    fn a_new_game_asks_for_an_origin_after_the_name() {
        let mut menu = with_origins(&[]);
        press(&mut menu, &[Select]);
        type_text(&mut menu, "hero");
        assert_eq!(press(&mut menu, &[Select]), None);
        assert!(!menu.typing());
        assert_eq!(menu.view().title, "Origin");
        assert!(text(&menu).contains("> Wanderer\n  Duelist"));
        assert!(text(&menu).contains("A traveller."));
        press(&mut menu, &[Down]);
        assert!(text(&menu).contains("A fencer."));
        assert!(text(&menu).contains("Carries: Duelist's things"));
        assert_eq!(
            press(&mut menu, &[Select]),
            Some(Action::New {
                character: "hero".into(),
                origin: Some("Duelist".into())
            })
        );
    }

    #[test]
    fn esc_from_the_origins_goes_back_to_the_name() {
        let mut menu = with_origins(&[]);
        press(&mut menu, &[Select]);
        type_text(&mut menu, "hero");
        press(&mut menu, &[Select, Back]);
        assert!(menu.typing());
        assert!(text(&menu).contains("hero"));
        press(&mut menu, &[Backspace]);
        assert_eq!(
            press(&mut menu, &[Select, Select]),
            Some(Action::New {
                character: "her".into(),
                origin: Some("Wanderer".into())
            })
        );
    }

    #[test]
    fn loading_a_save_never_asks_for_an_origin() {
        let mut menu = with_origins(&["hero"]);
        assert_eq!(
            press(&mut menu, &[Down, Select]),
            Some(Action::Play("hero".into()))
        );
    }

    #[test]
    fn long_details_wrap_inside_the_box() {
        let lines = wrap(&"word ".repeat(20), 20, "  ");
        assert!(
            lines.iter().all(|line| line.chars().count() <= 20),
            "{lines:?}"
        );
        assert!(lines[1].starts_with("  word"));
    }

    #[test]
    fn new_game_waits_until_no_one_is_alive() {
        let mut menu = menu(&["hero"]);
        assert!(menu_rows(&menu)[0].disabled);
        assert_eq!(press(&mut menu, &[Select]), None);
        assert!(!menu.typing());
        assert!(text(&menu).contains("hero still lives"));
    }

    #[test]
    fn the_fallen_are_listed_newest_first_and_esc_leads_back() {
        let mut menu = menu(&[]);
        assert!(menu_rows(&menu)[FALLEN].disabled, "no one has fallen yet");
        menu.set_fallen(vec![
            "Bo: abandoned in camp, turn 3.".into(),
            "Ana: …".into(),
        ]);
        press(&mut menu, &[Down, Down, Down, Select]);
        let view = menu.view();
        assert_eq!(view.title, "Fallen");
        let Body::Text(lines) = view.body else {
            panic!("the fallen are plain lines");
        };
        assert_eq!(lines[0], "Bo: abandoned in camp, turn 3.");
        press(&mut menu, &[Back]);
        assert_eq!(menu.view().title, "Roguelike");
        assert!(menu_rows(&menu)[FALLEN].selected);
    }

    fn menu_rows(menu: &Menu) -> Vec<Row> {
        let Body::List(rows) = menu.view().body else {
            panic!("the main menu is a list");
        };
        rows
    }

    #[test]
    fn new_game_replaces_an_outdated_save_of_that_name() {
        let mut menu = menu(&["old"]);
        menu.set_outdated(vec!["old".into()]);
        press(&mut menu, &[Down, Down, Select]);
        assert!(text(&menu).contains("old (old version: a new game replaces it)"));
        press(&mut menu, &[Back, Up, Up, Select]);
        type_text(&mut menu, "old");
        assert_eq!(
            press(&mut menu, &[Select]),
            Some(Action::Replace {
                character: "old".into(),
                origin: None
            })
        );
    }

    #[test]
    fn continue_plays_the_most_recent_save() {
        let mut menu = menu(&["recent", "older"]);
        assert!(text(&menu).contains("Continue (recent)"));
        assert_eq!(
            press(&mut menu, &[Down, Select]),
            Some(Action::Play("recent".into()))
        );
    }

    #[test]
    fn continue_without_saves_says_so() {
        let mut menu = menu(&[]);
        assert_eq!(press(&mut menu, &[Down, Select]), None);
        assert!(text(&menu).contains("No saves yet."));
    }

    #[test]
    fn load_plays_the_selected_save() {
        let mut menu = menu(&["a", "b", "c"]);
        let action = press(
            &mut menu,
            &[Down, Down, Select, Down, Down, Down, Up, Select],
        );
        assert_eq!(action, Some(Action::Play("b".into())));
    }

    #[test]
    fn delete_needs_confirmation() {
        let mut menu = menu(&["a", "b"]);
        press(&mut menu, &[Down, Down, Select, Down, Delete]);
        assert!(text(&menu).contains("Delete b?"));
        assert_eq!(press(&mut menu, &[Back]), None);
        assert_eq!(
            press(&mut menu, &[Delete, Select]),
            Some(Action::Delete("b".into()))
        );
        menu.set_saves(vec!["a".into()]);
        assert_eq!(press(&mut menu, &[Select]), Some(Action::Play("a".into())));
    }

    #[test]
    fn rename_starts_from_the_current_name() {
        let mut menu = menu(&["hero"]);
        press(&mut menu, &[Down, Down, Select, Rename]);
        assert!(menu.typing());
        assert_eq!(press(&mut menu, &[Select]), None, "unchanged name");
        press(&mut menu, &[Rename]);
        type_text(&mut menu, "ine");
        assert_eq!(
            press(&mut menu, &[Select]),
            Some(Action::Rename {
                from: "hero".into(),
                to: "heroine".into()
            })
        );
    }

    #[test]
    fn back_leads_out_to_quit() {
        let mut menu = menu(&["hero"]);
        assert_eq!(press(&mut menu, &[Down, Down, Select, Back]), None);
        assert_eq!(press(&mut menu, &[Back]), Some(Action::Quit));
    }

    #[test]
    fn long_save_lists_keep_the_selection_in_view() {
        let names: Vec<String> = (0..15).map(|i| format!("save{i:02}")).collect();
        let mut menu = Menu::new(names);
        press(&mut menu, &[Down, Down, Select]);
        for _ in 0..14 {
            menu.handle(Down);
        }
        let text = text(&menu);
        assert!(text.contains("> save14"));
        assert!(!text.contains("save04"));
    }
}
