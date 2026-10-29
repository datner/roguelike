# Plan: fights that are decisions

Handoff document. Read `docs/vision.md` (Fights, Legibility), ADR 0002 (commands), ADR 0005
(randomness), ADR 0006 (time), ADR 0007 (FOV), ADR 0015 (sides), ADR 0024 (techniques) and
`game/CLAUDE.md` first.

## Goal

The vision's Fights section: "A fight is a decision when the obvious move (bump the nearest thing) is
sometimes wrong and you can see why before you act." The floor bot shows the gap: goblins come several
at once and a starting character can only bump. This plan adds, one slice at a time, the rules and
the content that ask questions: being outnumbered, foes that remember, flee and roam, doors, chasms and
tall grass, sprinting, a brute's telegraphed blow, a skirmisher and a swarm.

## Decisions taken for this plan

- **Outnumbered is one rule for everyone**: each enemy adjacent to the defender beyond the first adds
  `OUTNUMBERED_BP` (1000, +10%) to the attacker's hit chance. It lives in `combat::odds`, so previews and
  rolls agree, and ganging up on a goblin works the same. No facing, so no flanking. (The vision said
  "lowers your deflection"; in this combat model deflection is a worn item's, so the rule is written
  as being easier to hit, and the vision is corrected.)
- **Foes remember where they last saw you** (`Quarry { at }`), and go there when they lose sight: that
  is what makes hiding a lure. Noticing is a command (`Command::Notice`) the scheduler queues before an
  actor decides, so `ai::decide` stays a pure choice (ADR 0002).
- **Morale is a threshold**, `Morale { flee_below }` (percent of max health): below it a creature steps
  away from the nearest enemy and only fights when cornered. Data on `Goblin` (25).
- **Roamers patrol without randomness**: `Roams` makes a creature with no quarry walk to the farthest
  open tile it can reach from where it stands (`Route { to }`, chosen by a command when reached), so
  two roamers on a floor cover it and no RNG stream is touched. A dungeon lists roamers as their own
  blueprint (`GoblinScout`), so spawn counts stay data.
- **Terrain is tiles, each doing one thing.** `Door` (closed: blocks walking and sight; bumping opens
  it, a turn; `C` and a direction closes one), `Chasm` (see-through, never walkable; a creature forced
  into it falls: a creature dies, the player falls to the next floor and takes damage), `Grass` (tall:
  walkable, hides what stands in it beyond the next tile and blocks sight past it; fire burns it to
  floor). Generators place them from dungeon data (`doors` in rooms, `chasms` and `grass` patches in
  caves), after the layout, on the map stream, so a dungeon without them draws nothing new.
- **Hiding is not stealth**: the one rule is "unseen beyond the next tile while in grass", for the
  player (`ai::can_see_player`) and for creatures (drawing and `perceives`). A foe that loses sight goes
  to its quarry: show yourself, step into the grass, meet it there.
- **Sprinting is a toggle** (`Z`): while sprinting, a step costs half the energy and a point of
  stamina, and bumping a creature is refused ("You can't attack while sprinting."). It ends when
  stamina runs out or on `Z` again. A creature with `SprintAi` sprints when chasing an enemy more than
  three tiles away (the spirit wolf's charge).
- **A telegraph is a technique with a wind-up.** `WindUp { ticks }` on a technique: performing it
  marks the tiles it will strike (`Telegraph`, a timed effect on the performer, ADR 0006) and the
  blow lands when the wind-up ends, on whoever stands there then. A hit that deals damage to the
  performer during the wind-up staggers it: the telegraph ends with no blow. The tiles are drawn (a
  `Cell` mark from `render_layers`) and the log says it ("The brute raises its club."). The **brute**
  (`GoblinBrute`) performs `HeavySwing`: the three tiles in front of it, heavy damage, a long wind-up.
- **The skirmisher keeps its distance**: `Skirmish { range }` steps away from an adjacent enemy when it
  can, and otherwise uses its `SpellAi`; its sling (`SlingStone`, a cooldown spell) is the ranged
  attack, so casting rules and previews are the ones spells already have. `GoblinSlinger`.
- **The swarm is content**: `Rat`, fast and weak, in packs; corridors and doors are the answer.
- **The bench reports, the owner tunes.** Each slice that changes creature behaviour reruns
  `scripts/balance.sh` and commits the report with what moved in the slice's as-built note.

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first.

### 1. Outnumbered — done
- `combat::odds`: the rule; `combat::outnumbered(state, defender) -> u32` (pub view through
  `view.rs`); the status line shows "Outnumbered: 2" when the player is.
- `docs/vision.md` wording.
- Tests: two goblins beside the player raise each one's hit chance by the step; one does not; the
  preview agrees with the odds; the status line.
- As built: `game::outnumbered` (view) and "  Outnumbered: 2" after the rest budget on the status line.
  The preview reads `odds` itself, so it agrees by construction; the status text is not tested in
  `front` (a session can't place goblins). Bench: the doll tables did not move (dolls never attack); the
  floors moved slightly (a goblin pack hits a little more often).

### 2. Memory and morale — done
- `Quarry`, `Morale`; `Command::Notice` queued by the scheduler before `decide`; `decide` goes to the
  quarry when it perceives no enemy, and flees below its morale; `Goblin.ron` gets `Morale(25)`.
- Saves (both components), `SAVE_VERSION`.
- Tests: a goblin that loses sight walks to where it last saw the player and forgets on arrival; a
  hurt goblin steps away; a cornered one still fights.
- As built: a fleeing creature is a threat (`ai::threatens`) only when adjacent: auto-explore, practice,
  rest and the floor bot ignore one backing off further away, and explore paths through it rather than
  around (routing around one that slips in and out of view turned the path back and forth). Without
  that the bot chased fleeing goblins until stuck. A quarry is forgotten when reached or when no step
  gets closer (the step is greedy). Saves v47.

### 3. Roamers — done
- `Roams`, `Route`; `Command::SetRoute`; the farthest reachable tile (BFS over open tiles);
  `GoblinScout.ron` (inherits `Goblin`, `Roams`); one in the caves and the castle.
- Tests: a roamer with nothing in view walks toward the far end and picks a new end on arrival; a
  roamer that sees the player hunts it instead; a route is the same from the same state.
- As built: a roamer steps straight toward its end when that gets closer and searches the floor only
  when it doesn't (searching every turn made the bench take three minutes; it takes about two now).
  A blocked way is waited out, not re-planned. Saves v48. Scouts now show up among the bench's
  causes of death.

### 4. Doors — done
- `Tile::Door`, `Tile::DoorOpen`; walking into a closed door opens it (a turn); `Command::Close`
  with `InputEvent::Close(Dir)` (`C`, then a direction, in `front`); AI opens doors by walking into them.
- `rooms.rs`: a door where a corridor meets a room, from `Rooms.doors` (a chance in data), on the map
  stream after the layout; `Castle.ron` turns it on.
- Sprites and glyphs (`+`, `'`); README Controls.
- Tests: a closed door blocks sight and walking; bumping opens it and costs a turn; closing it again;
  a goblin opens one; a castle floor has doors; a cave floor draws nothing new.
- As built: `C` closes the first open door beside you (no direction prompt). `Tile::is_way` (open,
  or a closed door) is what path planning walks (explore, AI); sight, missiles and areas keep
  `is_open`. `zone::region` walks through doors without listing them, so the stairs and spawns
  never land on one. Doors are drawn as glyphs; `tiles.ron` may name `door` and `door_open` sprites.
  Auto-explore stops once at each closed door (the step opened it instead of moving). Saves v49.

### 5. Chasms — done
- `Tile::Chasm`: open to sight, never to walking; forced movement (shove, push kick) into one makes the
  creature fall (`Command::Fall`): a creature dies, the player descends a floor and takes damage.
- `zone.rs` caves: `chasms` patches from data, on the map stream after the layout, never on the
  arrival or the stairs, never cutting the stairs off (re-checked by the region walk).
- Tests: a shoved goblin over a chasm dies; the player falls a floor and is hurt (4); walking into a
  chasm is refused; the stairs stay reachable; a dungeon without chasms draws nothing new.
- As built: chasms are dug in `dive::build` (not the cave generator), so any layout can have them and
  the bottom never does; `Dungeon.chasms` patches (Caves: 4), each a tile and the floor around it.
  `Tile::is_transparent` (open or chasm) is what sight, missiles and areas cross. Walking into one is
  refused with "You stop at the edge of the chasm." Goblins shove too, so the player can be shoved
  in. Saves v50.

### 6. Tall grass — done
- `Tile::Grass`: walkable, opaque past itself; what stands in grass is seen only from the next tile
  (player and creatures alike); a fire spell's area burns grass to floor.
- Caves: `grass` patches from data.
- Tests: the player in grass is unseen by a goblin two tiles off and seen from one; a goblin in grass
  is not drawn beyond a tile; a goblin that loses the player in grass walks to the quarry; fire burns
  it.
- As built: three tile tests now: `is_open` (walked: grass too), `is_clear` (missiles and areas: open
  or a chasm's air), `is_transparent` (sight: clear but not grass). `fov::hidden_in_grass` hides a
  creature or item in grass from drawing, auto-runs and the bot beyond a tile; `ai::perceives`
  applies the same rule between creatures. Grass grows in `dive::build` after the chasms
  (`Dungeon.grass`, Caves: 6). Any fire spell's area burns it ("The grass burns away."). The aim and
  look lists still offer a hidden creature (left for the legibility plan). Also here, found while
  the bench ran with chasms: a fall refreshes the player's view (falling on a goblin's turn left it
  blank and auto-explore had nothing to go on); the bot paths around obstacles when chasing, and
  gives up on a foe after 30 turns without a blow (one across a chasm), exploring past it
  (`explore::explore_step_past`). Saves v51.

### 7. Sprinting — done
- `Sprinting` (a marker), `Command::Sprint`, `InputEvent::Sprint` (`Z`); step cost halved while
  sprinting, a point of stamina per step, attacks refused; the status line shows "Sprinting".
- `SprintAi` and the spirit wolf.
- Tests: a sprinting step costs half and a point of stamina; out of stamina it ends; bumping a goblin
  is refused with the message; a wolf sprints when far.
- As built: `game/src/sprint.rs`. Toggling is free (the steps pay). Nothing had a stamina pool before
  (every technique costs 0 stamina), so `Player.ron` gains one (10, a point back every two turns,
  Agility deepens it); the spirit wolf has 6 and `SprintAi`, which charges toward its quarry from
  more than three tiles. A sprinting creature's walk into another is no attack. Saves v52.

### 8. The brute's telegraph — done
- `WindUp`, `Telegraph` (effect), the delayed blow on the tiles; stagger on damage; drawing the marked
  tiles; `HeavySwing.ron`, `GoblinBrute.ron`; the castle and the Embers caves get one.
- Tests: the wind-up marks the tiles and strikes them when it ends; stepping out avoids it; a hit
  during the wind-up staggers it; the marks are in the view; AI performs it only with the enemy in
  the tiles.
- As built: the telegraph is a timed effect like a rune (`data/entities/Telegraph.ron`, no name), its
  blow landed from `effect::expire_effect` with the technique's numbers as they stand then
  (`technique::inspect`). Any damage that gets through staggers it (`technique::stagger`, from the
  damage applier). The marked tiles get a dark red background (`state::TELEGRAPH_BG`), tested in the
  unlit layer rather than a gui shot (a shot would need a brute placed from the window crate, which
  never touches hecs). `HeavySwing`: everything around, 1.5 turns of wind-up, +150% damage;
  `GoblinBrute` (18 HP) in the castle and the Embers caves. The AI already performs it only with an
  enemy in the area (`technique_at`). Saves v53.

### 9. The skirmisher — done
- `Skirmish`, `SlingStone.ron`, `GoblinSlinger.ron`; in both dungeons.
- Tests: adjacent, it steps away; at range, it slings; cornered, it bumps.
- As built: a bow, not a sling spell (a spell would read "casts" and need a school). The
  `GoblinArcher` wears a short bow and a quiver and knows `PartingShot` (step back and shoot) and
  `Snipe` (a plain shot, no recharge), so its shots are the player's: same flight, preview and
  arrows, dropped where they land. `Skirmish` makes it step away from a foe at its elbow, as a shaken
  creature does. Creatures' quivers are stocked with 8 arrows when placed (no RNG, placement
  unchanged). One in each dungeon. Saves v54.

### 10. The swarm — done
- `Rat.ron` (speed 150, 3 HP); a pack in the caves.
- Tests: a rat outpaces the player; the pack spawns.
- As built: `Rat` inherits `Creature` (a beast, no equipment), speed 150, 3 HP, and backs off at half
  health (`Morale(50)`). Four on the caves' first floor, one more each floor down.

### 11. The bench and the vision — done
- `bench/dives.ron` keeps its dives; the report now dies to brutes, slingers and rats as well; the
  as-built note records what the floors look like with all of it.
- `docs/vision.md` Fights: mark what exists and what is still to come.
- As built, what the bench shows with all of it: the goblin archer is now the commonest killer, and
  the level-10 builds that reached floor 6 in 15–20 runs of 20 now reach it in none (archers in the
  caves); the brute kills starting characters in the castle and the Embers; the hedge mage still
  reaches floor 6 in 16–18 runs. Floors take longer (60–100 keypresses for a melee start, from
  20–40) and give more XP (1100–1300 on the first floor, from 600–900), since the foes are varied.
  The archer looks strong for its depth; tuning is the owner's call.

### Afterwards: archers that can be closed on
The owner's tuning after the bench reading above (the archer at equal speed was untouchable):
- **Range costs accuracy.** A launcher holds steady `steady` tiles (a short bow 4; the goblins'
  crude bow 2, range 7); each tile beyond takes 5% off. Named in the reasons ("range, 3 past steady").
- **Snap shots.** A shot drawn on the move loses 20%: one fired within two turns of the last shot with
  a step between (or a technique's step before it). The first shot of an encounter never is (the
  shooter came in ready), nor is one from a hand-nocked arrow (set before any step). `Strode` and
  `Loosed` timestamps; named "drawn on the move". Shots cost a standard turn.
- **Point blank is named** in the reasons instead of folded into accuracy.
- **The goblin archer** carries 3–8 arrows (drawn on the spawn stream after its placement) and knows
  only `Snipe`: no parting shot, so sticking to it stops its shooting in the open.
- Bench: archer kills fell to 4–7 per build in the caves (from 9–14); the level-10 builds still fail
  to clear (tri-sword 5/20, sabre 0/20), now mostly to plain goblins and rats: health never grows
  past 20, which is the open question.

## Out of scope

- Deep water, rubble, darkness as a trade (FOV assumes uniform light, ADR 0007), noise, stealth.
- A smarter floor bot that answers the questions (the "answer builds" of the vision's bench check):
  the bot stays naive; a smarter one is its own plan.
- Factions and situations (who fights whom beyond two sides).
- Tuning: the bench reports, the owner decides.
