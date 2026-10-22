//! Draws the hovering panel (`front::overlay`): a modal box over the map with the inventory, equipment, skills and styles tabs.

use front::overlay::{Contents, LineKind, Overlay, TABS, Tab, Title};
use front::text;
use game::ItemView;
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::widgets::{line, spans};

/// Half the map panel wide and three quarters high, at least 46×8 where it
/// fits, centred over `area`. A list longer than the panel scrolls so the
/// selected row stays in view.
pub fn draw(frame: &mut Frame, area: Rect, overlay: &Overlay, contents: &Contents) {
    let width = (area.width / 2).max(46).min(area.width);
    let height = (area.height * 3 / 4).max(8).min(area.height);
    let panel = area.centered(Constraint::Length(width), Constraint::Length(height));

    let view = overlay.view(contents);
    let title = match view.title {
        Title::Tabs(active) => tabs(active),
        Title::Text(text) => Line::from(text),
    };
    let rows: Vec<Line> = view
        .lines
        .into_iter()
        .map(|panel_line| {
            let row = line(panel_line.row);
            match panel_line.kind {
                LineKind::Plain => row,
                LineKind::Heading => row.patch_style(Style::new().add_modifier(Modifier::BOLD)),
                LineKind::Selected => {
                    row.patch_style(Style::new().add_modifier(Modifier::REVERSED))
                }
            }
        })
        .collect();
    let shown = usize::from(panel.height.saturating_sub(2)).max(1);
    let last_top = rows.len().saturating_sub(shown);
    let top = view
        .focus
        .map_or(0, |row| (row + 1).saturating_sub(shown).min(last_top));
    let block = Block::bordered().title(title).title_bottom(Line::styled(
        view.hint,
        Style::new().add_modifier(Modifier::DIM),
    ));
    frame.render_widget(Clear, panel);
    frame.render_widget(
        Paragraph::new(rows)
            .block(block)
            .scroll((u16::try_from(top).unwrap_or(u16::MAX), 0)),
        panel,
    );
}

/// Every tab name, the active one highlighted.
fn tabs(active: Tab) -> Line<'static> {
    let mut row = Vec::new();
    for (i, (tab, name)) in TABS.into_iter().enumerate() {
        if i > 0 {
            row.push(Span::raw(" "));
        }
        let style = if tab == active {
            Style::new().add_modifier(Modifier::REVERSED)
        } else {
            Style::new().add_modifier(Modifier::DIM)
        };
        row.push(Span::styled(format!(" {name} "), style));
    }
    Line::from(row)
}

/// The item's glyph in its colour, then its name.
pub fn item_spans(item: &ItemView) -> Vec<Span<'static>> {
    spans(text::item(item))
}

#[cfg(test)]
mod tests {
    use front::input::OverlayInput;
    use front::overlay::{OverlayAction, Tab};
    use game::{
        Domain, IdeaView, ItemRef, PartKind, PartView, PlayerAction, Rgb, SkillView, SpawnSeq,
        StyleView,
    };
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
                lesson: false,
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
            styles: &[],
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
            styles: &[],
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
            styles: &[],
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
            styles: &[],
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
                    styles: &[],
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
            styles: &[],
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
                styles: &[],
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
            styles: &[],
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
            styles: &[],
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
            styles: &[],
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
            styles: &[],
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
            styles: &[],
        };
        let mut overlay = Overlay::new(Tab::Inventory);
        assert_eq!(overlay.handle(OverlayInput::Equipment, &contents), None);
        let shown = screen(&overlay, &contents);
        assert!(shown.contains("right hand  / sword"), "{shown}");
        assert!(shown.contains("left hand   -"));
        assert_eq!(overlay.handle(OverlayInput::SwitchTab, &contents), None);
        assert!(screen(&overlay, &contents).contains("You have learned nothing yet."));
        assert_eq!(overlay.handle(OverlayInput::SwitchTab, &contents), None);
        assert!(screen(&overlay, &contents).contains("None discovered yet."));
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
            styles: &[],
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
            styles: &[],
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
            styles: &[],
        };
        assert!(screen(&Overlay::new(Tab::Inventory), &contents).contains("trains slash, dagger"));
    }

    #[test]
    fn the_styles_tab_lists_proficiencies_techniques_and_half_formed_ideas() {
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
            styles: &[],
        };
        let shown = screen(&Overlay::new(Tab::Styles), &contents);
        for line in [
            "Proficiencies",
            "  footwork",
            "Techniques",
            "None known yet.",
            "Ideas",
            "Dodge 4 + 1 more",
        ] {
            assert!(shown.contains(line), "{line:?} in\n{shown}");
        }
        assert!(!shown.contains("Fencing"), "an idea's name stays hidden");
    }

    #[test]
    fn the_styles_tab_follows_the_selected_style() {
        let styles = [
            StyleView {
                style: ItemRef(SpawnSeq(7)),
                name: "fencing".into(),
                level: 2,
                followed: true,
            },
            StyleView {
                style: ItemRef(SpawnSeq(8)),
                name: "brawling".into(),
                level: 0,
                followed: false,
            },
        ];
        let skills = [skill("Melee", Domain::Martial, 1, 60, Some(200))];
        let contents = Contents {
            inventory: &[],
            equipment: &[],
            skills: &skills,
            proficiencies: &[],
            ideas: &[],
            styles: &styles,
        };
        let mut overlay = Overlay::new(Tab::Styles);
        let shown = screen(&overlay, &contents);
        assert!(shown.contains("fencing   2  following"), "{shown}");
        assert!(shown.contains("brawling   0"));
        assert_eq!(overlay.handle(OverlayInput::Down, &contents), None);
        assert_eq!(
            overlay.handle(OverlayInput::Select, &contents),
            Some(OverlayAction::Act(PlayerAction::FollowStyle(ItemRef(
                SpawnSeq(8)
            ))))
        );
    }

    #[test]
    fn r_studies_a_selected_lesson_and_nothing_else() {
        let mut inventory = items(&["knife", "manual"]);
        inventory[1].lesson = true;
        let contents = Contents {
            inventory: &inventory,
            equipment: &[],
            skills: &[],
            proficiencies: &[],
            ideas: &[],
            styles: &[],
        };
        let mut overlay = Overlay::new(Tab::Inventory);
        assert_eq!(overlay.handle(OverlayInput::Study, &contents), None);
        overlay.handle(OverlayInput::Down, &contents);
        assert!(screen(&overlay, &contents).contains("r study"));
        assert_eq!(
            overlay.handle(OverlayInput::Study, &contents),
            Some(OverlayAction::Act(PlayerAction::Study(inventory[1].item)))
        );
    }

    #[test]
    fn long_views_scroll_and_keep_the_selection_in_sight() {
        let names: Vec<String> = (0..30).map(|i| format!("Skill{i:02}")).collect();
        let skills: Vec<SkillView> = names
            .iter()
            .map(|name| skill(name, Domain::Martial, 1, 60, Some(200)))
            .collect();
        let styles: Vec<StyleView> = (0..20)
            .map(|i| StyleView {
                style: ItemRef(SpawnSeq(i)),
                name: format!("style{i:02}"),
                level: 0,
                followed: false,
            })
            .collect();
        let contents = Contents {
            inventory: &[],
            equipment: &[],
            skills: &skills,
            proficiencies: &[],
            ideas: &[],
            styles: &styles,
        };
        let mut overlay = Overlay::new(Tab::Skills);
        assert!(!screen(&overlay, &contents).contains("Skill29"));
        for _ in 0..40 {
            overlay.handle(OverlayInput::Down, &contents);
        }
        let bottom = screen(&overlay, &contents);
        assert!(
            bottom.contains("Skill29") && !bottom.contains("Skill00"),
            "{bottom}"
        );
        for _ in 0..40 {
            overlay.handle(OverlayInput::Up, &contents);
        }
        assert!(screen(&overlay, &contents).contains("Skill00"));

        let mut overlay = Overlay::new(Tab::Styles);
        for _ in 0..19 {
            overlay.handle(OverlayInput::Down, &contents);
        }
        assert!(screen(&overlay, &contents).contains("style19"));
    }
}
