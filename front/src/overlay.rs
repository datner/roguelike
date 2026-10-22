//! The hovering panel's state: inventory, equipment, skills and styles tabs, the focused row, and picking an item for an empty body part. Pure state; a frontend draws it from the accessors and performs the actions `handle` returns.

use game::{
    Domain, GameState, IdeaView, ItemView, PartView, PlayerAction, SkillView, StyleView,
    TechniqueView, equipment_view, ideas, inventory_view, proficiencies, skills, styles,
    techniques,
};

use crate::input::OverlayInput;
use crate::text::{self, Row, Seg};

/// What the panel asks the session to do.
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
    pub styles: &'a [StyleView],
    pub techniques: &'a [TechniqueView],
}

/// The views `Contents` borrows, fetched from the game in one go.
pub struct Views {
    pub inventory: Vec<ItemView>,
    pub equipment: Vec<PartView>,
    pub skills: Vec<SkillView>,
    pub proficiencies: Vec<String>,
    pub ideas: Vec<IdeaView>,
    pub styles: Vec<StyleView>,
    pub techniques: Vec<TechniqueView>,
}

impl Views {
    pub fn of(state: &GameState) -> Self {
        Self {
            inventory: inventory_view(state),
            equipment: equipment_view(state),
            skills: skills(state),
            proficiencies: proficiencies(state),
            ideas: ideas(state),
            styles: styles(state),
            techniques: techniques(state),
        }
    }

    pub fn contents(&self) -> Contents<'_> {
        Contents {
            inventory: &self.inventory,
            equipment: &self.equipment,
            skills: &self.skills,
            proficiencies: &self.proficiencies,
            ideas: &self.ideas,
            styles: &self.styles,
            techniques: &self.techniques,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Inventory,
    Equipment,
    Skills,
    /// Styles, proficiencies, techniques and ideas.
    Styles,
}

#[derive(Debug)]
pub struct Overlay {
    tab: Tab,
    selected: usize,
    /// Choosing a carried item for this empty body part (equipment view).
    picking: Option<Picking>,
    notice: Option<&'static str>,
}

/// Choosing a carried item for the empty body part `part` (its
/// `PartView::index`); `selected` indexes the items that fit it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Picking {
    pub part: u32,
    pub selected: usize,
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

    pub fn tab(&self) -> Tab {
        self.tab
    }

    /// The focused row of the tab's list; it may be past the end of a list
    /// that shrank since, so clamp it when drawing.
    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn picking(&self) -> Option<Picking> {
        self.picking
    }

    /// A one-off message from the last key, such as why nothing happened.
    pub fn notice(&self) -> Option<&'static str> {
        self.notice
    }

    pub fn handle(&mut self, input: OverlayInput, contents: &Contents) -> Option<OverlayAction> {
        self.notice = None;
        let shown = |tab| match input {
            OverlayInput::Inventory => tab == Tab::Inventory,
            OverlayInput::Equipment => tab == Tab::Equipment,
            OverlayInput::Skills => tab == Tab::Skills,
            OverlayInput::Styles => tab == Tab::Styles,
            _ => false,
        };
        match input {
            OverlayInput::Close if self.picking.is_some() => self.picking = None,
            OverlayInput::Close => return Some(OverlayAction::Close),
            OverlayInput::Inventory
            | OverlayInput::Equipment
            | OverlayInput::Skills
            | OverlayInput::Styles
                if shown(self.tab) && self.picking.is_none() =>
            {
                return Some(OverlayAction::Close);
            }
            OverlayInput::Inventory => self.show(Tab::Inventory),
            OverlayInput::Equipment => self.show(Tab::Equipment),
            OverlayInput::Skills => self.show(Tab::Skills),
            OverlayInput::Styles => self.show(Tab::Styles),
            OverlayInput::SwitchTab => self.show(match self.tab {
                Tab::Inventory => Tab::Equipment,
                Tab::Equipment => Tab::Skills,
                Tab::Skills => Tab::Styles,
                Tab::Styles => Tab::Inventory,
            }),
            _ => {
                return match (self.tab, self.picking) {
                    (Tab::Inventory, _) => self.handle_inventory(input, contents.inventory),
                    (Tab::Equipment, None) => self.handle_equipment(input, contents),
                    (Tab::Equipment, Some(picking)) => {
                        self.handle_picking(input, picking, contents)
                    }
                    (Tab::Skills, _) => {
                        move_selection(&mut self.selected, input, contents.skills.len());
                        None
                    }
                    (Tab::Styles, _) => self.handle_styles(input, contents.styles),
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
            OverlayInput::Study => {
                let item = items.get(self.selected).filter(|item| item.lesson)?.item;
                Some(OverlayAction::Act(PlayerAction::Study(item)))
            }
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

    /// The skills view's only choice: which style to follow.
    fn handle_styles(
        &mut self,
        input: OverlayInput,
        styles: &[StyleView],
    ) -> Option<OverlayAction> {
        move_selection(&mut self.selected, input, styles.len());
        let style = styles.get(self.selected)?;
        (input == OverlayInput::Select)
            .then_some(OverlayAction::Act(PlayerAction::FollowStyle(style.style)))
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
        OverlayInput::Focus(row) => *selected = row.min(last),
        _ => {}
    }
}

/// The carried items that fit `part`, in carried order: what picking offers.
pub fn fitting<'a>(items: &'a [ItemView], part: &PartView) -> impl Iterator<Item = &'a ItemView> {
    let kind = part.kind;
    items.iter().filter(move |item| item.fits == Some(kind))
}

/// The panel ready to draw: a title, lines, the line to keep in view as a
/// long list scrolls, and the keys this view takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelView {
    pub title: Title,
    pub lines: Vec<PanelLine>,
    pub focus: Option<usize>,
    pub hint: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Title {
    /// Every tab's name, this one active (`TABS`).
    Tabs(Tab),
    Text(String),
}

/// Tab names in the order they are shown and cycled.
pub const TABS: [(Tab, &str); 4] = [
    (Tab::Inventory, "Inventory"),
    (Tab::Equipment, "Equipment"),
    (Tab::Skills, "Skills"),
    (Tab::Styles, "Styles"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelLine {
    pub row: Row,
    pub kind: LineKind,
    /// For a row that can be focused, its position in the list
    /// (`OverlayInput::Focus`); headings and notes have none.
    pub index: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Plain,
    Heading,
    /// The focused row: what Enter and the row's keys act on.
    Selected,
}

impl PanelLine {
    fn plain(row: Row) -> Self {
        Self {
            row,
            kind: LineKind::Plain,
            index: None,
        }
    }

    fn heading(text: &str) -> Self {
        Self {
            row: vec![Seg::Plain(text.to_string())],
            kind: LineKind::Heading,
            index: None,
        }
    }

    /// Row `index` of a list where `selected` is focused.
    fn row(row: Row, index: usize, selected: usize) -> Self {
        let kind = if index == selected {
            LineKind::Selected
        } else {
            LineKind::Plain
        };
        Self {
            row,
            kind,
            index: Some(index),
        }
    }
}

impl Overlay {
    /// What can be done to the focused row, as the inputs that do it: for a
    /// context menu, and the same choices the hint line names.
    pub fn actions(&self, contents: &Contents) -> Vec<(&'static str, OverlayInput)> {
        use OverlayInput::{Drop, Nock, Quiver, Select, Study};
        match (self.tab, self.picking) {
            (Tab::Equipment, Some(_)) => vec![("Equip", Select)],
            (Tab::Equipment, None) => match contents.equipment.get(self.selected) {
                Some(part) if part.item.is_some() => vec![("Unequip", Select)],
                Some(_) => vec![("Choose what to equip", Select)],
                None => Vec::new(),
            },
            (Tab::Inventory, _) => match contents.inventory.get(self.selected) {
                Some(item) if item.ammo => {
                    vec![("Nock", Nock), ("Put in quiver", Quiver), ("Drop", Drop)]
                }
                Some(item) if item.lesson => vec![("Study", Study), ("Drop", Drop)],
                Some(_) => vec![("Equip", Select), ("Drop", Drop)],
                None => Vec::new(),
            },
            (Tab::Styles, _) => match contents.styles.get(self.selected) {
                Some(style) if style.followed => vec![("Set aside", Select)],
                Some(_) => vec![("Follow", Select)],
                None => Vec::new(),
            },
            (Tab::Skills, _) => Vec::new(),
        }
    }
}

impl Overlay {
    pub fn view(&self, contents: &Contents) -> PanelView {
        let picking_part = self
            .picking
            .and_then(|picking| contents.equipment.iter().find(|p| p.index == picking.part));
        let (title, mut lines, focus, hint) = match (self.tab, self.picking, picking_part) {
            (Tab::Equipment, Some(picking), Some(part)) => {
                let fits: Vec<ItemView> = fitting(contents.inventory, part).cloned().collect();
                (
                    Title::Text(format!("Equip on {}", part.name)),
                    item_lines(&fits, picking.selected),
                    Some(picking.selected),
                    "Enter equip  Esc back",
                )
            }
            (Tab::Equipment, _, _) => (
                Title::Tabs(Tab::Equipment),
                part_lines(contents.equipment, self.selected),
                Some(self.selected),
                "Enter unequip / choose  Tab skills  Esc close",
            ),
            (Tab::Skills, _, _) => {
                let (lines, focus) = skill_lines(contents, self.selected);
                (
                    Title::Tabs(Tab::Skills),
                    lines,
                    focus,
                    "j/k scroll  Tab styles  Esc close",
                )
            }
            (Tab::Styles, _, _) => {
                let (lines, focus) = way_lines(contents, self.selected);
                (
                    Title::Tabs(Tab::Styles),
                    lines,
                    focus,
                    if contents.styles.is_empty() {
                        "Tab inventory  Esc close"
                    } else {
                        "Enter follow / set aside  Tab inventory  Esc close"
                    },
                )
            }
            (Tab::Inventory, _, _) => {
                let chosen = contents.inventory.get(self.selected);
                let ammo = chosen.is_some_and(|item| item.ammo);
                let lesson = chosen.is_some_and(|item| item.lesson);
                (
                    Title::Tabs(Tab::Inventory),
                    item_lines(contents.inventory, self.selected),
                    Some(self.selected),
                    if ammo {
                        "n nock  v quiver  d drop  Esc close"
                    } else if lesson {
                        "r study  d drop  Tab equipment  Esc close"
                    } else {
                        "Enter equip  d drop  Tab equipment  Esc close"
                    },
                )
            }
        };
        if let Some(notice) = self.notice {
            lines.push(PanelLine::plain(Vec::new()));
            lines.push(PanelLine::plain(vec![Seg::Plain(notice.to_string())]));
        }
        PanelView {
            title,
            lines,
            focus,
            hint,
        }
    }
}

fn quiet(text: impl Into<String>) -> Seg {
    Seg::Quiet(text.into())
}

fn item_lines(items: &[ItemView], selected: usize) -> Vec<PanelLine> {
    if items.is_empty() {
        return vec![PanelLine::plain(vec![Seg::Plain(
            "You carry nothing.".to_string(),
        )])];
    }
    let selected = selected.min(items.len() - 1);
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let mut row = text::item(item);
            if let Some(kind) = item.fits {
                row.push(quiet(format!("  ({})", kind.noun())));
            }
            if !item.trains.is_empty() {
                row.push(quiet(format!(
                    "  trains {}",
                    item.trains.join(", ").to_lowercase()
                )));
            }
            PanelLine::row(row, i, selected)
        })
        .collect()
}

/// The skills under a heading per domain: name, level, and XP toward the
/// next level, with the selected skill's line to keep in view.
fn skill_lines(contents: &Contents, selected: usize) -> (Vec<PanelLine>, Option<usize>) {
    let skills = contents.skills;
    if skills.is_empty() {
        return (
            vec![PanelLine::plain(vec![Seg::Plain(
                "You have learned nothing yet.".to_string(),
            )])],
            None,
        );
    }
    let selected = selected.min(skills.len() - 1);
    let mut focus = None;
    let width = skills
        .iter()
        .map(|skill| skill.name.chars().count())
        .max()
        .unwrap_or(0);
    let mut lines = Vec::new();
    let mut domain = None;
    for (i, skill) in skills.iter().enumerate() {
        if domain != Some(skill.domain) {
            domain = Some(skill.domain);
            lines.push(PanelLine::heading(domain_name(skill.domain)));
        }
        let progress = match skill.next {
            Some(next) => format!("  {}/{next} xp", skill.xp),
            None => "  mastered".to_string(),
        };
        let row = vec![
            Seg::Plain(format!("  {:<width$}  {:>2}", skill.name, skill.level)),
            quiet(progress),
        ];
        if i == selected {
            focus = Some(lines.len());
        }
        lines.push(PanelLine::row(row, i, selected));
    }
    (lines, focus)
}

/// What the player has made of their skills: styles (the selected one
/// focused), proficiencies, techniques and half-formed ideas.
fn way_lines(contents: &Contents, selected: usize) -> (Vec<PanelLine>, Option<usize>) {
    let none = |text: &str| PanelLine::plain(vec![quiet(format!("  {text}"))]);
    let mut lines = vec![PanelLine::heading("Styles")];
    let mut focus = None;
    if contents.styles.is_empty() {
        lines.push(none("None discovered yet."));
    }
    let selected = selected.min(contents.styles.len().saturating_sub(1));
    for (i, style) in contents.styles.iter().enumerate() {
        let following = if style.followed { "  following" } else { "" };
        let row = vec![
            Seg::Plain(format!("  {}  {:>2}", style.name, style.level)),
            quiet(following),
        ];
        if i == selected {
            focus = Some(lines.len());
        }
        lines.push(PanelLine::row(row, i, selected));
    }
    lines.push(PanelLine::heading("Proficiencies"));
    if contents.proficiencies.is_empty() {
        lines.push(none("None attained yet."));
    }
    lines.extend(
        contents
            .proficiencies
            .iter()
            .map(|p| PanelLine::plain(vec![Seg::Plain(format!("  {p}"))])),
    );
    lines.push(PanelLine::heading("Techniques"));
    if contents.techniques.is_empty() {
        lines.push(none("None known yet."));
    }
    lines.extend(contents.techniques.iter().map(|technique| {
        let (main, notes) = crate::targeting::technique_text(technique);
        let mut row = vec![Seg::Plain(format!("  {main}"))];
        row.extend(notes.into_iter().map(|note| quiet(format!("  {note}"))));
        PanelLine::plain(row)
    }));
    if !contents.ideas.is_empty() {
        lines.push(PanelLine::heading("Ideas"));
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
            lines.push(PanelLine::plain(vec![
                Seg::Plain("  ? ".to_string()),
                quiet(format!("{}{more}", met.join(", "))),
            ]));
        }
    }
    (lines, focus)
}

fn domain_name(domain: Domain) -> &'static str {
    match domain {
        Domain::Martial => "Martial",
        Domain::Defence => "Defence",
        Domain::Magic => "Magic",
        Domain::Lore => "Lore",
    }
}

/// One line per body part: its name, then what it holds or a dash.
fn part_lines(parts: &[PartView], selected: usize) -> Vec<PanelLine> {
    if parts.is_empty() {
        return vec![PanelLine::plain(vec![Seg::Plain(
            "You have no body to equip.".to_string(),
        )])];
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
            let mut row = vec![Seg::Plain(format!("{:<width$}  ", part.name))];
            match &part.item {
                Some(item) => row.extend(text::item(item)),
                None => row.push(quiet("-")),
            }
            PanelLine::row(row, i, selected)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::state;

    #[test]
    fn focusing_a_row_selects_it_and_offers_its_actions() {
        let views = Views::of(&state());
        let contents = views.contents();
        let mut overlay = Overlay::new(Tab::Equipment);
        let held = contents
            .equipment
            .iter()
            .position(|part| part.item.is_some())
            .unwrap();
        assert_eq!(overlay.handle(OverlayInput::Focus(held), &contents), None);
        assert_eq!(overlay.selected(), held);
        assert_eq!(
            overlay.actions(&contents),
            [("Unequip", OverlayInput::Select)]
        );
        // Past the end is clamped, as a shrunken list would be.
        overlay.handle(OverlayInput::Focus(99), &contents);
        assert_eq!(overlay.selected(), contents.equipment.len() - 1);
    }

    #[test]
    fn panel_lines_carry_the_row_they_focus() {
        let views = Views::of(&state());
        let contents = views.contents();
        let view = Overlay::new(Tab::Equipment).view(&contents);
        let indices: Vec<usize> = view.lines.iter().filter_map(|line| line.index).collect();
        assert_eq!(indices, (0..contents.equipment.len()).collect::<Vec<_>>());
        assert_eq!(view.lines[0].kind, LineKind::Selected);
    }
}
