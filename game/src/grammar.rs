//! Log-message grammar: every sentence about an entity goes through a
//! `Subject`, so the player reads "You take 3 damage." and everyone else
//! "The goblin takes 3 damage."

use hecs::{Entity, World};

use crate::entity::{Name, Player};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    /// "you", "the goblin", or "something" for an unnamed entity.
    noun: String,
    second_person: bool,
}

impl Subject {
    /// The player is "you"; anything with a `Name` is "the <name>".
    pub(crate) fn of(world: &World, entity: Entity) -> Self {
        if world.satisfies::<&Player>(entity) {
            return Self::you();
        }
        match world.get::<&Name>(entity) {
            Ok(name) => Self::the(&name.0),
            Err(_) => Self {
                noun: "something".to_string(),
                second_person: false,
            },
        }
    }

    pub fn you() -> Self {
        Self {
            noun: "you".to_string(),
            second_person: true,
        }
    }

    pub fn the(name: &str) -> Self {
        Self {
            noun: format!("the {name}"),
            second_person: false,
        }
    }

    /// "you" / "the goblin".
    pub fn noun(&self) -> &str {
        &self.noun
    }

    /// "your" / "the goblin's".
    pub fn possessive(&self) -> String {
        if self.second_person {
            "your".to_string()
        } else {
            format!("{}'s", self.noun)
        }
    }

    /// `base` ("take", "be") conjugated to agree with this subject.
    pub fn verb(&self, base: &str) -> String {
        if self.second_person {
            match base {
                "be" => "are".to_string(),
                _ => base.to_string(),
            }
        } else {
            third_person_singular(base)
        }
    }

    /// "<Subject> <verb>[ <rest>]." — e.g. `sentence("take", "3 damage")`.
    pub fn sentence(&self, verb: &str, rest: &str) -> String {
        let mut out = format!("{} {}", self.noun, self.verb(verb));
        if !rest.is_empty() {
            out.push(' ');
            out.push_str(rest);
        }
        out.push('.');
        capitalize(&out)
    }
}

fn third_person_singular(base: &str) -> String {
    match base {
        "be" => return "is".to_string(),
        "have" => return "has".to_string(),
        _ => {}
    }
    if ["s", "sh", "ch", "x", "z", "o"]
        .iter()
        .any(|end| base.ends_with(end))
    {
        return format!("{base}es");
    }
    if let Some(stem) = base.strip_suffix('y')
        && !stem.ends_with(['a', 'e', 'i', 'o', 'u'])
    {
        return format!("{stem}ies");
    }
    format!("{base}s")
}

/// Uppercases the first character.
pub fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "grammar tests on a bare World; never a GameState, never saved"
)]
mod tests {
    use super::*;

    #[test]
    fn player_and_non_player_subjects_agree_with_their_verbs() {
        assert_eq!(
            Subject::you().sentence("take", "3 damage"),
            "You take 3 damage."
        );
        assert_eq!(
            Subject::the("goblin").sentence("take", "3 damage"),
            "The goblin takes 3 damage."
        );
    }

    #[test]
    fn irregular_and_spelling_rules() {
        let you = Subject::you();
        let goblin = Subject::the("goblin");
        for (base, second, third) in [
            ("be", "are", "is"),
            ("have", "have", "has"),
            ("die", "die", "dies"),
            ("bump", "bump", "bumps"),
            ("catch", "catch", "catches"),
            ("miss", "miss", "misses"),
            ("go", "go", "goes"),
            ("fly", "fly", "flies"),
            ("stay", "stay", "stays"),
        ] {
            assert_eq!(you.verb(base), second, "you {base}");
            assert_eq!(goblin.verb(base), third, "goblin {base}");
        }
    }

    #[test]
    fn possessives() {
        assert_eq!(Subject::you().possessive(), "your");
        assert_eq!(Subject::the("goblin").possessive(), "the goblin's");
    }

    #[test]
    fn subjects_come_from_the_world() {
        let mut world = World::new();
        let player = world.spawn((Player, Name("player".into())));
        let goblin = world.spawn((Name("goblin".into()),));
        let blob = world.spawn(());
        assert_eq!(Subject::of(&world, player), Subject::you());
        assert_eq!(Subject::of(&world, goblin), Subject::the("goblin"));
        assert_eq!(
            Subject::of(&world, blob).sentence("die", ""),
            "Something dies."
        );
    }
}
