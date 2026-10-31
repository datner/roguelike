//! Data-driven entity definitions, loaded from `data/entities/*.ron`. A
//! blueprint's name is its file stem. Inheritance is resolved once at load
//! time, so spawning never walks the parent chain.
//!
//! Inheritance follows one rule: the child's field wins. Each component
//! variant counts as its own field (a child's `Health` replaces the parent's,
//! other parent components are kept), and `equipment` is a single field
//! (absent inherits the parent's list, present replaces it, `[]` clears it).
//!
//! Every name a blueprint refers to (`inherits`, `equipment`, on-hit effects)
//! is checked and turned into a `BlueprintId` at load, so a typo fails
//! loading instead of surfacing mid-game.

use crate::camp::{self, Camp};
use crate::dungeon::{self, Dungeon};
use crate::origin::OriginDef;
use crate::phase::{self, Phase};
use crate::tiles::Tiles;
use std::collections::BTreeMap;
use std::fs;
use std::mem::discriminant;
use std::path::{Path, PathBuf};

use hecs::{Entity, EntityBuilder, World};
use ron::extensions::Extensions;
use serde::{Deserialize, Serialize};

use crate::discovery::{self, Discovery, DiscoveryDef};
use crate::entity::{
    Ability, Hardiness, Keepsake, Made, Natural, Note, OffBalance, Roams, Skirmish, SprintAi,
    Teaches, TechniqueAi, Verb, WindUp,
};
use crate::entity::{
    Ammo, Appearance, Area, Armor, Binding, Blast, Bleeding, BlocksMovement, BloodCasting, Blow,
    Body, BodyPart, Chain, CooldownCasting, Dash, Deflect, Delay, Duration, Equippable, Fizzle,
    Focus, Footwork, Graze, Health, Hostile, Item, Launcher, Learner, Lesson, Light, Loose, Mana,
    ManaCasting, Melee, Morale, Name, OnHit, OnHitEntry, PartKind, Player, Proficiency, Projectile,
    Quiver, Regeneration, School, Scroll, Skills, Speed, Spell, SpellAi, SpellDamage, Stamina,
    Stats, Style, Summon, SummonLimit, Technique, Throwable, Trains, Weapon,
};
use crate::equipment::free_part;
use crate::holding;
use crate::skill::{SkillBook, xp_for};
use crate::style::StyleDef;
use crate::technique::{Missing, Names, TechniqueDef};

/// A blueprint's index in the `Blueprints` it was loaded into. Runtime-only:
/// ids depend on which files were loaded, so it has no serde derives and
/// anything persisted stores the blueprint's name instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlueprintId(u32);

/// Every component a blueprint can declare: the `plain`, `markers` and
/// `resolved` lists of `entity::with_components!`. A `resolved` one holds
/// names that loading resolves to ids, so it is spawned from
/// `ResolvedBlueprint`, never added here.
///
/// A child's entry overrides the parent's entry of the same variant.
macro_rules! component_defs {
    (
        plain: [$($(#[$plain_doc:meta])* $plain:ident),* $(,)?],
        state: [$($_state:tt)*],
        markers: [$($(#[$marker_doc:meta])* $marker:ident),* $(,)?],
        state_markers: [$($_state_marker:tt)*],
        resolved: [$($(#[$resolved_doc:meta])* $resolved:ident($ty:ty)),* $(,)?],
        custom: [$($_custom:tt)*] $(,)?
    ) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub enum ComponentDef {
            $($(#[$plain_doc])* $plain($plain),)*
            $($(#[$marker_doc])* $marker,)*
            $($(#[$resolved_doc])* $resolved($ty),)*
        }

        impl ComponentDef {
            fn add_to(&self, builder: &mut EntityBuilder) {
                match self {
                    $(ComponentDef::$plain(c) => {
                        builder.add(c.clone());
                    })*
                    $(ComponentDef::$marker => {
                        builder.add($marker);
                    })*
                    $(ComponentDef::$resolved(_))|* => {}
                }
            }
        }
    };
}

crate::entity::with_components!(component_defs);

/// A `Summon` as written in RON: the creature by blueprint name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummonDef {
    pub blueprint: String,
    pub count: u32,
    pub lifetime: u64,
}

/// An `OnHitEntry` as written in RON: the effect by blueprint name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnHitDef {
    pub effect: String,
    pub chance_bp: u32,
    /// Whether a graze applies it too (`OnHitEntry::on_graze`).
    #[serde(default)]
    pub on_graze: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Blueprint {
    pub inherits: Option<String>,
    pub components: Vec<ComponentDef>,
    /// Blueprint names spawned as separate entities and attached to this one.
    /// `None` inherits the parent's list; `Some` (even empty) replaces it.
    pub equipment: Option<Vec<String>>,
    /// Spells the creature knows, spawned as separate entities and attached
    /// (ADR 0014). `None` inherits the parent's; `Some` (even empty) replaces.
    pub spells: Option<Vec<String>>,
    /// Techniques the creature knows (ADR 0024), spawned and attached like
    /// `spells`, and inherited or replaced the same way.
    pub techniques: Option<Vec<String>>,
}

impl Blueprint {
    /// The equipment list, treating `None` as empty.
    pub fn equipment(&self) -> &[String] {
        self.equipment.as_deref().unwrap_or_default()
    }
}

/// A blueprint after load: inheritance flattened and every reference an id.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedBlueprint {
    pub name: String,
    /// Every declared component except `OnHit` and `Summon`, which hold
    /// names and live in `on_hit` and `summon` with ids.
    pub components: Vec<ComponentDef>,
    pub on_hit: Option<OnHit>,
    pub summon: Option<Summon>,
    pub skills: Option<Skills>,
    pub trains: Option<Trains>,
    pub style: Option<Style>,
    pub lesson: Option<Lesson>,
    pub equipment: Vec<BlueprintId>,
    /// The `Body` part each `equipment` entry is spawned on, index for index.
    pub equipped_on: Vec<u32>,
    pub spells: Vec<BlueprintId>,
    pub technique: Option<Technique>,
    pub techniques: Vec<BlueprintId>,
    pub teaches: Option<Teaches>,
}

impl ResolvedBlueprint {
    /// Whether it comes only from an origin (ADR 0025).
    pub fn is_ability(&self) -> bool {
        self.components.contains(&ComponentDef::Ability)
    }

    pub fn body(&self) -> &[BodyPart] {
        self.components
            .iter()
            .find_map(|c| match c {
                ComponentDef::Body(body) => Some(body.0.as_slice()),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// How long an effect made from this lasts, in ticks.
    pub fn duration(&self) -> Option<u64> {
        self.components.iter().find_map(|c| match c {
            ComponentDef::Duration(Duration(ticks)) => Some(*ticks),
            _ => None,
        })
    }

    /// What things made from this are called (their `Name`).
    pub fn noun(&self) -> Option<&str> {
        self.components.iter().find_map(|c| match c {
            ComponentDef::Name(name) => Some(name.0.as_str()),
            _ => None,
        })
    }

    pub fn equippable(&self) -> Option<PartKind> {
        self.components.iter().find_map(|c| match c {
            ComponentDef::Equippable(e) => Some(e.part),
            _ => None,
        })
    }
}

/// A loaded, validated set of blueprints.
#[derive(Debug, Clone, Default)]
pub struct Blueprints {
    /// Indexed by `BlueprintId`; ids follow name order.
    resolved: Vec<ResolvedBlueprint>,
    ids: BTreeMap<String, BlueprintId>,
    /// The skill list (`data/skills.ron`), which blueprints name skills from.
    skills: SkillBook,
    /// What skill combinations inspire (`data/discoveries.ron`, ADR 0018).
    discoveries: Vec<Discovery>,
    /// Sprites (`data/tiles.ron`, ADR 0023).
    tiles: Tiles,
    /// The dungeons (`data/dungeons/`), by file stem: how their floors are
    /// generated and filled, so a floor can be made mid-game and after a
    /// load.
    dungeons: BTreeMap<String, Dungeon>,
    /// Where a new game starts and dives end (`data/camp.ron`); without
    /// one, a new game starts on the first dungeon's first floor.
    camp: Option<Camp>,
    /// The phases the world turns through, in order (`data/phases.ron`).
    phases: Vec<Phase>,
    /// What a character can start as, in menu order (`data/origins.ron`).
    origins: Vec<OriginDef>,
    /// How many lives a lineage keeps and what time leaves of them
    /// (`data/legacy.ron`).
    legacy: crate::legacy::Legacy,
}

#[derive(Debug)]
pub enum BlueprintError {
    Io(PathBuf, std::io::Error),
    Parse(PathBuf, Box<ron::error::SpannedError>),
    UnknownBlueprint(String),
    UnknownParent {
        child: String,
        parent: String,
    },
    UnknownEquipment {
        owner: String,
        item: String,
    },
    UnknownEffect {
        owner: String,
        effect: String,
    },
    UnknownSummon {
        owner: String,
        summon: String,
    },
    UnknownSpell {
        owner: String,
        spell: String,
    },
    UnknownTechnique {
        owner: String,
        technique: String,
    },
    UnknownSkill {
        owner: String,
        skill: String,
    },
    /// `data/skills.ron` lists a skill twice.
    DuplicateSkill(String),
    /// `data/skills.ron` lacks a skill the code names (`skill::Core`).
    MissingSkill(String),
    TooManySkills,
    /// A discovery names a skill or proficiency blueprint that doesn't exist.
    UnknownInDiscovery {
        discovery: String,
        what: String,
    },
    EffectWithoutDuration {
        owner: String,
        effect: String,
    },
    /// Names along the cycle, starting and ending with the same blueprint.
    InheritanceCycle(Vec<String>),
    EquipmentCycle(Vec<String>),
    /// `item` can't be equipped, or `owner`'s `Body` has no free part for it.
    EquipmentDoesNotFit {
        owner: String,
        item: String,
    },
    /// Two of `owner`'s disciplines claim `school`; one must (ADR 0014).
    SchoolClaimedTwice {
        owner: String,
        school: School,
    },
    /// `owner` (a blueprint, or "terrain") names a sprite `tiles.ron` lacks.
    UnknownSprite {
        owner: String,
        sprite: String,
    },
    SpriteWithoutFrames(String),
    TooManySprites,
    BadDungeon {
        dungeon: String,
        why: String,
    },
    /// A new game needs a dungeon to start in.
    NoDungeon,
    BadCamp(String),
    BadPhases(String),
    BadOrigins(String),
    BadLegacy(String),
    /// A technique without the `Verb` its log needs.
    TechniqueWithoutVerb(String),
    /// A manual, a style's inventions or a discovery (`by`) hands out an
    /// ability, which only an origin gives (ADR 0025).
    AbilityHandedOut {
        by: String,
        ability: String,
    },
    /// A new game asked for an origin the content lacks.
    UnknownOrigin(String),
}

impl std::fmt::Display for BlueprintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(path, err) => write!(f, "{}: {err}", path.display()),
            Self::Parse(path, err) => write!(f, "{}:{err}", path.display()),
            Self::UnknownBlueprint(name) => write!(f, "unknown blueprint {name:?}"),
            Self::UnknownSprite { owner, sprite } => {
                write!(f, "{owner:?} names unknown sprite {sprite:?}")
            }
            Self::SpriteWithoutFrames(name) => write!(f, "sprite {name:?} has no frames"),
            Self::TooManySprites => write!(f, "too many sprites"),
            Self::BadDungeon { dungeon, why } => write!(f, "dungeon {dungeon:?}: {why}"),
            Self::NoDungeon => write!(f, "no dungeons in data/dungeons"),
            Self::BadCamp(why) => write!(f, "data/camp.ron: {why}"),
            Self::BadPhases(why) => write!(f, "data/phases.ron: {why}"),
            Self::BadOrigins(why) => write!(f, "data/origins.ron: {why}"),
            Self::BadLegacy(why) => write!(f, "data/legacy.ron: {why}"),
            Self::UnknownOrigin(key) => write!(f, "no origin {key:?}"),
            Self::TechniqueWithoutVerb(name) => {
                write!(f, "technique {name:?} has no Verb to be logged with")
            }
            Self::AbilityHandedOut { by, ability } => write!(
                f,
                "{by} hands out the ability {ability:?}; abilities come only from origins"
            ),
            Self::UnknownParent { child, parent } => {
                write!(f, "blueprint {child:?} inherits from unknown {parent:?}")
            }
            Self::UnknownEquipment { owner, item } => {
                write!(f, "blueprint {owner:?} is equipped with unknown {item:?}")
            }
            Self::UnknownEffect { owner, effect } => {
                write!(
                    f,
                    "blueprint {owner:?} applies unknown effect {effect:?} on hit"
                )
            }
            Self::UnknownSpell { owner, spell } => {
                write!(f, "blueprint {owner:?} knows unknown spell {spell:?}")
            }
            Self::UnknownTechnique { owner, technique } => {
                write!(
                    f,
                    "blueprint {owner:?} knows unknown technique {technique:?}"
                )
            }
            Self::UnknownSkill { owner, skill } => {
                write!(f, "blueprint {owner:?} names unknown skill {skill:?}")
            }
            Self::DuplicateSkill(name) => write!(f, "skill {name:?} is listed twice"),
            Self::MissingSkill(name) => write!(f, "the skill list lacks {name:?}"),
            Self::TooManySkills => write!(f, "too many skills"),
            Self::UnknownInDiscovery { discovery, what } => {
                write!(
                    f,
                    "discovery {discovery:?} names unknown {what:?} (or not a proficiency)"
                )
            }
            Self::UnknownSummon { owner, summon } => {
                write!(f, "blueprint {owner:?} summons unknown {summon:?}")
            }
            Self::EffectWithoutDuration { owner, effect } => write!(
                f,
                "blueprint {owner:?} applies effect {effect:?} on hit, which has no Duration"
            ),
            Self::InheritanceCycle(path) => write!(f, "inheritance cycle: {}", path.join(" -> ")),
            Self::EquipmentCycle(path) => write!(f, "equipment cycle: {}", path.join(" -> ")),
            Self::EquipmentDoesNotFit { owner, item } => {
                write!(
                    f,
                    "blueprint {owner:?} has no free body part for equipment {item:?}"
                )
            }
            Self::SchoolClaimedTwice { owner, school } => {
                write!(f, "blueprint {owner:?} has two disciplines for {school:?}")
            }
        }
    }
}

impl std::error::Error for BlueprintError {}

pub(crate) fn ron_options() -> ron::Options {
    ron::Options::default()
        .with_default_extension(Extensions::UNWRAP_VARIANT_NEWTYPES | Extensions::IMPLICIT_SOME)
}

impl Blueprints {
    /// Loads the game's content from `data`: the skill list in `skills.ron`
    /// and every blueprint in `entities/`.
    pub fn load(data: &Path) -> Result<Self, BlueprintError> {
        let skills = SkillBook::load(&data.join("skills.ron"))?;
        Self::resolve_with(Self::read_dir(&data.join("entities"))?, skills)?
            .with_discoveries(discovery::read(&data.join("discoveries.ron"))?)?
            .with_tiles(Tiles::load(data)?)?
            .with_origins(crate::origin::read(data)?)?
            .with_phases(phase::read(data)?)?
            .with_dungeons(dungeon::read_dir(&data.join("dungeons"))?)?
            .with_legacy(crate::legacy::read(data)?)
            .and_then(|blueprints| {
                let camp = camp::load(data, &blueprints)?;
                Ok(blueprints.with_camp(camp))
            })
    }

    /// Reads every `*.ron` file in `dir`, named by file stem.
    fn read_dir(dir: &Path) -> Result<BTreeMap<String, Blueprint>, BlueprintError> {
        let io_err = |err| BlueprintError::Io(dir.to_path_buf(), err);
        let mut raw = BTreeMap::new();
        for entry in fs::read_dir(dir).map_err(io_err)? {
            let path = entry.map_err(io_err)?.path();
            if path.extension().is_none_or(|ext| ext != "ron") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let name = name.to_string();
            let contents =
                fs::read_to_string(&path).map_err(|err| BlueprintError::Io(path.clone(), err))?;
            let blueprint = ron_options()
                .from_str(&contents)
                .map_err(|err| BlueprintError::Parse(path.clone(), Box::new(err)))?;
            raw.insert(name, blueprint);
        }
        Ok(raw)
    }

    /// `resolve_with` and no skills: for tests whose blueprints name none.
    pub fn resolve(raw: BTreeMap<String, Blueprint>) -> Result<Self, BlueprintError> {
        Self::resolve_with(raw, SkillBook::default())
    }

    /// Checks that every referenced name exists, flattens inheritance,
    /// assigns ids in name order, and rejects on-hit effects without a
    /// `Duration` and equipment cycles, so neither can fail during play.
    pub fn resolve_with(
        raw: BTreeMap<String, Blueprint>,
        skills: SkillBook,
    ) -> Result<Self, BlueprintError> {
        check_references(&raw)?;
        check_skills(&raw, &skills)?;
        let mut flat = BTreeMap::new();
        for name in raw.keys() {
            resolve_one(name, &raw, &mut flat, &mut Vec::new())?;
        }
        check_effect_durations(&raw, &flat)?;
        check_disciplines(&flat)?;

        let ids: BTreeMap<String, BlueprintId> =
            flat.keys().cloned().zip((0..).map(BlueprintId)).collect();
        let id_of = |name: &String| {
            ids.get(name)
                .copied()
                .ok_or_else(|| BlueprintError::UnknownBlueprint(name.clone()))
        };
        let mut resolved = Vec::with_capacity(flat.len());
        for (name, blueprint) in flat {
            let mut components = Vec::with_capacity(blueprint.components.len());
            let mut on_hit = None;
            let mut summon = None;
            let mut learned = None;
            let mut trains = None;
            let mut style = None;
            let mut lesson = None;
            let mut technique = None;
            let mut teaches = None;
            let skill_id = |skill: &String| {
                skills
                    .id(skill)
                    .ok_or_else(|| BlueprintError::UnknownSkill {
                        owner: name.clone(),
                        skill: skill.clone(),
                    })
            };
            for component in blueprint.components {
                match component {
                    ComponentDef::OnHit(entries) => {
                        let entries = entries
                            .iter()
                            .map(|entry| {
                                Ok(OnHitEntry {
                                    effect: id_of(&entry.effect)?,
                                    chance_bp: entry.chance_bp,
                                    on_graze: entry.on_graze,
                                })
                            })
                            .collect::<Result<_, BlueprintError>>()?;
                        on_hit = Some(OnHit(entries));
                    }
                    ComponentDef::Summon(def) => {
                        summon = Some(Summon {
                            blueprint: id_of(&def.blueprint)?,
                            count: def.count,
                            lifetime: def.lifetime,
                        });
                    }
                    ComponentDef::Skills(levels) => {
                        let mut xp = levels
                            .iter()
                            .map(|(name, &level)| Ok((skill_id(name)?, xp_for(level))))
                            .collect::<Result<Vec<_>, BlueprintError>>()?;
                        xp.sort();
                        learned = Some(Skills(xp));
                    }
                    ComponentDef::Trains(names) => {
                        let mut ids = names.iter().map(skill_id).collect::<Result<Vec<_>, _>>()?;
                        ids.sort();
                        ids.dedup();
                        trains = Some(Trains(ids));
                    }
                    ComponentDef::Lesson(grants) => {
                        let grants = grants
                            .iter()
                            .map(|(skill, &xp)| Ok((skill_id(skill)?, xp)))
                            .collect::<Result<Vec<_>, BlueprintError>>()?;
                        lesson = Some(Lesson(grants));
                    }
                    ComponentDef::Style(def) => {
                        let names = Names {
                            skills: &skills,
                            blueprint: &|name| ids.get(name).copied(),
                            blueprint_name: &|_| None,
                        };
                        style = Some(def.resolve(&names).map_err(|missing| match missing {
                            Missing::Blueprint(technique) => BlueprintError::UnknownTechnique {
                                owner: name.clone(),
                                technique,
                            },
                            Missing::Skill(skill) => BlueprintError::UnknownSkill {
                                owner: name.clone(),
                                skill,
                            },
                            Missing::SkillId(_) | Missing::BlueprintId(_) => {
                                BlueprintError::UnknownBlueprint(name.clone())
                            }
                        })?);
                    }
                    ComponentDef::Technique(def) => {
                        let names = Names {
                            skills: &skills,
                            blueprint: &|name| ids.get(name).copied(),
                            blueprint_name: &|_| None,
                        };
                        technique = Some(def.resolve(&names).map_err(|missing| match missing {
                            Missing::Skill(skill) => BlueprintError::UnknownSkill {
                                owner: name.clone(),
                                skill,
                            },
                            Missing::Blueprint(effect) => BlueprintError::UnknownEffect {
                                owner: name.clone(),
                                effect,
                            },
                            Missing::SkillId(_) | Missing::BlueprintId(_) => {
                                BlueprintError::UnknownBlueprint(name.clone())
                            }
                        })?);
                    }
                    ComponentDef::Teaches(names) => {
                        let ids = names.iter().map(id_of).collect::<Result<_, _>>()?;
                        teaches = Some(Teaches(ids));
                    }
                    other => components.push(other),
                }
            }
            let equipment = blueprint
                .equipment
                .iter()
                .flatten()
                .map(id_of)
                .collect::<Result<_, _>>()?;
            let spells = blueprint
                .spells
                .iter()
                .flatten()
                .map(id_of)
                .collect::<Result<_, _>>()?;
            let techniques = blueprint
                .techniques
                .iter()
                .flatten()
                .map(id_of)
                .collect::<Result<_, _>>()?;
            resolved.push(ResolvedBlueprint {
                name,
                components,
                on_hit,
                summon,
                skills: learned,
                trains,
                style,
                lesson,
                equipment,
                equipped_on: Vec::new(),
                spells,
                technique,
                techniques,
                teaches,
            });
        }
        check_equipment_cycles(&resolved)?;
        check_taught(&resolved)?;
        fit_equipment(&mut resolved)?;
        Ok(Self {
            resolved,
            ids,
            skills,
            discoveries: Vec::new(),
            tiles: Tiles::default(),
            dungeons: BTreeMap::new(),
            camp: None,
            phases: Vec::new(),
            origins: Vec::new(),
            legacy: crate::legacy::Legacy::default(),
        })
    }

    /// Ruins are built from `AbandonedCamp`, so keeping lives needs it.
    pub fn with_legacy(mut self, legacy: crate::legacy::Legacy) -> Result<Self, BlueprintError> {
        if legacy.keep > 0 && self.id("AbandonedCamp").is_none() {
            return Err(BlueprintError::BadLegacy(
                "lives are kept, so there must be an AbandonedCamp blueprint".into(),
            ));
        }
        self.legacy = legacy;
        Ok(self)
    }

    pub fn legacy(&self) -> &crate::legacy::Legacy {
        &self.legacy
    }

    /// Every skill there is.
    pub fn skills(&self) -> &SkillBook {
        &self.skills
    }

    /// These blueprints with `defs` as their discoveries, every name checked.
    pub fn with_discoveries(mut self, defs: Vec<DiscoveryDef>) -> Result<Self, BlueprintError> {
        self.discoveries = discovery::resolve(defs, &self)?;
        Ok(self)
    }

    /// Every sprite a blueprint names must be in `tiles`.
    pub fn with_tiles(mut self, tiles: Tiles) -> Result<Self, BlueprintError> {
        for blueprint in &self.resolved {
            for component in &blueprint.components {
                if let ComponentDef::Appearance(Appearance {
                    sprite: Some(sprite),
                    ..
                }) = component
                    && tiles.id(sprite).is_none()
                {
                    return Err(BlueprintError::UnknownSprite {
                        owner: blueprint.name.clone(),
                        sprite: sprite.clone(),
                    });
                }
            }
        }
        self.tiles = tiles;
        Ok(self)
    }

    pub fn tiles(&self) -> &Tiles {
        &self.tiles
    }

    /// Every dungeon is checked against these blueprints
    /// (`dungeon::check`).
    pub fn with_dungeons(
        mut self,
        dungeons: BTreeMap<String, Dungeon>,
    ) -> Result<Self, BlueprintError> {
        for (key, dungeon) in &dungeons {
            dungeon::check(key, dungeon, &self)?;
        }
        self.dungeons = dungeons;
        Ok(self)
    }

    /// Set before the dungeons, which name phases.
    /// A phase's attunement must be a skill there is.
    pub fn with_phases(mut self, phases: Vec<Phase>) -> Result<Self, BlueprintError> {
        for phase in &phases {
            if let Some(skill) = &phase.attunement
                && self.skills.id(skill).is_none()
            {
                return Err(BlueprintError::BadPhases(format!(
                    "{:?}'s attunement {skill:?} is no skill",
                    phase.key
                )));
            }
        }
        let known = |key: &String| phases.iter().any(|phase| &phase.key == key);
        let bad = |why: String| Err(BlueprintError::BadPhases(why));
        for blueprint in &self.resolved {
            for (key, _) in blueprint.style.iter().flat_map(|style| &style.thrives) {
                if !known(key) {
                    return bad(format!("{:?} thrives in no phase {key:?}", blueprint.name));
                }
            }
        }
        for discovery in &self.discoveries {
            if let Some(key) = discovery.phase.as_ref().filter(|key| !known(key)) {
                return bad(format!(
                    "discovery {:?} needs no phase {key:?}",
                    discovery.name
                ));
            }
        }
        self.phases = phases;
        Ok(self)
    }

    pub fn phases(&self) -> &[Phase] {
        &self.phases
    }

    /// These blueprints with `origins`, each checked (`origin::check`).
    pub fn with_origins(mut self, origins: Vec<OriginDef>) -> Result<Self, BlueprintError> {
        crate::origin::check(&origins, &self)?;
        self.origins = origins;
        Ok(self)
    }

    /// Every origin, in menu order; the first is the default.
    pub fn origins(&self) -> &[OriginDef] {
        &self.origins
    }

    pub fn origin(&self, key: &str) -> Option<&OriginDef> {
        self.origins.iter().find(|origin| origin.key == key)
    }

    /// The camp, checked against these blueprints' dungeons already.
    pub fn with_camp(mut self, camp: Option<Camp>) -> Self {
        self.camp = camp;
        self
    }

    pub fn camp(&self) -> Option<&Camp> {
        self.camp.as_ref()
    }

    pub fn dungeon(&self, key: &str) -> Option<&Dungeon> {
        self.dungeons.get(key)
    }

    /// Every dungeon, by key.
    pub fn dungeons(&self) -> impl Iterator<Item = (&str, &Dungeon)> {
        self.dungeons
            .iter()
            .map(|(key, dungeon)| (key.as_str(), dungeon))
    }

    /// Every blueprint, in id (name) order.
    pub(crate) fn all(&self) -> impl Iterator<Item = (BlueprintId, &ResolvedBlueprint)> {
        (0..)
            .zip(&self.resolved)
            .map(|(id, blueprint)| (BlueprintId(id), blueprint))
    }

    /// The dungeon a new game starts in, until there is a camp: the first
    /// by name.
    pub fn first_dungeon(&self) -> Option<&str> {
        self.dungeons.keys().next().map(String::as_str)
    }

    pub(crate) fn discoveries(&self) -> &[Discovery] {
        &self.discoveries
    }

    pub fn id(&self, name: &str) -> Option<BlueprintId> {
        self.ids.get(name).copied()
    }

    pub fn by_id(&self, id: BlueprintId) -> Option<&ResolvedBlueprint> {
        self.resolved.get(id.0 as usize)
    }

    pub fn get(&self, name: &str) -> Option<&ResolvedBlueprint> {
        self.by_id(self.id(name)?)
    }

    /// Every item that can be found lying about (an `Item`, not a
    /// `Keepsake`), in id (name) order so picking from it by index is
    /// deterministic.
    pub fn items(&self) -> Vec<BlueprintId> {
        (0..)
            .zip(&self.resolved)
            .filter(|(_, blueprint)| {
                blueprint.components.contains(&ComponentDef::Item)
                    && !blueprint.components.contains(&ComponentDef::Keepsake)
            })
            .map(|(id, _)| BlueprintId(id))
            .collect()
    }

    /// Spawns blueprint `id`, and each piece of its equipment as a separate
    /// entity put on the owner's body part (`holding::put_on`). Only for
    /// `GameState::spawn_id`, which gives them all a `SpawnSeq`. Equipment
    /// cycles are rejected at load, so the recursion ends.
    #[expect(
        clippy::disallowed_methods,
        reason = "the blueprint half of GameState::spawn_id, which registers what this creates"
    )]
    pub(crate) fn spawn_id(
        &self,
        world: &mut World,
        id: BlueprintId,
    ) -> Result<Entity, BlueprintError> {
        let blueprint = self
            .by_id(id)
            .ok_or_else(|| BlueprintError::UnknownBlueprint(format!("{id:?}")))?;

        let mut builder = EntityBuilder::new();
        builder.add(Made(blueprint.name.clone()));
        for component in &blueprint.components {
            component.add_to(&mut builder);
        }
        if let Some(on_hit) = &blueprint.on_hit {
            builder.add(on_hit.clone());
        }
        if let Some(summon) = blueprint.summon {
            builder.add(summon);
        }
        if let Some(skills) = &blueprint.skills {
            builder.add(skills.clone());
        }
        if let Some(trains) = &blueprint.trains {
            builder.add(trains.clone());
        }
        if let Some(style) = &blueprint.style {
            builder.add(style.clone());
        }
        if let Some(lesson) = &blueprint.lesson {
            builder.add(lesson.clone());
        }
        if let Some(technique) = &blueprint.technique {
            builder.add(technique.clone());
        }
        if let Some(teaches) = &blueprint.teaches {
            builder.add(teaches.clone());
        }
        let owner = world.spawn(builder.build());
        for (&item, &part) in blueprint.equipment.iter().zip(&blueprint.equipped_on) {
            let item = self.spawn_id(world, item)?;
            holding::put_on(world, owner, item, part);
        }
        for &spell in &blueprint.spells {
            let spell = self.spawn_id(world, spell)?;
            holding::attach(world, owner, spell);
        }
        for &technique in &blueprint.techniques {
            let technique = self.spawn_id(world, technique)?;
            holding::attach(world, owner, technique);
        }
        Ok(owner)
    }
}

/// Every skill a blueprint names exists. Reported against the blueprint
/// that wrote it, like `check_references`.
fn check_skills(raw: &BTreeMap<String, Blueprint>, book: &SkillBook) -> Result<(), BlueprintError> {
    for (owner, blueprint) in raw {
        let named = blueprint.components.iter().flat_map(|c| match c {
            ComponentDef::Skills(levels) => levels.keys().collect::<Vec<_>>(),
            ComponentDef::Trains(names) => names.iter().collect(),
            ComponentDef::Style(def) => def.names().collect(),
            ComponentDef::Technique(def) => def.names().collect(),
            ComponentDef::Lesson(grants) => grants.keys().collect(),
            _ => Vec::new(),
        });
        for skill in named {
            if book.id(skill).is_none() {
                return Err(BlueprintError::UnknownSkill {
                    owner: owner.clone(),
                    skill: skill.clone(),
                });
            }
        }
    }
    Ok(())
}

/// On-hit effects are applied with `ApplyEffect`, which needs a `Duration`
/// (possibly inherited, hence checked on `flat`). Reported against the
/// blueprint that wrote the entry, like `check_references`.
fn check_effect_durations(
    raw: &BTreeMap<String, Blueprint>,
    flat: &BTreeMap<String, Blueprint>,
) -> Result<(), BlueprintError> {
    for (owner, blueprint) in raw {
        let effects = blueprint
            .components
            .iter()
            .flat_map(|component| match component {
                ComponentDef::OnHit(entries) => entries.iter().map(|e| &e.effect).collect(),
                ComponentDef::Technique(def) => def.effects().collect(),
                _ => Vec::new(),
            });
        for effect in effects {
            let has_duration = flat.get(effect).is_some_and(|effect| {
                effect
                    .components
                    .iter()
                    .any(|c| matches!(c, ComponentDef::Duration(_)))
            });
            if !has_duration {
                return Err(BlueprintError::EffectWithoutDuration {
                    owner: owner.clone(),
                    effect: effect.clone(),
                });
            }
        }
    }
    Ok(())
}

/// What a manual teaches, a creature knows and a style invents must be
/// techniques; what is taught or invented must not be an ability.
fn check_taught(resolved: &[ResolvedBlueprint]) -> Result<(), BlueprintError> {
    for blueprint in resolved {
        let taught = blueprint.teaches.iter().flat_map(|teaches| &teaches.0);
        let invented = blueprint
            .style
            .iter()
            .flat_map(|style| style.invents.iter().map(|(_, id)| id));
        let learned: Vec<BlueprintId> = taught.chain(invented).copied().collect();
        for &id in learned.iter().chain(&blueprint.techniques) {
            let target = &resolved[id.0 as usize];
            if target.technique.is_none() {
                return Err(BlueprintError::UnknownTechnique {
                    owner: blueprint.name.clone(),
                    technique: target.name.clone(),
                });
            }
        }
        let has_verb = blueprint
            .components
            .iter()
            .any(|c| matches!(c, ComponentDef::Verb(_)));
        if blueprint.technique.is_some() && !has_verb {
            return Err(BlueprintError::TechniqueWithoutVerb(blueprint.name.clone()));
        }
        if let Some(&id) = learned
            .iter()
            .find(|id| resolved[id.0 as usize].is_ability())
        {
            return Err(BlueprintError::AbilityHandedOut {
                by: format!("blueprint {:?}", blueprint.name),
                ability: resolved[id.0 as usize].name.clone(),
            });
        }
    }
    Ok(())
}

/// A school is claimed by one discipline per caster, so which one pays for
/// a cast never depends on handler order. Checked after flattening: a child
/// can add a discipline to an inherited one.
fn check_disciplines(flat: &BTreeMap<String, Blueprint>) -> Result<(), BlueprintError> {
    for (owner, blueprint) in flat {
        let mut schools = Vec::new();
        for component in &blueprint.components {
            let school = match component {
                ComponentDef::CooldownCasting(rule) => rule.school,
                ComponentDef::ManaCasting(rule) => rule.school,
                ComponentDef::BloodCasting(rule) => rule.school,
                _ => continue,
            };
            if schools.contains(&school) {
                return Err(BlueprintError::SchoolClaimedTwice {
                    owner: owner.clone(),
                    school,
                });
            }
            schools.push(school);
        }
    }
    Ok(())
}

/// Checked after flattening, since a cycle can run through inherited
/// equipment lists.
/// Puts each blueprint's equipment on the first free body part that fits,
/// in list order, so spawning can never fail to place it.
fn fit_equipment(resolved: &mut [ResolvedBlueprint]) -> Result<(), BlueprintError> {
    for owner in 0..resolved.len() {
        let mut parts = Vec::with_capacity(resolved[owner].equipment.len());
        for &item in &resolved[owner].equipment {
            let item = &resolved[item.0 as usize];
            let part = item
                .equippable()
                .and_then(|kind| free_part(resolved[owner].body(), &parts, kind))
                .ok_or_else(|| BlueprintError::EquipmentDoesNotFit {
                    owner: resolved[owner].name.clone(),
                    item: item.name.clone(),
                })?;
            parts.push(part);
        }
        resolved[owner].equipped_on = parts;
    }
    Ok(())
}

fn check_equipment_cycles(resolved: &[ResolvedBlueprint]) -> Result<(), BlueprintError> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        New,
        Open,
        Done,
    }

    fn visit(
        resolved: &[ResolvedBlueprint],
        id: usize,
        marks: &mut [Mark],
        path: &mut Vec<usize>,
    ) -> Result<(), BlueprintError> {
        match marks[id] {
            Mark::Done => return Ok(()),
            Mark::Open => {
                let start = path.iter().position(|&p| p == id).unwrap_or_default();
                let cycle = path[start..]
                    .iter()
                    .chain([&id])
                    .map(|&i| resolved[i].name.clone())
                    .collect();
                return Err(BlueprintError::EquipmentCycle(cycle));
            }
            Mark::New => {}
        }
        marks[id] = Mark::Open;
        path.push(id);
        let held = resolved[id].equipment.iter().chain(&resolved[id].spells);
        for item in held.chain(&resolved[id].techniques) {
            visit(resolved, item.0 as usize, marks, path)?;
        }
        path.pop();
        marks[id] = Mark::Done;
        Ok(())
    }

    let mut marks = vec![Mark::New; resolved.len()];
    for id in 0..resolved.len() {
        visit(resolved, id, &mut marks, &mut Vec::new())?;
    }
    Ok(())
}

/// Checks equipment and on-hit names as written, before flattening, so the
/// error names the blueprint that wrote the bad reference rather than a
/// child that inherited it. `resolve_one` checks `inherits`.
fn check_references(raw: &BTreeMap<String, Blueprint>) -> Result<(), BlueprintError> {
    for (owner, blueprint) in raw {
        if let Some(item) = blueprint
            .equipment()
            .iter()
            .find(|item| !raw.contains_key(*item))
        {
            return Err(BlueprintError::UnknownEquipment {
                owner: owner.clone(),
                item: item.clone(),
            });
        }
        if let Some(spell) = blueprint
            .spells
            .iter()
            .flatten()
            .find(|spell| !raw.contains_key(*spell))
        {
            return Err(BlueprintError::UnknownSpell {
                owner: owner.clone(),
                spell: spell.clone(),
            });
        }
        if let Some(technique) = blueprint
            .techniques
            .iter()
            .flatten()
            .find(|technique| !raw.contains_key(*technique))
        {
            return Err(BlueprintError::UnknownTechnique {
                owner: owner.clone(),
                technique: technique.clone(),
            });
        }
        let effects = blueprint
            .components
            .iter()
            .flat_map(|component| match component {
                ComponentDef::OnHit(entries) => entries.as_slice(),
                _ => &[],
            });
        for entry in effects {
            if !raw.contains_key(&entry.effect) {
                return Err(BlueprintError::UnknownEffect {
                    owner: owner.clone(),
                    effect: entry.effect.clone(),
                });
            }
        }
        let summons = blueprint
            .components
            .iter()
            .filter_map(|component| match component {
                ComponentDef::Summon(def) => Some(&def.blueprint),
                _ => None,
            });
        for summon in summons {
            if !raw.contains_key(summon) {
                return Err(BlueprintError::UnknownSummon {
                    owner: owner.clone(),
                    summon: summon.clone(),
                });
            }
        }
    }
    Ok(())
}

fn resolve_one(
    name: &str,
    raw: &BTreeMap<String, Blueprint>,
    resolved: &mut BTreeMap<String, Blueprint>,
    stack: &mut Vec<String>,
) -> Result<(), BlueprintError> {
    if resolved.contains_key(name) {
        return Ok(());
    }
    if let Some(start) = stack.iter().position(|n| n == name) {
        let mut cycle = stack[start..].to_vec();
        cycle.push(name.to_string());
        return Err(BlueprintError::InheritanceCycle(cycle));
    }
    let blueprint = &raw[name];

    let mut flat = Blueprint::default();
    if let Some(parent) = &blueprint.inherits {
        if !raw.contains_key(parent) {
            return Err(BlueprintError::UnknownParent {
                child: name.to_string(),
                parent: parent.clone(),
            });
        }
        stack.push(name.to_string());
        resolve_one(parent, raw, resolved, stack)?;
        stack.pop();
        flat = resolved[parent].clone();
    }

    for component in &blueprint.components {
        match flat
            .components
            .iter_mut()
            .find(|existing| discriminant(*existing) == discriminant(component))
        {
            Some(slot) => *slot = component.clone(),
            None => flat.components.push(component.clone()),
        }
    }
    if blueprint.equipment.is_some() {
        flat.equipment = blueprint.equipment.clone();
    }
    if blueprint.spells.is_some() {
        flat.spells = blueprint.spells.clone();
    }
    if blueprint.techniques.is_some() {
        flat.techniques = blueprint.techniques.clone();
    }

    resolved.insert(name.to_string(), flat);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../data")
    }

    fn bp(inherits: Option<&str>, components: Vec<ComponentDef>) -> Blueprint {
        Blueprint {
            inherits: inherits.map(str::to_string),
            components,
            equipment: None,
            spells: None,
            techniques: None,
        }
    }

    fn with_equipment(mut blueprint: Blueprint, equipment: Option<&[&str]>) -> Blueprint {
        blueprint.equipment = equipment.map(|items| items.iter().map(|s| s.to_string()).collect());
        blueprint
    }

    /// `Base` carries a sword; `Child` sets `equipment` to `child_equipment`.
    fn held() -> ComponentDef {
        ComponentDef::Equippable(Equippable {
            part: PartKind::Hand,
        })
    }

    fn two_hands() -> ComponentDef {
        ComponentDef::Body(Body(
            ["right hand", "left hand"]
                .map(|name| BodyPart {
                    kind: PartKind::Hand,
                    name: name.to_string(),
                })
                .to_vec(),
        ))
    }

    fn resolve_child_equipment(child_equipment: Option<&[&str]>) -> Vec<String> {
        let raw = BTreeMap::from([
            ("Sword".to_string(), bp(None, vec![held()])),
            ("Shield".to_string(), bp(None, vec![held()])),
            (
                "Base".to_string(),
                with_equipment(bp(None, vec![two_hands()]), Some(&["Sword"])),
            ),
            (
                "Child".to_string(),
                with_equipment(bp(Some("Base"), vec![]), child_equipment),
            ),
        ]);
        let blueprints = Blueprints::resolve(raw).unwrap();
        blueprints
            .get("Child")
            .unwrap()
            .equipment
            .iter()
            .map(|&id| blueprints.by_id(id).unwrap().name.clone())
            .collect()
    }

    #[test]
    fn equipment_that_does_not_fit_fails_the_load() {
        let fits = |owner: Vec<ComponentDef>, item: Vec<ComponentDef>, gear: &[&str]| {
            let raw = BTreeMap::from([
                ("Sword".to_string(), bp(None, item)),
                (
                    "Owner".to_string(),
                    with_equipment(bp(None, owner), Some(gear)),
                ),
            ]);
            match Blueprints::resolve(raw) {
                Ok(_) => Ok(()),
                Err(BlueprintError::EquipmentDoesNotFit { owner, item }) => Err((owner, item)),
                Err(other) => panic!("unexpected {other}"),
            }
        };
        let refused = Err(("Owner".to_string(), "Sword".to_string()));
        assert_eq!(
            fits(vec![two_hands()], vec![held()], &["Sword", "Sword"]),
            Ok(())
        );
        assert_eq!(
            fits(vec![two_hands()], vec![held()], &["Sword"; 3]),
            refused
        );
        assert_eq!(fits(vec![two_hands()], vec![], &["Sword"]), refused);
        assert_eq!(fits(vec![], vec![held()], &["Sword"]), refused);
    }

    #[test]
    fn two_disciplines_for_one_school_fail_the_load() {
        use crate::entity::Stat;
        let cooldown = |school| {
            ComponentDef::CooldownCasting(CooldownCasting {
                school,
                ticks_per_power: 100,
                stat: Stat::Willpower,
            })
        };
        let mana = |school| {
            ComponentDef::ManaCasting(ManaCasting {
                school,
                mana_per_power: 1,
                stat: Stat::Intelligence,
            })
        };
        let load = |child: ComponentDef| {
            Blueprints::resolve(BTreeMap::from([
                (
                    "Base".to_string(),
                    bp(None, vec![cooldown(School::Pyromancy)]),
                ),
                ("Mage".to_string(), bp(Some("Base"), vec![child])),
            ]))
        };
        assert!(load(mana(School::Arcane)).is_ok());
        match load(mana(School::Pyromancy)) {
            Err(BlueprintError::SchoolClaimedTwice { owner, school }) => {
                assert_eq!((owner.as_str(), school), ("Mage", School::Pyromancy));
            }
            other => panic!("expected SchoolClaimedTwice, got {other:?}"),
        }
    }

    #[test]
    fn known_spells_are_inherited_replaced_and_checked() {
        let with_spells = |mut blueprint: Blueprint, spells: Option<&[&str]>| {
            blueprint.spells = spells.map(|names| names.iter().map(|s| s.to_string()).collect());
            blueprint
        };
        let raw = |child: Option<&[&str]>| {
            BTreeMap::from([
                ("Bolt".to_string(), bp(None, vec![])),
                ("Nova".to_string(), bp(None, vec![])),
                (
                    "Mage".to_string(),
                    with_spells(bp(None, vec![]), Some(&["Bolt"])),
                ),
                (
                    "Apprentice".to_string(),
                    with_spells(bp(Some("Mage"), vec![]), child),
                ),
            ])
        };
        let spells = |child| {
            let blueprints = Blueprints::resolve(raw(child)).unwrap();
            blueprints
                .get("Apprentice")
                .unwrap()
                .spells
                .iter()
                .map(|&id| blueprints.by_id(id).unwrap().name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(spells(None), ["Bolt"]);
        assert_eq!(spells(Some(&["Nova"])), ["Nova"]);
        assert!(spells(Some(&[])).is_empty());
        match Blueprints::resolve(raw(Some(&["Nope"]))) {
            Err(BlueprintError::UnknownSpell { owner, spell }) => {
                assert_eq!((owner.as_str(), spell.as_str()), ("Apprentice", "Nope"));
            }
            other => panic!("expected UnknownSpell, got {other:?}"),
        }
    }

    #[test]
    fn known_techniques_are_inherited_replaced_and_checked() {
        let with = |mut blueprint: Blueprint, techniques: Option<&[&str]>| {
            blueprint.techniques =
                techniques.map(|names| names.iter().map(|s| s.to_string()).collect());
            blueprint
        };
        let technique = || {
            ComponentDef::Technique(TechniqueDef {
                domain: crate::skill::Domain::Martial,
                reach: 1,
                recharge: 0,
                stamina: 0,
                requires: Vec::new(),
                riders: Vec::new(),
                uses: None,
                bonuses: Vec::new(),
                combo: None,
            })
        };
        let verb = || {
            ComponentDef::Verb(crate::entity::Verb {
                word: "strike".to_string(),
                at: false,
            })
        };
        let raw = |child: Option<&[&str]>| {
            BTreeMap::from([
                ("Lunge".to_string(), bp(None, vec![technique(), verb()])),
                ("Shove".to_string(), bp(None, vec![technique(), verb()])),
                ("Rock".to_string(), bp(None, vec![])),
                (
                    "Duelist".to_string(),
                    with(bp(None, vec![]), Some(&["Lunge"])),
                ),
                (
                    "Squire".to_string(),
                    with(bp(Some("Duelist"), vec![]), child),
                ),
            ])
        };
        let techniques = |child| {
            let blueprints = Blueprints::resolve(raw(child)).unwrap();
            blueprints
                .get("Squire")
                .unwrap()
                .techniques
                .iter()
                .map(|&id| blueprints.by_id(id).unwrap().name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(techniques(None), ["Lunge"]);
        assert_eq!(techniques(Some(&["Shove"])), ["Shove"]);
        assert!(techniques(Some(&[])).is_empty());
        match Blueprints::resolve(raw(Some(&["Nope"]))) {
            Err(BlueprintError::UnknownTechnique { owner, technique }) => {
                assert_eq!((owner.as_str(), technique.as_str()), ("Squire", "Nope"));
            }
            other => panic!("expected UnknownTechnique, got {other:?}"),
        }
        let mut mute = raw(None);
        mute.insert("Lunge".to_string(), bp(None, vec![technique()]));
        assert_eq!(
            Blueprints::resolve(mute).unwrap_err().to_string(),
            "technique \"Lunge\" has no Verb to be logged with"
        );
        // Known by name, but not a technique.
        match Blueprints::resolve(raw(Some(&["Rock"]))) {
            Err(BlueprintError::UnknownTechnique { owner, technique }) => {
                assert_eq!((owner.as_str(), technique.as_str()), ("Squire", "Rock"));
            }
            other => panic!("expected UnknownTechnique, got {other:?}"),
        }
    }

    #[test]
    fn abilities_are_never_taught_invented_or_discovered() {
        let data = data_dir();
        let skills = || SkillBook::load(&data.join("skills.ron")).unwrap();
        let refused = |ability: &str| {
            let mut raw = Blueprints::read_dir(&data.join("entities")).unwrap();
            let marked = raw.get_mut(ability).unwrap();
            marked.components.push(ComponentDef::Ability);
            Blueprints::resolve_with(raw, skills())
                .and_then(|blueprints| {
                    blueprints
                        .with_discoveries(discovery::read(&data.join("discoveries.ron")).unwrap())
                })
                .map(|_| ())
                .unwrap_err()
                .to_string()
        };
        let message = |by: &str, ability: &str| {
            format!("{by} hands out the ability {ability:?}; abilities come only from origins")
        };
        assert_eq!(
            refused("Sweep"),
            message("blueprint \"EdgeManual\"", "Sweep")
        );
        assert_eq!(refused("Shove"), message("blueprint \"Brawling\"", "Shove"));
        assert_eq!(
            refused("Cleave"),
            message("discovery \"the cleave\"", "Cleave")
        );
    }

    #[test]
    fn child_without_equipment_inherits_the_parents() {
        assert_eq!(resolve_child_equipment(None), ["Sword"]);
    }

    #[test]
    fn child_equipment_replaces_the_parents() {
        assert_eq!(resolve_child_equipment(Some(&["Shield"])), ["Shield"]);
    }

    #[test]
    fn empty_child_equipment_clears_the_parents() {
        assert!(resolve_child_equipment(Some(&[])).is_empty());
    }

    #[test]
    fn equipment_parses_absent_present_and_empty() {
        let parse = |src: &str| ron_options().from_str::<Blueprint>(src).unwrap().equipment;
        assert_eq!(parse("(components: [])"), None);
        assert_eq!(
            parse(r#"(equipment: ["Sword"])"#),
            Some(vec!["Sword".to_string()])
        );
        assert_eq!(parse("(equipment: [])"), Some(vec![]));
    }

    #[test]
    fn loads_the_data_directory() {
        let blueprints = Blueprints::load(&data_dir()).expect("data should load");
        for name in [
            "Bleeding",
            "Creature",
            "Goblin",
            "LeatherArmor",
            "Player",
            "RustyKnife",
        ] {
            assert!(blueprints.get(name).is_some(), "missing {name}");
        }
        let id = |name| blueprints.id(name).unwrap();
        assert_eq!(
            blueprints.get("Goblin").unwrap().equipment,
            [id("LeatherArmor"), id("RustyKnife")]
        );
        assert_eq!(
            blueprints.get("RustyKnife").unwrap().on_hit,
            Some(OnHit(vec![OnHitEntry {
                effect: id("Bleeding"),
                chance_bp: 2500,
                on_graze: false,
            }]))
        );
        assert!(
            blueprints
                .get("Player")
                .unwrap()
                .components
                .contains(&ComponentDef::Player)
        );
        let goblin = &blueprints.get("Goblin").unwrap().components;
        assert!(goblin.contains(&ComponentDef::Hostile));
        assert!(goblin.contains(&ComponentDef::Speed(Speed(100))));
        let bleeding = &blueprints.get("Bleeding").unwrap().components;
        assert!(bleeding.contains(&ComponentDef::Duration(Duration(500))));
    }

    #[test]
    fn child_components_override_the_parents_of_the_same_variant() {
        let raw = BTreeMap::from([
            (
                "Base".to_string(),
                bp(
                    None,
                    vec![
                        ComponentDef::BlocksMovement,
                        ComponentDef::Health(Health {
                            current: 10,
                            max: 10,
                        }),
                    ],
                ),
            ),
            (
                "Child".to_string(),
                bp(
                    Some("Base"),
                    vec![
                        ComponentDef::Health(Health { current: 3, max: 3 }),
                        ComponentDef::Melee(Melee { min: 1, max: 2 }),
                    ],
                ),
            ),
        ]);
        let blueprints = Blueprints::resolve(raw).unwrap();
        let child = blueprints.get("Child").unwrap();
        assert_eq!(
            child.components,
            [
                ComponentDef::BlocksMovement,
                ComponentDef::Health(Health { current: 3, max: 3 }),
                ComponentDef::Melee(Melee { min: 1, max: 2 }),
            ]
        );
    }

    #[test]
    fn inheritance_cycles_are_reported() {
        let raw = BTreeMap::from([
            ("A".to_string(), bp(Some("B"), vec![])),
            ("B".to_string(), bp(Some("C"), vec![])),
            ("C".to_string(), bp(Some("A"), vec![])),
        ]);
        match Blueprints::resolve(raw) {
            Err(BlueprintError::InheritanceCycle(path)) => assert_eq!(path, ["A", "B", "C", "A"]),
            other => panic!("expected a cycle error, got {other:?}"),
        }
    }

    fn load_error(raw: Vec<(&str, Blueprint)>) -> String {
        let raw = raw
            .into_iter()
            .map(|(name, blueprint)| (name.to_string(), blueprint))
            .collect();
        match Blueprints::resolve(raw) {
            Err(err) => err.to_string(),
            Ok(_) => panic!("expected a load error"),
        }
    }

    fn on_hit(effect: &str) -> ComponentDef {
        ComponentDef::OnHit(vec![OnHitDef {
            effect: effect.to_string(),
            chance_bp: 5000,
            on_graze: false,
        }])
    }

    #[test]
    fn unknown_parent_is_reported() {
        assert_eq!(
            load_error(vec![("A", bp(Some("Nope"), vec![]))]),
            r#"blueprint "A" inherits from unknown "Nope""#
        );
    }

    /// The parent, which wrote the bad name, is reported, not the child
    /// that inherited it (which would come first in name order).
    #[test]
    fn unknown_equipment_is_reported_on_the_blueprint_that_lists_it() {
        assert_eq!(
            load_error(vec![
                ("Zed", with_equipment(bp(None, vec![]), Some(&["Nope"]))),
                ("Alpha", bp(Some("Zed"), vec![])),
            ]),
            r#"blueprint "Zed" is equipped with unknown "Nope""#
        );
    }

    #[test]
    fn unknown_on_hit_effect_is_reported() {
        assert_eq!(
            load_error(vec![
                ("Zed", bp(None, vec![on_hit("Nope")])),
                ("Alpha", bp(Some("Zed"), vec![])),
            ]),
            r#"blueprint "Zed" applies unknown effect "Nope" on hit"#
        );
    }

    #[test]
    fn unknown_skills_are_reported_and_known_ones_spawn_as_ids_and_xp() {
        let skills = crate::skill::tests::book();
        let named = |components| {
            Blueprints::resolve_with(
                BTreeMap::from([("Fencer".to_string(), bp(None, components))]),
                skills.clone(),
            )
        };
        let levels = |name: &str| ComponentDef::Skills(BTreeMap::from([(name.to_string(), 2)]));
        let trains = |name: &str| ComponentDef::Trains(vec![name.to_string()]);
        for bad in [levels("Rapier"), trains("Rapier")] {
            assert_eq!(
                named(vec![bad]).unwrap_err().to_string(),
                r#"blueprint "Fencer" names unknown skill "Rapier""#
            );
        }

        let blueprints = named(vec![levels("Dagger"), trains("Melee")]).unwrap();
        let mut world = World::new();
        let fencer = blueprints
            .spawn_id(&mut world, blueprints.id("Fencer").unwrap())
            .unwrap();
        let (dagger, melee) = (skills.id("Dagger").unwrap(), skills.id("Melee").unwrap());
        assert_eq!(
            *world.get::<&Skills>(fencer).unwrap(),
            Skills(vec![(dagger, crate::skill::xp_for(2))])
        );
        assert_eq!(*world.get::<&Trains>(fencer).unwrap(), Trains(vec![melee]));
    }

    fn timed() -> ComponentDef {
        ComponentDef::Duration(Duration(100))
    }

    #[test]
    fn an_on_hit_effect_without_a_duration_is_reported() {
        assert_eq!(
            load_error(vec![
                ("Knife", bp(None, vec![on_hit("Rock")])),
                ("Rock", bp(None, vec![])),
            ]),
            r#"blueprint "Knife" applies effect "Rock" on hit, which has no Duration"#
        );
    }

    #[test]
    fn an_on_hit_effect_may_inherit_its_duration() {
        let raw = BTreeMap::from([
            ("Knife".to_string(), bp(None, vec![on_hit("Cut")])),
            ("Timed".to_string(), bp(None, vec![timed()])),
            ("Cut".to_string(), bp(Some("Timed"), vec![])),
        ]);
        assert!(Blueprints::resolve(raw).is_ok());
    }

    #[test]
    fn equipment_cycles_are_reported() {
        assert_eq!(
            load_error(vec![
                ("A", with_equipment(bp(None, vec![]), Some(&["B"]))),
                ("B", with_equipment(bp(None, vec![]), Some(&["A"]))),
            ]),
            "equipment cycle: A -> B -> A"
        );
    }

    /// `Holster` only gets `Belt` by inheriting it.
    #[test]
    fn equipment_cycles_through_inherited_lists_are_reported() {
        assert_eq!(
            load_error(vec![
                ("Base", with_equipment(bp(None, vec![]), Some(&["Belt"]))),
                ("Belt", with_equipment(bp(None, vec![]), Some(&["Holster"]))),
                ("Holster", bp(Some("Base"), vec![])),
            ]),
            "equipment cycle: Belt -> Holster -> Belt"
        );
    }

    #[test]
    fn on_hit_parses_and_is_spawned_with_resolved_ids() {
        let def: Blueprint = ron_options()
            .from_str(r#"(components: [OnHit([(effect: "Cut", chance_bp: 5000)])])"#)
            .unwrap();
        assert_eq!(def.components, [on_hit("Cut")]);

        let blueprints = Blueprints::resolve(BTreeMap::from([
            ("Knife".to_string(), def),
            ("Cut".to_string(), bp(None, vec![timed()])),
        ]))
        .unwrap();
        let mut world = World::new();
        let knife = blueprints
            .spawn_id(&mut world, blueprints.id("Knife").unwrap())
            .unwrap();
        assert_eq!(
            *world.get::<&OnHit>(knife).unwrap(),
            OnHit(vec![OnHitEntry {
                effect: blueprints.id("Cut").unwrap(),
                chance_bp: 5000,
                on_graze: false,
            }])
        );
    }

    #[test]
    fn equipment_is_spawned_and_attached() {
        let blueprints = Blueprints::load(&data_dir()).unwrap();
        let mut world = World::new();
        let goblin = blueprints
            .spawn_id(&mut world, blueprints.id("Goblin").unwrap())
            .unwrap();
        // Equipment first, then what it knows: the shove.
        let attachments = holding::attached(&world, goblin);
        assert_eq!(attachments.len(), 3);
        assert!(world.satisfies::<&Armor>(attachments[0]));
        assert!(world.satisfies::<&OnHit>(attachments[1]));
        assert!(world.satisfies::<&Technique>(attachments[2]));
    }

    #[test]
    fn missing_directory_is_an_error() {
        assert!(Blueprints::load(&data_dir().join("does-not-exist")).is_err());
    }
}
