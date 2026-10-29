//! Synchronous, typed event dispatch.
//!
//! Handlers are registered per (event type, component type). Firing an event
//! at an entity runs every handler whose component is present on that entity
//! or on one of its `Attachments`. Handlers only get a shared `&World`: they
//! may rewrite the event, stop propagation, and emit commands into `ctx.out`,
//! but they never mutate the world — the command applier does that.

use std::any::{Any, TypeId};
use std::collections::BTreeMap;
use std::ops::ControlFlow;

use hecs::{Component, Entity, World};
use rand_pcg::Pcg64;

use crate::command::Command;
use crate::entity::Attachments;
use crate::map::Map;

/// What a handler can see and do. The fields are disjoint borrows so a
/// handler can read the world while drawing from the RNG and emitting commands.
pub(crate) struct Ctx<'a> {
    pub world: &'a World,
    #[expect(
        dead_code,
        reason = "no handler reads the map yet; part of the handler context"
    )]
    pub map: &'a Map,
    pub rng: &'a mut Pcg64,
    pub out: &'a mut Vec<Command>,
}

type ErasedHandler<E> = Box<dyn Fn(&mut Ctx<'_>, Entity, E) -> ControlFlow<E, E>>;

struct Entry<E> {
    priority: i32,
    /// Whether `entity` carries this handler's component.
    applies_to: fn(&World, Entity) -> bool,
    call: ErasedHandler<E>,
}

fn has_component<C: Component>(world: &World, entity: Entity) -> bool {
    world.entity(entity).is_ok_and(|e| e.has::<C>())
}

/// Handlers keyed by event type.
///
/// Dispatch order is a function of priorities and the target's attachments
/// alone: each event type's handlers have distinct priorities (enforced at
/// registration) and are kept sorted by priority. The `TypeId` map is only
/// ever used for lookup, never iterated, and registration order never matters.
#[derive(Default)]
pub(crate) struct EventRegistry {
    /// `TypeId::of::<E>()` -> `Vec<Entry<E>>`, sorted by priority.
    handlers: BTreeMap<TypeId, Box<dyn Any>>,
}

impl EventRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `handler` for event `E`, run for entities that carry `C`.
    ///
    /// # Panics
    /// If `E` already has a handler at `priority`. Two handlers at one
    /// priority would have no order that doesn't depend on registration
    /// order; give each its own constant in `prio`.
    pub fn on<E: 'static, C: Component>(
        &mut self,
        priority: i32,
        handler: impl Fn(&mut Ctx<'_>, &C, E) -> ControlFlow<E, E> + 'static,
    ) -> &mut Self {
        let call: ErasedHandler<E> = Box::new(move |ctx: &mut Ctx<'_>, holder: Entity, ev: E| {
            let world = ctx.world;
            match world.get::<&C>(holder) {
                Ok(component) => handler(ctx, &component, ev),
                Err(_) => ControlFlow::Continue(ev),
            }
        });
        let entries = self
            .handlers
            .entry(TypeId::of::<E>())
            .or_insert_with(|| Box::new(Vec::<Entry<E>>::new()))
            .downcast_mut::<Vec<Entry<E>>>()
            .expect("handler list is keyed by its event's TypeId");
        match entries.binary_search_by_key(&priority, |entry| entry.priority) {
            Ok(_) => panic!(
                "event {} already has a handler at priority {priority}",
                std::any::type_name::<E>()
            ),
            Err(at) => entries.insert(
                at,
                Entry {
                    priority,
                    applies_to: has_component::<C>,
                    call,
                },
            ),
        }
        self
    }

    /// Dispatches `ev` to `target`. Matching handlers run in ascending
    /// priority; a handler matching several holders runs for the target
    /// first, then attachments in `Vec` order. `Break` stops the rest.
    pub fn fire<E: 'static>(&self, ctx: &mut Ctx<'_>, target: Entity, ev: E) -> ControlFlow<E, E> {
        let Some(entries) = self
            .handlers
            .get(&TypeId::of::<E>())
            .and_then(|list| list.downcast_ref::<Vec<Entry<E>>>())
        else {
            return ControlFlow::Continue(ev);
        };

        let mut holders = vec![target];
        if let Ok(attachments) = ctx.world.get::<&Attachments>(target) {
            holders.extend(attachments.0.iter().copied());
        }

        // `entries` is sorted by (unique) priority and `holders` is in slot
        // order, so this nesting already yields (priority, slot) order.
        let world = ctx.world;
        entries
            .iter()
            .flat_map(|entry| {
                holders
                    .iter()
                    .filter(move |&&holder| (entry.applies_to)(world, holder))
                    .map(move |&holder| (entry, holder))
            })
            .try_fold(ev, |ev, (entry, holder)| (entry.call)(ctx, holder, ev))
    }
}

impl EventRegistry {
    /// `fire`, telling `trace` after each handler which holder's part ran and
    /// the event before and after it, so a caller can name what changed
    /// (legibility: odds that carry their reasons). Same order, same result.
    pub fn fire_traced<E: 'static + Clone>(
        &self,
        ctx: &mut Ctx<'_>,
        target: Entity,
        ev: E,
        trace: &mut dyn FnMut(Entity, &E, &E),
    ) -> ControlFlow<E, E> {
        let Some(entries) = self
            .handlers
            .get(&TypeId::of::<E>())
            .and_then(|list| list.downcast_ref::<Vec<Entry<E>>>())
        else {
            return ControlFlow::Continue(ev);
        };
        let mut holders = vec![target];
        if let Ok(attachments) = ctx.world.get::<&Attachments>(target) {
            holders.extend(attachments.0.iter().copied());
        }
        let world = ctx.world;
        let calls: Vec<(&Entry<E>, Entity)> = entries
            .iter()
            .flat_map(|entry| {
                holders
                    .iter()
                    .filter(move |&&holder| (entry.applies_to)(world, holder))
                    .map(move |&holder| (entry, holder))
            })
            .collect();
        let mut ev = ev;
        for (entry, holder) in calls {
            let before = ev.clone();
            match (entry.call)(ctx, holder, ev) {
                ControlFlow::Continue(after) => {
                    trace(holder, &before, &after);
                    ev = after;
                }
                ControlFlow::Break(after) => {
                    trace(holder, &before, &after);
                    return ControlFlow::Break(after);
                }
            }
        }
        ControlFlow::Continue(ev)
    }
}

/// The event as it stands, whether or not propagation was stopped.
pub fn settle<E>(flow: ControlFlow<E, E>) -> E {
    match flow {
        ControlFlow::Continue(ev) | ControlFlow::Break(ev) => ev,
    }
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "dispatch tests on a bare World with test-only components; never a GameState, never saved"
)]
mod tests {
    use rand::SeedableRng;

    use super::*;
    use crate::map::{Grid, Tile};

    struct Trace(Vec<&'static str>);

    struct A;
    struct B;
    struct Absent;

    fn record(
        label: &'static str,
    ) -> impl Fn(&mut Ctx<'_>, &A, Trace) -> ControlFlow<Trace, Trace> {
        move |_, _, mut t| {
            t.0.push(label);
            ControlFlow::Continue(t)
        }
    }

    fn record_b(
        label: &'static str,
    ) -> impl Fn(&mut Ctx<'_>, &B, Trace) -> ControlFlow<Trace, Trace> {
        move |_, _, mut t| {
            t.0.push(label);
            ControlFlow::Continue(t)
        }
    }

    fn run(
        registry: &EventRegistry,
        world: &World,
        target: Entity,
    ) -> (ControlFlow<Trace, Trace>, Vec<Command>) {
        let map = Grid::new(1, 1, Tile::Floor);
        let mut rng = Pcg64::seed_from_u64(0);
        let mut out = Vec::new();
        let mut ctx = Ctx {
            world,
            map: &map,
            rng: &mut rng,
            out: &mut out,
        };
        let flow = registry.fire(&mut ctx, target, Trace(Vec::new()));
        (flow, out)
    }

    fn trace(flow: ControlFlow<Trace, Trace>) -> Vec<&'static str> {
        settle(flow).0
    }

    #[test]
    fn handlers_run_in_priority_order_across_target_and_attachments() {
        let mut world = World::new();
        let gear = world.spawn((B,));
        let target = world.spawn((A, Attachments(vec![gear])));

        let mut registry = EventRegistry::new();
        registry.on(20, record("target A @20"));
        registry.on(10, record_b("gear B @10"));
        registry.on(30, record_b("gear B @30"));

        let (flow, _) = run(&registry, &world, target);
        assert_eq!(trace(flow), ["gear B @10", "target A @20", "gear B @30"]);
    }

    /// Labels its holder, so traces show which entity a handler ran for.
    struct Tag(&'static str);

    fn record_tag(t: &Tag, mut trace: Trace) -> ControlFlow<Trace, Trace> {
        trace.0.push(t.0);
        ControlFlow::Continue(trace)
    }

    #[test]
    fn one_handler_runs_for_the_target_then_attachments_in_vec_order() {
        let mut world = World::new();
        let first = world.spawn((Tag("first"),));
        let second = world.spawn((Tag("second"),));
        let target = world.spawn((Tag("target"), Attachments(vec![second, first])));

        let mut registry = EventRegistry::new();
        registry.on::<Trace, Tag>(0, |_, tag, t| record_tag(tag, t));

        let (flow, _) = run(&registry, &world, target);
        assert_eq!(trace(flow), ["target", "second", "first"]);
    }

    #[test]
    fn registration_order_does_not_change_dispatch_order() {
        let mut world = World::new();
        let gear = world.spawn((A, B));
        let target = world.spawn((A, B, Attachments(vec![gear])));

        let forward = {
            let mut registry = EventRegistry::new();
            registry.on(10, record("A @10"));
            registry.on(20, record_b("B @20"));
            registry.on(5, record_b("B @5"));
            registry
        };
        let backward = {
            let mut registry = EventRegistry::new();
            registry.on(5, record_b("B @5"));
            registry.on(20, record_b("B @20"));
            registry.on(10, record("A @10"));
            registry
        };

        let expected = ["B @5", "B @5", "A @10", "A @10", "B @20", "B @20"];
        assert_eq!(trace(run(&forward, &world, target).0), expected);
        assert_eq!(trace(run(&backward, &world, target).0), expected);
    }

    #[test]
    #[should_panic(expected = "already has a handler at priority 7")]
    fn duplicate_priority_for_one_event_is_rejected() {
        let mut registry = EventRegistry::new();
        registry.on(7, record("A"));
        registry.on(7, record_b("B"));
    }

    #[test]
    fn the_same_priority_may_be_reused_across_events() {
        struct Other;
        let mut registry = EventRegistry::new();
        registry.on(7, record("A"));
        registry.on::<Other, A>(7, |_, _, ev| ControlFlow::Continue(ev));
    }

    #[test]
    fn break_stops_later_handlers() {
        let mut world = World::new();
        let target = world.spawn((A,));

        let mut registry = EventRegistry::new();
        registry.on(1, record("first"));
        registry.on::<Trace, A>(2, |_, _, mut t| {
            t.0.push("stop");
            ControlFlow::Break(t)
        });
        registry.on(3, record("never"));

        let (flow, _) = run(&registry, &world, target);
        assert!(flow.is_break());
        assert_eq!(trace(flow), ["first", "stop"]);
    }

    #[test]
    fn handlers_only_run_when_their_component_is_present() {
        let mut world = World::new();
        let target = world.spawn((A,));

        let mut registry = EventRegistry::new();
        registry.on(1, record("A"));
        registry.on(2, record_b("B"));
        registry.on::<Trace, Absent>(3, |_, _, mut t| {
            t.0.push("absent");
            ControlFlow::Continue(t)
        });

        let (flow, _) = run(&registry, &world, target);
        assert_eq!(trace(flow), ["A"]);
    }

    #[test]
    fn handlers_emit_commands_through_ctx_out() {
        let mut world = World::new();
        let target = world.spawn((A,));

        let mut registry = EventRegistry::new();
        registry.on::<Trace, A>(0, move |ctx, _, t| {
            ctx.out.push(Command::Die { entity: target });
            ControlFlow::Continue(t)
        });

        let (_, out) = run(&registry, &world, target);
        assert!(matches!(out.as_slice(), [Command::Die { entity }] if *entity == target));
    }

    #[test]
    fn unregistered_events_pass_through() {
        let mut world = World::new();
        let target = world.spawn((A,));
        let (flow, _) = run(&EventRegistry::new(), &world, target);
        assert!(flow.is_continue());
    }
}
