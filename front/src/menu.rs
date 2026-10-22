//! Title menu: new game, continue, load, and save management (rename, delete). Pure state; `view` says what to draw.

use crate::input::MenuInput;

/// What the menu asks the session to do. The menu never touches files
/// itself, so it can be driven by plain inputs in tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Load the character's save, or start a new game if there is none.
    Play(String),
    Delete(String),
    Rename {
        from: String,
        to: String,
    },
    Quit,
}

const MAIN: [&str; 3] = ["New game", "Continue", "Load"];
const NEW: usize = 0;
const CONTINUE: usize = 1;
const LOAD: usize = 2;
/// Save files are named after characters; a short cap keeps them tidy.
const MAX_NAME: usize = 24;
/// Rows of the save list shown at once; longer lists scroll.
const VISIBLE_SAVES: usize = 10;

enum Screen {
    Main { selected: usize },
    Load { selected: usize },
    ConfirmDelete { selected: usize },
    Name { naming: Naming, text: String },
}

enum Naming {
    New,
    Rename { selected: usize },
}

pub struct Menu {
    screen: Screen,
    /// Most recently saved first, as `saves::list` returns them.
    saves: Vec<String>,
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
            message: None,
        }
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
        }
        None
    }

    fn choose_main(&mut self) -> Option<Action> {
        let Screen::Main { selected } = self.screen else {
            return None;
        };
        match selected {
            NEW => {
                self.screen = Screen::Name {
                    naming: Naming::New,
                    text: String::new(),
                }
            }
            CONTINUE => match self.saves.first() {
                Some(name) => return Some(Action::Play(name.clone())),
                None => self.show("No saves yet."),
            },
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
            Naming::New if self.saves.contains(&name) => {
                self.show(format!("{name} already has a save; load or delete it."));
                None
            }
            Naming::New => {
                self.screen = Screen::Main { selected: CONTINUE };
                Some(Action::Play(name))
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
                        disabled: i != NEW && self.saves.is_empty(),
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
                save_list(&self.saves, *selected),
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
        };
        MenuView {
            title,
            body,
            message: self.message.clone(),
            hint,
        }
    }
}

/// Names become file names (`saves::path`); this keeps them portable.
fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_'
}

/// A window of at most `VISIBLE_SAVES` rows that keeps the selection in view.
fn save_list(saves: &[String], selected: usize) -> Body {
    if saves.is_empty() {
        return Body::Text(vec!["No saves.".to_string()]);
    }
    let first = selected.saturating_sub(VISIBLE_SAVES - 1);
    let rows = saves
        .iter()
        .enumerate()
        .skip(first)
        .take(VISIBLE_SAVES)
        .map(|(i, name)| Row {
            text: name.clone(),
            selected: i == selected,
            disabled: false,
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
            Some(Action::Play("her".into()))
        );
        assert!(!menu.typing());
    }

    #[test]
    fn new_game_refuses_an_existing_name() {
        let mut menu = menu(&["hero"]);
        press(&mut menu, &[Select]);
        type_text(&mut menu, "hero");
        assert_eq!(press(&mut menu, &[Select]), None);
        assert!(menu.typing());
        assert!(text(&menu).contains("hero already has a save"));
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
