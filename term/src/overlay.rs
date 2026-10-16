//! The hovering panel: a modal box drawn over the map with an inventory and
//! an equipment view, and the skills the player has learned. Pure state plus drawing; `app::run` feeds it keys and
//! performs the actions it returns.

use game::{Domain, IdeaView, ItemView, PartView, PlayerAction, SkillView};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::input::OverlayInput;
use crate::widgets::color;

/// What the panel asks `app::run` to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayAction {
    Close,
    /// Take this as the player's turn; the panel stays open.
    Act(PlayerAction),
}

/// What the panel shows, from `game`'s views, fetched fresh for each key
/// and frame so it never goes stale after a turn.
pub struct Contents<'a> {
    pub inventory: &'a [ItemView],
    pub equipment: &'a [PartView],
    pub skills: &'a [SkillView],
    pub proficiencies: &'a [String],
    pub ideas: &'a [IdeaView],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Inventory,
    Equipment,
    Skills,
}

#[derive(Debug)]
pub struct Overlay {
    tab: Tab,
    selected: usize,
    /// Choosing a carried item for this empty body part (equipment view).
    picking: Option<Picking>,
    notice: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
struct Picking {
    part: u32,
    selected: usize,
}

impl Overlay {
    pub fn new(tab: Tab) -> Self {
        Self {
            tab,
            selected: 0,
            picking: None,
            notice: None,
        }
    }

    pub fn handle(&mut self, input: OverlayInput, contents: &Contents) -> Option<OverlayAction> {
        self.notice = None;
        let shown = |tab| match input {
            OverlayInput::Inventory => tab == Tab::Inventory,
            OverlayInput::Equipment => tab == Tab::Equipment,
            OverlayInput::Skills => tab == Tab::Skills,
            _ => false,
        };
        match input {
            OverlayInput::Close if self.picking.is_some() => self.picking = None,
            OverlayInput::Close => return Some(OverlayAction::Close),
            OverlayInput::Inventory | OverlayInput::Equipment | OverlayInput::Skills
                if shown(self.tab) && self.picking.is_none() =>
            {
                return Some(OverlayAction::Close);
            }
            OverlayInput::Inventory => self.show(Tab::Inventory),
            OverlayInput::Equipment => self.show(Tab::Equipment),
            OverlayInput::Skills => self.show(Tab::Skills),
            OverlayInput::SwitchTab => self.show(match self.tab {
                Tab::Inventory => Tab::Equipment,
                Tab::Equipment => Tab::Skills,
                Tab::Skills => Tab::Inventory,
            }),
            _ => {
                return match (self.tab, self.picking) {
                    (Tab::Inventory, _) => self.handle_inventory(input, contents.inventory),
                    (Tab::Equipment, None) => self.handle_equipment(input, contents),
                    (Tab::Equipment, Some(picking)) => {
                        self.handle_picking(input, picking, contents)
                    }
                    // Nothing to choose: the skills view only shows.
                    (Tab::Skills, _) => None,
                };
            }
        }
        None
    }

    fn show(&mut self, tab: Tab) {
        *self = Self::new(tab);
    }

    fn handle_inventory(
        &mut self,
        input: OverlayInput,
        items: &[ItemView],
    ) -> Option<OverlayAction> {
        move_selection(&mut self.selected, input, items.len());
        let chosen = items.get(self.selected).map(|item| item.item);
        match input {
            OverlayInput::Select => {
                let item = chosen?;
                Some(OverlayAction::Act(PlayerAction::Equip { item, part: None }))
            }
            OverlayInput::Drop => Some(OverlayAction::Act(PlayerAction::Drop(chosen?))),
            OverlayInput::Nock | OverlayInput::Quiver => {
                let item = items.get(self.selected).filter(|item| item.ammo)?.item;
                Some(OverlayAction::Act(if input == OverlayInput::Nock {
                    PlayerAction::Nock(item)
                } else {
                    PlayerAction::Quiver(item)
                }))
            }
            _ => None,
        }
    }

    fn handle_equipment(
        &mut self,
        input: OverlayInput,
        contents: &Contents,
    ) -> Option<OverlayAction> {
        move_selection(&mut self.selected, input, contents.equipment.len());
        if input != OverlayInput::Select {
            return None;
        }
        let part = contents.equipment.get(self.selected)?;
        if let Some(item) = &part.item {
            return Some(OverlayAction::Act(PlayerAction::Unequip(item.item)));
        }
        if fitting(contents.inventory, part).next().is_none() {
            self.notice = Some("Nothing you carry fits there.");
            return None;
        }
        self.picking = Some(Picking {
            part: part.index,
            selected: 0,
        });
        None
    }

    fn handle_picking(
        &mut self,
        input: OverlayInput,
        mut picking: Picking,
        contents: &Contents,
    ) -> Option<OverlayAction> {
        let part = contents
            .equipment
            .iter()
            .find(|p| p.index == picking.part)?;
        let candidates: Vec<&ItemView> = fitting(contents.inventory, part).collect();
        move_selection(&mut picking.selected, input, candidates.len());
        self.picking = Some(picking);
        if input != OverlayInput::Select {
            return None;
        }
        let item = candidates.get(picking.selected)?.item;
        self.picking = None;
        Some(OverlayAction::Act(PlayerAction::Equip {
            item,
            part: Some(part.index),
        }))
    }
}

/// Clamps `selected` into a list of `len` (it may have shrunk since the
/// last key) and applies Up/Down.
fn move_selection(selected: &mut usize, input: OverlayInput, len: usize) {
    let last = len.saturating_sub(1);
    *selected = (*selected).min(last);
    match input {
        OverlayInput::Up => *selected = selected.saturating_sub(1),
        OverlayInput::Down => *selected = (*selected + 1).min(last),
        _ => {}
    }
}

fn fitting<'a>(items: &'a [ItemView], part: &PartView) -> impl Iterator<Item = &'a ItemView> {
    let kind = part.kind;
    items.iter().filter(move |item| item.fits == Some(kind))
}

/// Half the map panel, at least 36×8 where it fits, centred over `area`.
pub fn draw(frame: &mut Frame, area: Rect, overlay: &Overlay, contents: &Contents) {
    let width = (area.width / 2).max(36).min(area.width);
    let height = (area.height / 2).max(8).min(area.height);
    let panel = area.centered(Constraint::Length(width), Constraint::Length(height));

    let picking_part = overlay
        .picking
        .and_then(|picking| contents.equipment.iter().find(|p| p.index == picking.part));
    let (title, mut rows, hint) = match (overlay.tab, overlay.picking, picking_part) {
        (Tab::Equipment, Some(picking), Some(part)) => (
            Line::from(format!("Equip on {}", part.name)),
            item_rows(
                &fitting(contents.inventory, part)
                    .cloned()
                    .collect::<Vec<_>>(),
                picking.selected,
            ),
            "Enter equip  Esc back",
        ),
        (Tab::Equipment, _, _) => (
            tabs(Tab::Equipment),
            part_rows(contents.equipment, overlay.selected),
            "Enter unequip / choose  Tab skills  Esc close",
        ),
        (Tab::Skills, _, _) => (
            tabs(Tab::Skills),
            skill_rows(contents),
            "Tab inventory  Esc close",
        ),
        (Tab::Inventory, _, _) => {
            let ammo = contents
                .inventory
                .get(overlay.selected)
                .is_some_and(|item| item.ammo);
            (
                tabs(Tab::Inventory),
                item_rows(contents.inventory, overlay.selected),
                if ammo {
                    "n nock  v quiver  d drop  Esc close"
                } else {
                    "Enter equip  d drop  Tab equipment  Esc close"
                },
            )
        }
    };
    if let Some(notice) = overlay.notice {
        rows.push(Line::default());
        rows.push(Line::from(notice));
    }
    let block = Block::bordered()
        .title(title)
        .title_bottom(Line::styled(hint, Style::new().add_modifier(Modifier::DIM)));
    frame.render_widget(Clear, panel);
    frame.render_widget(Paragraph::new(rows).block(block), panel);
}

/// Both tab names, the active one highlighted.
fn tabs(active: Tab) -> Line<'static> {
    let tab = |name: &'static str, tab: Tab| {
        let style = if tab == active {
            Style::new().add_modifier(Modifier::REVERSED)
        } else {
            Style::new().add_modifier(Modifier::DIM)
        };
        Span::styled(format!(" {name} "), style)
    };
    Line::from(vec![
        tab("Inventory", Tab::Inventory),
        Span::raw(" "),
        tab("Equipment", Tab::Equipment),
        Span::raw(" "),
        tab("Skills", Tab::Skills),
    ])
}

fn item_rows(items: &[ItemView], selected: usize) -> Vec<Line<'static>> {
    if items.is_empty() {
        return vec![Line::from("You carry nothing.")];
    }
    let selected = selected.min(items.len() - 1);
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let mut row = item_spans(item);
            if let Some(kind) = item.fits {
                row.push(Span::styled(
                    format!("  ({})", kind.noun()),
                    Style::new().add_modifier(Modifier::DIM),
                ));
            }
            if !item.trains.is_empty() {
                row.push(Span::styled(
                    format!("  trains {}", item.trains.join(", ").to_lowercase()),
                    Style::new().add_modifier(Modifier::DIM),
                ));
            }
            highlight(Line::from(row), i == selected)
        })
        .collect()
}

/// The skills under a heading per domain (name, level, XP toward the next
/// level), then the proficiencies attained and the half-formed ideas.
fn skill_rows(contents: &Contents) -> Vec<Line<'static>> {
    let skills = contents.skills;
    if skills.is_empty() {
        return vec![Line::from("You have learned nothing yet.")];
    }
    let width = skills
        .iter()
        .map(|skill| skill.name.chars().count())
        .max()
        .unwrap_or(0);
    let dim = Style::new().add_modifier(Modifier::DIM);
    let heading =
        |text: &'static str| Line::styled(text, Style::new().add_modifier(Modifier::BOLD));
    let mut rows = Vec::new();
    let mut domain = None;
    for skill in skills {
        if domain != Some(skill.domain) {
            domain = Some(skill.domain);
            rows.push(heading(domain_name(skill.domain)));
        }
        let progress = match skill.next {
            Some(next) => format!("  {}/{next} xp", skill.xp),
            None => "  mastered".to_string(),
        };
        rows.push(Line::from(vec![
            Span::raw(format!("  {:<width$}  {:>2}", skill.name, skill.level)),
            Span::styled(progress, dim),
        ]));
    }
    if !contents.proficiencies.is_empty() {
        rows.push(heading("Proficiencies"));
        rows.extend(
            contents
                .proficiencies
                .iter()
                .map(|p| Line::from(format!("  {p}"))),
        );
    }
    if !contents.ideas.is_empty() {
        rows.push(heading("Ideas"));
        for idea in contents.ideas {
            let met: Vec<String> = idea
                .met
                .iter()
                .map(|(skill, level)| format!("{skill} {level}"))
                .collect();
            let more = match idea.unmet {
                0 => String::new(),
                n => format!(" + {n} more"),
            };
            rows.push(Line::from(vec![
                Span::raw("  ? "),
                Span::styled(format!("{}{more}", met.join(", ")), dim),
            ]));
        }
    }
    rows
}

fn domain_name(domain: Domain) -> &'static str {
    match domain {
        Domain::Martial => "Martial",
        Domain::Defence => "Defence",
        Domain::Magic => "Magic",
    }
}

/// One row per body part: its name, then what it holds or a dash.
fn part_rows(parts: &[PartView], selected: usize) -> Vec<Line<'static>> {
    if parts.is_empty() {
        return vec![Line::from("You have no body to equip.")];
    }
    let width = parts
        .iter()
        .map(|part| part.name.chars().count())
        .max()
        .unwrap_or(0);
    let selected = selected.min(parts.len() - 1);
    parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            let mut row = vec![Span::raw(format!("{:<width$}  ", part.name))];
            match &part.item {
                Some(item) => row.extend(item_spans(item)),
                None => row.push(Span::styled("-", Style::new().add_modifier(Modifier::DIM))),
            }
            highlight(Line::from(row), i == selected)
        })
        .collect()
}

/// The item's glyph in its colour, then its name.
pub fn item_spans(item: &ItemView) -> Vec<Span<'static>> {
    let mut spans = vec![
        Span::styled(item.glyph.to_string(), Style::new().fg(color(item.fg))),
        Span::raw(format!(" {}", item.name)),
    ];
    if let Some(note) = &item.note {
        spans.push(Span::styled(
            format!(" ({note})"),
            Style::new().add_modifier(Modifier::DIM),
        ));
    }
    spans
}

fn highlight(line: Line<'static>, selected: bool) -> Line<'static> {
    if selected {
        line.patch_style(Style::new().add_modifier(Modifier::REVERSED))
    } else {
        line
    }
}

#[cfg(test)]
mod tests {
    use game::{ItemRef, PartKind, Rgb, SpawnSeq};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn items(names: &[&str]) -> Vec<ItemView> {
        (0..)
            .zip(names)
            .map(|(seq, name)| ItemView {
                item: ItemRef(SpawnSeq(seq)),
                name: name.to_string(),
                glyph: '/',
                fg: Rgb(200, 100, 50),
                fits: Some(PartKind::Hand),
                ammo: false,
                note: None,
                trains: Vec::new(),
            })
            .collect()
    }

    fn screen(overlay: &Overlay, contents: &Contents) -> String {
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal
            .draw(|frame| draw(frame, frame.area(), overlay, contents))
            .unwrap();
        let buffer = terminal.backend().buffer();
        buffer
            .content()
            .chunks(usize::from(buffer.area.width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn enter_equips_and_d_drops_the_selected_item() {
        let inventory = items(&["knife", "dagger"]);
        let contents = Contents {
            inventory: &inventory,
            equipment: &[],
            skills: &[],
            proficiencies: &[],
            ideas: &[],
        };
        let mut overlay = Overlay::new(Tab::Inventory);
        assert_eq!(overlay.handle(OverlayInput::Down, &contents), None);
        assert_eq!(overlay.handle(OverlayInput::Down, &contents), None);
        assert_eq!(
            overlay.handle(OverlayInput::Select, &contents),
            Some(OverlayAction::Act(PlayerAction::Equip {
                item: inventory[1].item,
                part: None
            }))
        );
        overlay.handle(OverlayInput::Up, &contents);
        assert_eq!(
            overlay.handle(OverlayInput::Drop, &contents),
            Some(OverlayAction::Act(PlayerAction::Drop(inventory[0].item)))
        );
    }

    #[test]
    fn n_and_v_nock_or_quiver_ammunition_and_ignore_anything_else() {
        let mut inventory = items(&["knife", "arrow"]);
        inventory[1].ammo = true;
        let contents = Contents {
            inventory: &inventory,
            equipment: &[],
            skills: &[],
            proficiencies: &[],
            ideas: &[],
        };
        let mut overlay = Overlay::new(Tab::Inventory);
        assert_eq!(overlay.handle(OverlayInput::Nock, &contents), None);
        overlay.handle(OverlayInput::Down, &contents);
        assert_eq!(
            overlay.handle(OverlayInput::Nock, &contents),
            Some(OverlayAction::Act(PlayerAction::Nock(inventory[1].item)))
        );
        assert_eq!(
            overlay.handle(OverlayInput::Quiver, &contents),
            Some(OverlayAction::Act(PlayerAction::Quiver(inventory[1].item)))
        );
        assert!(screen(&overlay, &contents).contains("n nock"));
    }

    #[test]
    fn a_note_shows_beside_the_item_name() {
        let mut inventory = items(&["quiver"]);
        inventory[0].note = Some("3/20".to_string());
        let contents = Contents {
            inventory: &inventory,
            equipment: &[],
            skills: &[],
            proficiencies: &[],
            ideas: &[],
        };
        assert!(screen(&Overlay::new(Tab::Inventory), &contents).contains("quiver (3/20)"));
    }

    #[test]
    fn an_empty_pack_offers_nothing_but_closing() {
        let contents = Contents {
            inventory: &[],
            equipment: &[],
            skills: &[],
            proficiencies: &[],
            ideas: &[],
        };
        let mut overlay = Overlay::new(Tab::Inventory);
        assert_eq!(overlay.handle(OverlayInput::Select, &contents), None);
        assert_eq!(overlay.handle(OverlayInput::Drop, &contents), None);
        assert_eq!(
            overlay.handle(OverlayInput::Close, &contents),
            Some(OverlayAction::Close)
        );
        assert!(screen(&overlay, &contents).contains("You carry nothing."));
    }

    #[test]
    fn a_shrunken_list_keeps_the_selection_inside_it() {
        let three = items(&["a", "b", "c"]);
        let mut overlay = Overlay::new(Tab::Inventory);
        for _ in 0..2 {
            overlay.handle(
                OverlayInput::Down,
                &Contents {
                    inventory: &three,
                    equipment: &[],
                    skills: &[],
                    proficiencies: &[],
                    ideas: &[],
                },
            );
        }
        let one = &three[..1];
        let contents = Contents {
            inventory: one,
            equipment: &[],
            skills: &[],
            proficiencies: &[],
            ideas: &[],
        };
        assert_eq!(
            overlay.handle(OverlayInput::Drop, &contents),
            Some(OverlayAction::Act(PlayerAction::Drop(three[0].item)))
        );
    }

    #[test]
    fn the_panel_lists_items_with_where_they_go() {
        let inventory = items(&["knife"]);
        let screen = screen(
            &Overlay::new(Tab::Inventory),
            &Contents {
                inventory: &inventory,
                equipment: &[],
                skills: &[],
                proficiencies: &[],
                ideas: &[],
            },
        );
        assert!(screen.contains("Inventory"));
        assert!(screen.contains("/ knife  (hand)"));
        assert!(screen.contains("Enter equip"));
    }

    /// Right hand holding `held`, empty left hand, empty body.
    fn hands(held: &ItemView) -> Vec<PartView> {
        let part = |index, name: &str, kind, item| PartView {
            index,
            name: name.to_string(),
            kind,
            item,
        };
        vec![
            part(0, "body", PartKind::Body, None),
            part(1, "right hand", PartKind::Hand, Some(held.clone())),
            part(2, "left hand", PartKind::Hand, None),
        ]
    }

    #[test]
    fn enter_on_a_filled_part_unequips_it() {
        let worn = items(&["sword"]);
        let equipment = hands(&worn[0]);
        let contents = Contents {
            inventory: &[],
            equipment: &equipment,
            skills: &[],
            proficiencies: &[],
            ideas: &[],
        };
        let mut overlay = Overlay::new(Tab::Equipment);
        overlay.handle(OverlayInput::Down, &contents);
        assert_eq!(
            overlay.handle(OverlayInput::Select, &contents),
            Some(OverlayAction::Act(PlayerAction::Unequip(worn[0].item)))
        );
    }

    #[test]
    fn enter_on_an_empty_part_picks_a_fitting_item_for_it() {
        let worn = items(&["sword"]);
        let equipment = hands(&worn[0]);
        let inventory = items(&["knife", "dagger"]);
        let contents = Contents {
            inventory: &inventory,
            equipment: &equipment,
            skills: &[],
            proficiencies: &[],
            ideas: &[],
        };
        let mut overlay = Overlay::new(Tab::Equipment);
        overlay.handle(OverlayInput::Down, &contents);
        overlay.handle(OverlayInput::Down, &contents);
        assert_eq!(overlay.handle(OverlayInput::Select, &contents), None);
        let picker = screen(&overlay, &contents);
        assert!(picker.contains("Equip on left hand") && picker.contains("dagger"));

        overlay.handle(OverlayInput::Down, &contents);
        assert_eq!(
            overlay.handle(OverlayInput::Select, &contents),
            Some(OverlayAction::Act(PlayerAction::Equip {
                item: inventory[1].item,
                part: Some(2)
            }))
        );
        assert!(screen(&overlay, &contents).contains("left hand"));
    }

    #[test]
    fn an_empty_part_with_nothing_to_fit_says_so() {
        let worn = items(&["sword"]);
        let equipment = hands(&worn[0]);
        // Hand items only; the body part takes none of them.
        let inventory = items(&["knife"]);
        let contents = Contents {
            inventory: &inventory,
            equipment: &equipment,
            skills: &[],
            proficiencies: &[],
            ideas: &[],
        };
        let mut overlay = Overlay::new(Tab::Equipment);
        assert_eq!(overlay.handle(OverlayInput::Select, &contents), None);
        assert!(screen(&overlay, &contents).contains("Nothing you carry fits there."));
    }

    #[test]
    fn escape_backs_out_of_the_picker_before_closing() {
        let worn = items(&["sword"]);
        let equipment = hands(&worn[0]);
        let inventory = items(&["knife"]);
        let contents = Contents {
            inventory: &inventory,
            equipment: &equipment,
            skills: &[],
            proficiencies: &[],
            ideas: &[],
        };
        let mut overlay = Overlay::new(Tab::Equipment);
        overlay.handle(OverlayInput::Down, &contents);
        overlay.handle(OverlayInput::Down, &contents);
        overlay.handle(OverlayInput::Select, &contents);
        assert_eq!(overlay.handle(OverlayInput::Close, &contents), None);
        assert!(screen(&overlay, &contents).contains("right hand"));
        assert_eq!(
            overlay.handle(OverlayInput::Close, &contents),
            Some(OverlayAction::Close)
        );
    }

    #[test]
    fn tab_keys_switch_views_and_the_open_view_key_closes() {
        let worn = items(&["sword"]);
        let equipment = hands(&worn[0]);
        let contents = Contents {
            inventory: &[],
            equipment: &equipment,
            skills: &[],
            proficiencies: &[],
            ideas: &[],
        };
        let mut overlay = Overlay::new(Tab::Inventory);
        assert_eq!(overlay.handle(OverlayInput::Equipment, &contents), None);
        let shown = screen(&overlay, &contents);
        assert!(shown.contains("right hand  / sword"), "{shown}");
        assert!(shown.contains("left hand   -"));
        assert_eq!(overlay.handle(OverlayInput::SwitchTab, &contents), None);
        assert!(screen(&overlay, &contents).contains("You have learned nothing yet."));
        assert_eq!(overlay.handle(OverlayInput::SwitchTab, &contents), None);
        assert!(screen(&overlay, &contents).contains("You carry nothing."));
        assert_eq!(
            overlay.handle(OverlayInput::Inventory, &contents),
            Some(OverlayAction::Close)
        );
    }

    fn skill(name: &str, domain: Domain, level: u32, xp: u32, next: Option<u32>) -> SkillView {
        SkillView {
            name: name.to_string(),
            domain,
            level,
            xp,
            next,
        }
    }

    #[test]
    fn the_skills_tab_groups_skills_by_domain_and_tab_cycles_to_it() {
        let skills = [
            skill("Melee", Domain::Martial, 3, 460, Some(800)),
            skill("Dagger", Domain::Martial, 20, 20_000, None),
            skill("Dodge", Domain::Defence, 0, 12, Some(50)),
        ];
        let contents = Contents {
            inventory: &[],
            equipment: &[],
            skills: &skills,
            proficiencies: &[],
            ideas: &[],
        };
        let mut overlay = Overlay::new(Tab::Equipment);
        overlay.handle(OverlayInput::SwitchTab, &contents);
        let shown = screen(&overlay, &contents);
        for line in [
            "Martial",
            "Melee    3  460/800 xp",
            "Dagger  20  mastered",
            "Defence",
            "Dodge    0  12/50 xp",
        ] {
            assert!(shown.contains(line), "{line:?} in\n{shown}");
        }
        assert_eq!(
            overlay.handle(OverlayInput::Skills, &contents),
            Some(OverlayAction::Close)
        );
        let empty = Contents {
            inventory: &[],
            equipment: &[],
            skills: &[],
            proficiencies: &[],
            ideas: &[],
        };
        assert!(
            screen(&Overlay::new(Tab::Skills), &empty).contains("You have learned nothing yet.")
        );
    }

    #[test]
    fn an_item_row_says_what_it_trains() {
        let mut inventory = items(&["knife"]);
        inventory[0].fits = None;
        inventory[0].trains = vec!["Slash".into(), "Dagger".into()];
        let contents = Contents {
            inventory: &inventory,
            equipment: &[],
            skills: &[],
            proficiencies: &[],
            ideas: &[],
        };
        assert!(screen(&Overlay::new(Tab::Inventory), &contents).contains("trains slash, dagger"));
    }

    #[test]
    fn the_skills_tab_lists_proficiencies_and_half_formed_ideas() {
        let skills = [skill("Dodge", Domain::Defence, 4, 800, Some(1250))];
        let ideas = [IdeaView {
            name: "Fencing".into(),
            met: vec![("Dodge".into(), 4)],
            unmet: 1,
        }];
        let contents = Contents {
            inventory: &[],
            equipment: &[],
            skills: &skills,
            proficiencies: &["footwork".to_string()],
            ideas: &ideas,
        };
        let shown = screen(&Overlay::new(Tab::Skills), &contents);
        for line in ["Proficiencies", "  footwork", "Ideas", "Dodge 4 + 1 more"] {
            assert!(shown.contains(line), "{line:?} in\n{shown}");
        }
        assert!(!shown.contains("Fencing"), "an idea's name stays hidden");
    }
}
