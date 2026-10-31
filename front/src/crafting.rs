//! Making things from the keyboard (docs/plans/crafting.md): the recipe list, a recipe's slots with a material picked by hand for any of them, and a batch run. Pure state; what a recipe needs, what fits and why it can't be made come from `game::recipe`, never worked out here.

use game::{GameState, ItemRef, MAX_SLOTS, PlayerAction, RecipeView, recipe, recipes};

use crate::input::OverlayInput;
use crate::targeting::{Chooser, ChooserAction};

/// The recipe list (`m`): Enter makes one with the materials filled in,
/// Tab opens its slots, `a` makes as many as the materials allow.
#[derive(Debug, Default)]
pub struct Recipes {
    chooser: Chooser,
}

/// One recipe's slots, each with the material picked for it by hand, or
/// none for the first that fits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Making {
    recipe: u32,
    slot: usize,
    chosen: [Option<ItemRef>; MAX_SLOTS],
}

/// What the session does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CraftAction {
    /// Close the list.
    Close,
    /// Back from a recipe's slots to the list.
    Back,
    /// A recipe's slots.
    Open(Making),
    /// Make one now.
    Make(PlayerAction),
    /// Make one a turn while the materials last: each slot picked by hand
    /// keeps to that kind of material (by noun).
    Batch(Batch),
}

/// A batch run's recipe and the kind of material each hand-picked slot
/// keeps to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    pub recipe: u32,
    pub kinds: [Option<String>; MAX_SLOTS],
}

impl Recipes {
    pub fn selected(&self) -> usize {
        self.chooser.selected()
    }

    pub fn handle(&mut self, input: OverlayInput, state: &GameState) -> Option<CraftAction> {
        let listed: Vec<u32> = recipes(state).iter().map(|view| view.recipe).collect();
        match input {
            OverlayInput::SwitchTab => {
                let &recipe = listed.get(self.chooser.selected())?;
                return Some(CraftAction::Open(Making {
                    recipe,
                    slot: 0,
                    chosen: [None; MAX_SLOTS],
                }));
            }
            OverlayInput::Batch => {
                let &recipe = listed.get(self.chooser.selected())?;
                return Some(CraftAction::Batch(Batch {
                    recipe,
                    kinds: Default::default(),
                }));
            }
            _ => {}
        }
        match self.chooser.handle(input, &listed)? {
            ChooserAction::Cancel => Some(CraftAction::Close),
            ChooserAction::Pick(recipe) => Some(CraftAction::Make(PlayerAction::Craft {
                recipe,
                inputs: [None; MAX_SLOTS],
            })),
        }
    }
}

impl Making {
    pub fn recipe(&self) -> u32 {
        self.recipe
    }

    pub fn slot(&self) -> usize {
        self.slot
    }

    /// The recipe as it would go with what is picked now.
    pub fn view(&self, state: &GameState) -> Option<RecipeView> {
        recipe(state, self.recipe, self.chosen)
    }

    pub fn handle(&mut self, input: OverlayInput, state: &GameState) -> Option<CraftAction> {
        let view = self.view(state)?;
        let slots = view.slots.len();
        match input {
            OverlayInput::Up => self.slot = self.slot.saturating_sub(1),
            OverlayInput::Down => self.slot = (self.slot + 1).min(slots.saturating_sub(1)),
            OverlayInput::Focus(row) => self.slot = row.min(slots.saturating_sub(1)),
            OverlayInput::Left | OverlayInput::Right => {
                let fits = &view.slots.get(self.slot)?.fits;
                // `None` (the first that fits) comes before every material.
                let choices: Vec<Option<ItemRef>> = std::iter::once(None)
                    .chain(fits.iter().map(|&(item, _)| Some(item)))
                    .collect();
                let at = choices
                    .iter()
                    .position(|&choice| choice == self.chosen[self.slot])
                    .unwrap_or(0);
                let step = if input == OverlayInput::Right {
                    1
                } else {
                    choices.len() - 1
                };
                self.chosen[self.slot] = choices[(at + step) % choices.len()];
            }
            OverlayInput::Select => {
                return Some(CraftAction::Make(PlayerAction::Craft {
                    recipe: self.recipe,
                    inputs: self.chosen,
                }));
            }
            OverlayInput::Batch => {
                let mut kinds: [Option<String>; MAX_SLOTS] = Default::default();
                for (kind, (slot, chosen)) in
                    kinds.iter_mut().zip(view.slots.iter().zip(self.chosen))
                {
                    *kind = chosen.and_then(|chosen| {
                        slot.fits
                            .iter()
                            .find(|(item, _)| *item == chosen)
                            .map(|(_, noun)| noun.clone())
                    });
                }
                return Some(CraftAction::Batch(Batch {
                    recipe: self.recipe,
                    kinds,
                }));
            }
            OverlayInput::Close => return Some(CraftAction::Back),
            _ => {}
        }
        None
    }

    /// The material picked by hand for each slot, by noun, `None` for the
    /// first that fits.
    pub fn picked(&self, state: &GameState) -> Vec<Option<String>> {
        let Some(view) = self.view(state) else {
            return Vec::new();
        };
        view.slots
            .iter()
            .zip(self.chosen)
            .map(|(slot, chosen)| {
                let chosen = chosen?;
                slot.fits
                    .iter()
                    .find(|(item, _)| *item == chosen)
                    .map(|(_, noun)| noun.clone())
            })
            .collect()
    }
}

/// One craft of a batch: each kept kind's first carried piece not already
/// used, the rest filled by `game`. `None` once a kept kind has run out.
pub(crate) fn batch_action(state: &GameState, batch: &Batch) -> Option<PlayerAction> {
    let view = recipe(state, batch.recipe, [None; MAX_SLOTS])?;
    let mut inputs = [None; MAX_SLOTS];
    for (i, (slot, kind)) in view.slots.iter().zip(&batch.kinds).enumerate() {
        let Some(kind) = kind else { continue };
        let taken = &inputs[..i];
        let &(item, _) = slot
            .fits
            .iter()
            .find(|(item, noun)| noun == kind && !taken.contains(&Some(*item)))?;
        inputs[i] = Some(item);
    }
    Some(PlayerAction::Craft {
        recipe: batch.recipe,
        inputs,
    })
}

/// A recipe as a list row: what it makes and from what, then the notes a
/// frontend shows quieter: where it is made, and why it can't be now.
pub fn recipe_text(view: &RecipeView) -> (String, Vec<String>) {
    let from: Vec<String> = view
        .slots
        .iter()
        .map(|slot| format!("{} ({})", slot.role, slot.needs.join(", ")))
        .collect();
    let main = format!("{}: {}", view.name, from.join(" + "));
    let mut notes = Vec::new();
    if view.field {
        notes.push("anywhere".to_string());
    }
    notes.extend(view.blocked.clone());
    (main, notes)
}

/// A recipe's slots as rows: the role and what fills it, picked by hand or
/// else what `game` would use; the note lists what else fits.
pub fn slot_rows(view: &RecipeView, picked: &[Option<String>]) -> Vec<(String, Vec<String>)> {
    view.slots
        .iter()
        .enumerate()
        .map(|(i, slot)| {
            let using = match picked.get(i).cloned().flatten() {
                Some(noun) => noun,
                None => slot.uses.clone().map_or_else(
                    || format!("nothing {}", slot.needs.join(" and ")),
                    |noun| format!("{noun} (first that fits)"),
                ),
            };
            let note = format!("{} to choose from", slot.fits.len());
            (format!("{}: {using}", slot.role), vec![note])
        })
        .collect()
}
