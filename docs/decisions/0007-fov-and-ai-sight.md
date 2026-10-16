# 0007 — Field of view: symmetric shadowcasting, shared with AI sight

Status: Accepted
Date: 2026-09-28

## Context
The player needs a field of view and remembered terrain, and hostiles need to stop noticing the player through walls. Most FOV algorithms are asymmetric: the player can see a monster that can't see them back, or the other way round, which reads as unfair and makes "can this monster see me?" a separate question needing its own computation per actor.

## Decision
- **Algorithm.** Albert Ford's symmetric shadowcasting (`game/src/fov.rs`): four quadrants, rows scanned outward, exact rational slopes in integer math. A floor tile is visible only if its centre lies inside the lit sector; opaque tiles are visible if any part is. Between floor tiles, A sees B iff B sees A. Every non-floor tile (and off-map) is opaque.
- **Radius.** Circular: a tile is in range iff `dx² + dy² ≤ fov::RADIUS²`. The radius only gates what is revealed, not how shadows propagate, so it keeps the symmetry.
- **When.** Once per player turn: `take_turn` applies the player's action, drains its follow-ups, then applies `Command::UpdateFov`, then runs the world. `GameState::bare` computes the first view. The result is `GameState.visible` (one flag per tile); `GameState.seen` accumulates it. Terrain is remembered, and so is the item last seen on each tile (`GameState.remembered_items`, refreshed only while the tile is visible); creatures never are.
- **AI sight is an assumption, not a computation.** `ai::can_see_player` says a creature sees the player iff the creature's tile is in the player's visible set. That is valid only while FOV is symmetric, lighting is uniform, and every creature's sight radius equals the player's. All AI perception goes through that one function.

## Consequences
- One FOV per player turn instead of one per actor. Creatures don't block sight, so moving monsters between player turns never invalidates the set.
- Adding lighting, darkness, blindness, differing sight radii or per-actor senses breaks the assumption: `can_see_player` must then compute (or cache) the actor's own view. Its doc comment lists the conditions.
- **Future projectile targeting must use a line consistent with this FOV** (for example, a line that stays within the symmetric sector / Ford's "visible iff centre in sector" rule). A Bresenham line would let you see a target you can't shoot, or shoot one you can't see.
- Moving into a wall is still refused for every wall, seen or not; costs for bumping unseen walls are out of scope.
