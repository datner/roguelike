//! Origins (docs/plans/origins.md, ADR 0025): what a character is when it starts, in place of a class. An origin names the blueprint the character is made from, which inherits `Player` and adds the kit, skills and techniques, and the styles it starts knowing. Listed in `data/origins.ron`; the first is the default.

use std::path::Path;

use hecs::Entity;
use serde::Deserialize;

use crate::blueprint::{BlueprintError, Blueprints, ComponentDef, ron_options};
use crate::entity::{Origin, Position};
use crate::spawn::Spawn;
use crate::state::GameState;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OriginDef {
    /// How saves and the menu's answer name it.
    pub key: String,
    /// What the player reads: "Wanderer".
    pub name: String,
    /// One line on who this is, for choosing.
    pub summary: String,
    /// The blueprint the character is made from; it must be a `Player`.
    pub blueprint: String,
    /// Style blueprints known from the start, carried.
    #[serde(default)]
    pub styles: Vec<String>,
    /// One of `styles`, followed from the start.
    #[serde(default)]
    pub follow: Option<String>,
}

/// `origins.ron` in `data`: at least one origin, keys unique. Checked against
/// the blueprints by `check`.
pub(crate) fn read(data: &Path) -> Result<Vec<OriginDef>, BlueprintError> {
    let path = data.join("origins.ron");
    let text =
        std::fs::read_to_string(&path).map_err(|err| BlueprintError::Io(path.clone(), err))?;
    ron_options()
        .from_str(&text)
        .map_err(|err| BlueprintError::Parse(path.clone(), Box::new(err)))
}

/// Why `origins` can't be played with `blueprints`, if they can't.
pub(crate) fn check(origins: &[OriginDef], blueprints: &Blueprints) -> Result<(), BlueprintError> {
    let bad = |key: &str, why: String| BlueprintError::BadOrigins(format!("{key:?} {why}"));
    if origins.is_empty() {
        return Err(BlueprintError::BadOrigins(
            "there must be at least one origin".into(),
        ));
    }
    for (i, origin) in origins.iter().enumerate() {
        let key = origin.key.as_str();
        if origins[..i].iter().any(|other| other.key == key) {
            return Err(bad(key, "is listed twice".into()));
        }
        let is_player = blueprints
            .get(&origin.blueprint)
            .is_some_and(|bp| bp.components.contains(&ComponentDef::Player));
        if !is_player {
            return Err(bad(
                key,
                format!("names {:?}, not a player blueprint", origin.blueprint),
            ));
        }
        if let Some(style) = origin
            .styles
            .iter()
            .find(|name| blueprints.get(name).is_none_or(|bp| bp.style.is_none()))
        {
            return Err(bad(key, format!("names {style:?}, not a style")));
        }
        if let Some(follow) = &origin.follow
            && !origin.styles.contains(follow)
        {
            return Err(bad(
                key,
                format!("follows {follow:?}, not one of its styles"),
            ));
        }
    }
    Ok(())
}

/// An origin as the new-game chooser shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginView {
    pub key: String,
    pub name: String,
    pub summary: String,
    /// What it starts with, a line per kind in plain words ("Carries: short
    /// bow, quiver, torch"); kinds it has none of are left out.
    pub starts_with: Vec<String>,
}

/// Every origin in menu order, read from the content, so every frontend
/// says the same about them.
pub fn origins(blueprints: &Blueprints) -> Vec<OriginView> {
    let noun = |name: &str| -> Option<String> { Some(blueprints.get(name)?.noun()?.to_string()) };
    let nouns = |ids: &[crate::blueprint::BlueprintId]| -> Vec<String> {
        ids.iter()
            .filter_map(|&id| Some(blueprints.by_id(id)?.noun()?.to_string()))
            .collect()
    };
    blueprints
        .origins()
        .iter()
        .map(|def| {
            let mut starts_with = Vec::new();
            let mut add = |label: &str, things: Vec<String>| {
                if !things.is_empty() {
                    starts_with.push(format!("{label}: {}", things.join(", ")));
                }
            };
            if let Some(bp) = blueprints.get(&def.blueprint) {
                add("Carries", nouns(&bp.equipment));
                let skills = bp.skills.iter().flat_map(|skills| &skills.0);
                add(
                    "Skills",
                    skills
                        .filter_map(|&(id, xp)| {
                            let def = blueprints.skills().get(id)?;
                            Some(format!("{} {}", def.name, crate::skill::level(xp)))
                        })
                        .collect(),
                );
                let (abilities, learned): (Vec<_>, Vec<_>) = bp
                    .techniques
                    .iter()
                    .copied()
                    .partition(|&id| blueprints.by_id(id).is_some_and(|bp| bp.is_ability()));
                add("Abilities", nouns(&abilities));
                add("Techniques", nouns(&learned));
                add("Spells", nouns(&bp.spells));
            }
            add(
                "Styles",
                def.styles
                    .iter()
                    .filter_map(|style| {
                        let noun = noun(style)?;
                        Some(if def.follow.as_ref() == Some(style) {
                            format!("{noun} (followed)")
                        } else {
                            noun
                        })
                    })
                    .collect(),
            );
            OriginView {
                key: def.key.clone(),
                name: def.name.clone(),
                summary: def.summary.clone(),
                starts_with,
            }
        })
        .collect()
}

impl GameState {
    /// The player as `origin` (a key; `None` for the first) makes them, at
    /// `at`: its blueprint, then its styles. Content without origins (tests'
    /// blueprints) starts from `Player` itself. No RNG is drawn, so the
    /// world around them is the same whatever the origin.
    pub(crate) fn spawn_player(
        &mut self,
        origin: Option<&str>,
        at: Position,
    ) -> Result<Entity, BlueprintError> {
        let def = match origin {
            Some(key) => Some(
                self.blueprints
                    .origin(key)
                    .cloned()
                    .ok_or_else(|| BlueprintError::UnknownOrigin(key.to_string()))?,
            ),
            None => self.blueprints.origins().first().cloned(),
        };
        let Some(def) = def else {
            return self.spawn(Spawn::blueprint("Player").with(at));
        };
        let player = self.spawn(
            Spawn::blueprint(&def.blueprint)
                .with(at)
                .with(Origin(def.key.clone())),
        )?;
        for style in &def.styles {
            let spawn = Spawn::blueprint(style);
            self.spawn(if def.follow.as_ref() == Some(style) {
                spawn.attached_to(player)
            } else {
                spawn.carried_by(player)
            })?;
        }
        Ok(player)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::tests::test_blueprints;

    fn data() -> std::path::PathBuf {
        crate::testkit::data()
    }

    fn origin(key: &str, blueprint: &str) -> OriginDef {
        OriginDef {
            key: key.into(),
            name: key.into(),
            summary: String::new(),
            blueprint: blueprint.into(),
            styles: Vec::new(),
            follow: None,
        }
    }

    #[test]
    fn the_shipped_origins_load_and_the_first_is_the_default() {
        let blueprints = Blueprints::load(&data()).unwrap();
        let first = blueprints.origins().first().unwrap().clone();
        let state = GameState::new(3, blueprints).unwrap();
        assert_eq!(
            *state.world.get::<&Origin>(state.player).unwrap(),
            Origin(first.key)
        );
    }

    #[test]
    fn bad_origins_fail_at_load_with_their_reason() {
        let blueprints = test_blueprints();
        let refused = |origins: Vec<OriginDef>| {
            blueprints
                .clone()
                .with_origins(origins)
                .unwrap_err()
                .to_string()
        };
        assert_eq!(
            refused(Vec::new()),
            "data/origins.ron: there must be at least one origin"
        );
        assert_eq!(
            refused(vec![origin("A", "Player"), origin("A", "Player")]),
            "data/origins.ron: \"A\" is listed twice"
        );
        assert_eq!(
            refused(vec![origin("A", "Goblin")]),
            "data/origins.ron: \"A\" names \"Goblin\", not a player blueprint"
        );
        let mut styled = origin("A", "Player");
        styled.styles = vec!["Goblin".into()];
        assert_eq!(
            refused(vec![styled.clone()]),
            "data/origins.ron: \"A\" names \"Goblin\", not a style"
        );
        styled.styles.clear();
        styled.follow = Some("Goblin".into());
        assert_eq!(
            refused(vec![styled]),
            "data/origins.ron: \"A\" follows \"Goblin\", not one of its styles"
        );
    }

    /// The shipped content plus "Bare": the bare `Player`, knowing fencing
    /// and bow dancing and following fencing.
    fn with_bare() -> Blueprints {
        let blueprints = Blueprints::load(&data()).unwrap().with_camp(None);
        let mut origins = blueprints.origins().to_vec();
        let mut bare = origin("Bare", "Player");
        bare.styles = vec!["Fencing".into(), "BowDancing".into()];
        bare.follow = Some("Fencing".into());
        origins.push(bare);
        blueprints.with_origins(origins).unwrap()
    }

    fn style_names(state: &GameState, styles: Vec<Entity>) -> Vec<String> {
        styles
            .into_iter()
            .map(|style| {
                crate::grammar::Subject::of(&state.world, style)
                    .noun()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn an_origin_decides_the_kit_and_styles_and_nothing_else() {
        let wanderer = GameState::new_as(5, with_bare(), None).unwrap();
        let bare = GameState::new_as(5, with_bare(), Some("Bare")).unwrap();
        assert_eq!(
            *bare.world.get::<&Origin>(bare.player).unwrap(),
            Origin("Bare".into())
        );
        assert!(!crate::equipment::equipped(&wanderer.world, wanderer.player).is_empty());
        assert!(crate::equipment::equipped(&bare.world, bare.player).is_empty());
        assert_eq!(
            style_names(&bare, crate::style::known(&bare.world, bare.player)),
            ["the fencing", "the bow dancing"]
        );
        assert_eq!(
            style_names(&bare, crate::style::followed(&bare.world, bare.player)),
            ["the fencing"]
        );

        let world = |state: &GameState| {
            let mut placed: Vec<(i32, i32, String)> = state
                .world
                .query::<(&Position, &crate::entity::Name)>()
                .iter()
                .map(|(pos, name)| (pos.x, pos.y, name.0.clone()))
                .filter(|(_, _, name)| name != "player")
                .collect();
            placed.sort();
            placed
        };
        assert!(!world(&wanderer).is_empty());
        assert_eq!(
            world(&wanderer),
            world(&bare),
            "the same seed, the same world"
        );
    }

    #[test]
    fn an_origin_says_what_it_starts_with() {
        let views = origins(&with_bare());
        let wanderer = &views[0];
        assert_eq!(wanderer.name, "Wanderer");
        // What the wanderer carries, in the blueprint's order, by noun.
        let blueprints = with_bare();
        let kit: Vec<&str> = blueprints
            .get("Wanderer")
            .unwrap()
            .equipment
            .iter()
            .map(|&id| blueprints.by_id(id).unwrap().noun().unwrap())
            .collect();
        assert!(
            wanderer
                .starts_with
                .contains(&format!("Carries: {}", kit.join(", "))),
            "{:?}",
            wanderer.starts_with
        );
        assert!(
            wanderer
                .starts_with
                .contains(&"Techniques: trip".to_string())
        );
        let bare = views.last().unwrap();
        assert_eq!(
            bare.starts_with,
            ["Styles: fencing (followed), bow dancing"]
        );
    }

    /// The bare test room with the player at (1, 1), starting as `origin`.
    fn room_as(origin: &str) -> GameState {
        GameState::with_map(
            1,
            crate::rng::Rngs::new(1, 1),
            crate::map::tests::room(),
            Position { x: 1, y: 1 },
            Blueprints::load(&data()).unwrap(),
            Some(origin),
        )
        .unwrap()
    }

    fn nouns(state: &GameState, things: Vec<Entity>) -> Vec<String> {
        things
            .into_iter()
            .map(|thing| {
                crate::grammar::Subject::of(&state.world, thing)
                    .noun()
                    .to_string()
            })
            .collect()
    }

    fn equipped(state: &GameState) -> Vec<String> {
        let items = crate::equipment::equipped(&state.world, state.player);
        nouns(state, items.into_iter().map(|(item, _)| item).collect())
    }

    #[test]
    /// Every origin starts with exactly what its blueprint and its entry in
    /// `origins.ron` declare: compared with the data, so content can change.
    fn each_origin_starts_as_described() {
        let blueprints = Blueprints::load(&data()).unwrap();
        let the = |name: &str| {
            let noun = blueprints.get(name).and_then(|b| b.noun()).unwrap();
            format!("the {noun}")
        };
        let the_id = |id| {
            let noun = blueprints.by_id(id).and_then(|b| b.noun()).unwrap();
            format!("the {noun}")
        };
        let sorted = |mut list: Vec<String>| {
            list.sort();
            list
        };
        for origin in blueprints.origins() {
            let state = room_as(&origin.key);
            let declared = blueprints.get(&origin.blueprint).unwrap();
            let key = &origin.key;
            assert_eq!(
                sorted(equipped(&state)),
                sorted(declared.equipment.iter().map(|&id| the_id(id)).collect()),
                "{key}'s kit"
            );
            for &(skill, xp) in declared.skills.iter().flat_map(|skills| &skills.0) {
                assert_eq!(
                    crate::skill::xp(&state.world, state.player, skill),
                    xp,
                    "{key}'s skills"
                );
            }
            assert_eq!(
                nouns(&state, crate::view::known_spells(&state)),
                declared
                    .spells
                    .iter()
                    .map(|&id| the_id(id))
                    .collect::<Vec<_>>(),
                "{key}'s spells"
            );
            let player = state.player;
            assert_eq!(
                sorted(nouns(&state, crate::style::known(&state.world, player))),
                sorted(origin.styles.iter().map(|style| the(style)).collect()),
                "{key}'s known styles"
            );
            assert_eq!(
                nouns(&state, crate::style::followed(&state.world, player)),
                origin
                    .follow
                    .iter()
                    .map(|style| the(style))
                    .collect::<Vec<_>>(),
                "{key}'s followed style"
            );
        }
    }

    #[test]
    fn the_headbutt_is_an_ability_that_lands() {
        let mut state = room_as("PitFighter");
        let player = state.player;
        crate::testkit::see_everything(&mut state);
        crate::testkit::sure_hits(&mut state, player);
        let goblin = crate::testkit::thing("goblin")
            .at(2, 1)
            .health(30)
            .hostile()
            .make(&mut state);
        let views = crate::technique::techniques(&state);
        let headbutt = views.iter().find(|view| view.name == "headbutt").unwrap();
        assert!(headbutt.ability);
        assert!(views.iter().filter(|view| view.ability).count() == 1);

        crate::time::take_turn(
            &mut state,
            crate::input::PlayerAction::Perform {
                technique: headbutt.technique,
                at: (2, 1),
            },
        );
        let health = state.world.get::<&crate::entity::Health>(goblin).unwrap();
        assert!(health.current < 30);
        drop(health);
        let log: Vec<&str> = state.log.recent(10).map(String::as_str).collect();
        assert!(
            log.contains(&"You headbutt the goblin.")
                && log.contains(&"The goblin starts reeling."),
            "{log:?}"
        );
    }

    #[test]
    fn a_save_keeps_the_origin() {
        let blueprints = with_bare();
        let state = GameState::new_as(5, blueprints.clone(), Some("Bare")).unwrap();
        let loaded =
            crate::save::from_ron(&crate::save::to_ron(&state).unwrap(), blueprints).unwrap();
        assert_eq!(
            *loaded.world.get::<&Origin>(loaded.player).unwrap(),
            Origin("Bare".into())
        );
    }

    #[test]
    fn an_unknown_origin_is_refused() {
        let err = GameState::new_as(1, test_blueprints(), Some("Nobody")).err();
        assert_eq!(
            err.map(|err| err.to_string()).as_deref(),
            Some("no origin \"Nobody\"")
        );
    }
}
