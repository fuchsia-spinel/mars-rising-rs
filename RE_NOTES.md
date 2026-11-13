# Reverse-engineering notes — `Mars Rising` 1.0.1 (PowerPC CFM / PEF)

`Mars Rising` is a stripped PEF app: code section 404,860 bytes (no symbols, no traceback tables), pattern-initialised
data section, imports from InterfaceLib (369 symbols), SoundLib, MathLib, DisplayLib, InputSprocketLib.
`tools/re/pef.py` loads sections and unpacks pattern data; `tools/re/ppc.py` is a small PowerPC disassembler
(integer, load/store, branches, `rlwinm`…; no FP arithmetic / imports resolution yet). Addresses are code-section offsets.

## Located routines
| Address | Finding |
|---|---|
| `0x54b8`, `0x5524` | `GetResource('TILE',1)` / `('TILF',1)` -> TOC globals at `-27960(r2)` / `-27964(r2)` |
| `0x103fc…0x10a00`, `0x1493c…0x14b58` | map resource loaders (`BASE`,`FRIN`,`SHAD`,`GS  `,`DIME`,`ASGM`,`_ASG`,`__GS`) |
| `0xc1e0…0xc3cc` | **map draw**: pass 1 base tiles, pass 2 fringe (`FRIN`), pass 3 shadow (`SHAD`) |
| `0x3af4` | `draw_tile(x,y,tile_id,flagA,flagB)`: tile source = `TILE` sheet if flagA==0 else `TILF`; `(id-1)*1024+12`; flagB!=0 copies rows bottom-up (vertical flip) |
| `0x3728` | `draw_group_sprite(group,x,y,w,h,_,idx)`: clips to a screen rect, idx is 1-based; group 2 = fringe sprite set (ids 10000+), group 3 = shadow set (15000+) |
| `0x3e70` / `0x4cec` | look up a sprite in a group / draw it |

## Map value decoding (from the loop at `0xc1f8`)
`v == 0xFFFF` skip; `v < 1 -> 1`; `v <= limit` plain tile; else `v <= 10000 -> limit`, `10000 < v <= 20000` -> flagB (vflip),
`20000 < v <= 30000` -> flagA (mirrored sheet), `v > 30000` -> both; tile id = `v - k*10000`. FRIN/SHAD bytes: `255` skip, `<1 -> 1`, clamp to a limit.

## Verification
Rendering level 5 with these rules matches the original screenshot at scroll 2755 (row order bottom-up): 64 % → 85 % → 90.3 %
of pixels as base / +fringe / +shadow. Before the fixes (0-based ids, no flips, top-down) no alignment existed.

## Ground objects (`__GS` records) — loader at `0x14b58`
Each 50-byte record is copied into a 168-byte runtime struct (250 per level): `rec[0]`→+0, `rec[1]`→+1, `kind` (u32 @rec+2)→+8,
`rec+6`→+12, `variant` (@rec+10)→+76, x float→+24, y float→+28, `prev_link`→+92, `next_link`→+96, `rec+30..+46` (5×u32)→+108,+112,+116,+120,+124.
A jump table (TOC−12052, 25 entries) then stores a **1-based animation index at +52** (index into the `SpIc` 1000 table, i.e. sprite strips):
kinds 1 and 24 choose by `variant` (1→51/70, 2→54/73, 3→52/71, 4→53/72, 5–8 repeat) or 65/75 if the record has a predecessor link;
2→56, 3→58, 4→59, 5→26, 6→89, 7→60, 8→64, 9–14→23, 15→57, 16→158, 17→155, 18→131, 19→139, 20→135, 21→66, 22→68, 23→62;
kinds 7 and 15 set state(+56)=3; kinds 16–21 copy `variant` to state(+56).
TOC base `r2 = 0x8000` (main vector at data `0x20f8`); `tools/re/pef.py` now unpacks the data section correctly (158,216 bytes).
Screenshot checks: kind 8 -> strip 28400 frame 0 at the cell origin (95-100 % match); kind 4 -> strip 28160 (24 frames), frame 28172 at +(7,7) (97 %).
`0x26188–0x26340` is the score-award table (500…20000), not sprite lookup.

## Ground-object draw path (verified)
| Address | Finding |
|---|---|
| `0x15a10…0x15bd8` | per-object draw: converts the object's float position to ints via `0x3b248`, draws a shadow copy at (−6,+8) when `flags[1]` is set, then `draw_anim(group 0, x, y, obj+16, obj+20, anim=obj+52, frame=obj+56)` |
| `0x2f88` | `draw_anim(group,x,y,w,h,anim,frame)`: frame ≤ 0 draws nothing, frame is 1-based; adds a 64 px off-screen margin and clips; sprite = `0x3e70(group, anim-1, frame-1)` |
| `0x3e70` / `0x3ed0` | sprite-handle lookup / animation-table lookup `(anim-1)*4+2` -> (first sprite, frame count), i.e. the `SpIc` table, 1-based |
| `0x3b248` | **anchor**: `x += (64-w)/2` if `w>32` else `(32-w)/2` (same for y with `h`), truncating toward zero; kind 1 then applies per-variant fix-ups through a table |
Static frame rule: `state` (+56) starts at 1; kinds 7/15 start at 3; kinds 16–21 copy `variant` into it -> frame index `variant-1` (6/6 screenshot cases exact).

## Per-cell object draw and shadows (verified)
`0x158a8` `draw_cell_object(col,row)`: reads the object index from the **GS marker layer** (255 = none; row-pointer table at `-27604(r2)`),
computes the cell's float position, anchors via `0x3b248`, then (1) if the record's `flags[1]` is set draws a **shadow** copy at (−6,+8) through `0x3358`,
(2) draws the object (`0x2f88` / `0x3170`, which differ only in their clip rectangles), (3) if `next_link != -1` draws the linked partner record on top at the same cell.
Shadow = the sprite's silhouette; pixels already in the frame are remapped through `DARK 128 "System Shadow"`. Verified: 5/5 static-frame objects exactly, 9 shadow
casters in total match at 97–100 %, the one `flags[1]=0` object shows none.
Kind-1 objects: 13 visible in the screenshots, none drawn; their records come in linked base+partner pairs (partner anim 65 = strip 28500, 24 frames). Hypothesis: they
start hidden (state/frame 0 draws nothing in `0x2f88`) until triggered.

## Spawn system (groups / enemies)
Runtime arrays (TOC-relative globals): **groups** `-29532(r2)`: 100 × 124 bytes = the `_ASG` resource copied verbatim; **enemy slots** `-28712(r2)`: 40 × 176 bytes
(cleared at level init `0x2d528`, which also loads `ASGM`/`_ASG` with `GetResource`). `ASGM` (one u32 per map row, 255 = none) is the spawn-trigger table.
| Address | Finding |
|---|---|
| `0x2d9bc` | spawn trigger: `g = ASGM[row]`; `0 <= g < 100`, then if `group+88 == 0` -> `0x2da30` (normal spawn) else `0x40e7c` (special spawn) |
| `0x2da30` | `spawn_group(g)`: only if `group[0] == 0`; `count = clamp(group+48, 0..10)`; needs `count` free enemy slots (byte 0 of a slot == 0) else **no spawn**; marks `group[0]=1`, copies the slot indices into `group+4..+43`, converts `+64`/`+68` (position), then calls the group's behaviour function through `+84` |
| `0x2d7b8…0x2d960` | group init loop at level load: for each group with `byte0 != 0` and `+88 == 0`, a 24-entry jump table (TOC−5148) on `type = group+44` stores the **behaviour TVector at `+84`** (the values saved in the resource at +84 are stale editor pointers and are ignored) |
| `0x33a64…0x37c74` | the 22 per-type behaviour routines (types 1–21 and 23), ~0x300 bytes each; each initialises its members' enemy slots (random x 64..352, staggered launch delays, constants below) then sets movement |
Group record fields (u32 index): `[0]` active byte, `[11]` enemy type, `[12]` member count (≤10), `[15,16,17,19,20]` formation/path parameters (unverified), `[21]` stale pointer, `[22]` special-spawn flag.
Enemy slot (176 B) offsets seen in the init code: `+0` active, `+4` spawn tick, `+8` type, `+24/+28` x/y floats, `+48/+52` sprite w/h, `+72/+76` half-size, `+80` animation index, `+84` probably HP, `+132` score.
`tools/re/enemy_types.py` re-derives `data/enemy_types.json` (and thus `src/enemies.rs`) from the executable; the animation indices are cross-checked two ways:
sprite size equals `+48/+52` for 20 of 22 types, and enemy sprites of types 8/14, 10, 17 and 23 are found pixel-exactly (>=97 %) in the original screenshots.

## Main loop, scrolling and input
| Address | Finding |
|---|---|
| `0xc6f0` | game-session function; owns the frame loop. Player structs at `r2+17496` (P1) and `r2+17088` (P2); `r27` = number of players |
| `0xd0dc…0xd2f0` | per-frame update order: `0x3d0fc, 0x2dc20, 0x4127c, 0x22170, 0x27434(p1,p2), 0x27dfc, 0x38b0c, 0x203a4, 0x2b884, 0x25264, 0x2cb40, 0x40b50, 0x45a6c, 0xc0a0, 0xc0c8,` **`0xc3e0` (scroll step)**, `0xc5b4, 0x3f554, 0x205ec, 0x39020, 0x2e118, 0x417a4, 0x39338, 0x236b4, 0x29b10, 0x28260, 0x2242c, 0x275c8, 0x2c480, 0x2d1d8, 0x257b4, 0x2633c …` |
| `0xc3e0` | **scroll step** (ported as `src/scroll.rs`): speed is only ever 0 or 1 px; at 32 px: refresh off-screen rows -1/+15 for the 3 layers (`0xc170`), update ground objects (`0x3b450`), **fire spawn trigger `0x2d9bc` (ASGM[row])**, advance the row (`0xfe2c`); row == height-5 raises the end event, row == height stops scrolling (`0xc0a8`) |
| `0x25074` / `0x250a0` | per-frame player input: clear flags `+96..+101` (if `+1 == 3`, i.e. alive), then fill them from InputSprocket (`0x8fec` polls 8 elements: thresholds `<0x3000_0000` / `>0xC000_0000` on axes) |
Player struct: see the ship section below (`+96 up, +97 down, +98 left, +99 right, +100 button 1, +101 button 2`).
Default bindings (`tset` resources, value = Mac keycode<<16): 1 Player = arrows + Command/Option; 2 Players = P1 J/L/K/I + Command/Option, P2 keypad 4/6/5/8 + Page Down/End; `setl` also lists mouse sets.
Screenshots fix the scroll position as `height*32 - 480 - scroll_y` px from the start (1565, 2301, 582, 676 px for levels 5, 8, 15, 17); **the original runs at 30 frames per second** (see "Frame loop" below), so the level scrolls 30 px/s.

## Ship, shots and weapons (ported: `src/ship.rs`, verified against the executable)
Method: each routine was disassembled (`tools/re/ppcdis.py`), lifted to pseudo-C (`tools/re/lift.py`), then **executed on random states in a PowerPC
emulator** (`tools/re/ppcemu.py`, `tools/re/gen_ship_vectors.py`); `tests/ship_golden.rs` requires the Rust port to reproduce the resulting 408-byte
player struct, all 60 shot records, the shot counter, the return value and the call log of the mocked callees bit for bit (28,500 random vectors
passed once; 570 are committed in `tests/vectors/ship_vectors.txt`). Corrections to the first notes: `0x22170` is the player-shot update (not
"collision"), `0x24544` is the ship-destruction tail, `0x26fd8` its overheat helper, `0x2b884` the pickup collision, `0x2cb40` the drone code; the
input flags `+96/+97` are **up/down** (see below).

Player struct, 408 bytes (P1 `r2+17496`, P2 `r2+17088`), fields used by the ported routines:
| Offset | Meaning |
|---|---|
| `+0` / `+1` | active / state (1 = appearing, 3 = playing; every update routine returns unless `+1 == 3`; set by the session loop through `0x2407c`) |
| `+4` | tick of the last state change (`0x2408c` / `0x24084`) |
| `+8` / `+9` / `+10` | player number (= shot owner) / input set / 1 = one-player game |
| `+12` | lives (`0x24140` sets 3, `0x24154` decrements, `0x2414c` reads) |
| `+20/+24/+28/+32` | x, y, vx, vy (f32; y grows downward) |
| `+36/+40`, `+44/+48` | sprite size 48x49, hit size 24x24 (set in `0x23de4`) |
| `+52` | maximum speed 7.2 (`0x240f0`), 9.0 with the speed-up pickup (`0x24128`) |
| `+60` | bank frame 1..5 (1 = straight, 2/3 = left, 4/5 = right; 3 and 5 at full speed) |
| `+96..+101` | input flags: **up, down, left, right, button 1, button 2** (up decreases vy; keyboard path `0x250a0`) |
| `+80/+84/+88/+92` | button-2 special weapon (`0x26b58`): `+80` charges per pickup (<=10), flag, tick of last use, charges left (25-tick re-arm, 3-tick repeat) |
| `+108` | button-1 latch (set while the button is held, cleared on release) |
| `+208` | frames button 1 has been held (capped at 64) — the charge of the beam |
| `+212..+252` | wave beam: active `+212`, x `+216`, y `+220`, vx `+224`, vy `+228` (-20), w `+232`, h `+236`, sprite `+240`, pattern `+244`, power `+248`, delay `+252` |
| `+256/+260/+264/+268/+272/+276` | charge/beam visual state (`+256` charge effect on, `+268` release effect on; ticks and frame counters) |
| `+280/+284/+288/+292` | over-charge: frames held, heat counter, flash flag, stage 0..7 |
| `+296` | **weapon level 1..16 = volleys per burst** (`0x26c94` adds a delta, clamped; `0x26c58` loads it from `0xd7a0`) |
| `+300` | volleys left in the current burst |
| `+304`, `+308/+309` | drone count (`0x2c87c` resets) |
| `+312/+316` | "trail" point: x = ship x + 14, y follows ship y - 140 at maximum speed per frame (used by homing/aiming code) |
| `+324` | weapon kind 0 = small shot (3x10), 1 = tall beam segments (3x60, four per volley), 2 = large shot (4x32); other values call the fatal-error routine |
| `+328` | spread 0..5 -> 1, 2, 3, 4, 6, 6 parallel shots (initial value 1 from `0x26800`; wave beam power is multiplied by it) |
| `+332`, `+348` | side-missile level and tick of the last launch (`0x277e4`, >40 ticks apart) |
| `+336` | tick of the last volley; `+341` rear shots enabled (`0x2169c`); `+344` pattern id written by the spawners |
| `+356/+360/+364` | charge-loop sound handle (sound 18) / "over-charge stream not started" flag / its handle (`0x276e0`, `0x27730`) |

Routines (all ported unless noted):
| Address | Role |
|---|---|
| `0x25264` | movement: +-2.5 per frame towards +-vmax, decay by 2.5 when no direction is held, bank frame, `x += vx`, clamp to 0..416-w / 0..480-h (velocity zeroed), trail follower. Frame order: input `0x250a0` precedes it |
| `0x21224` / `0x21268` | clear the pool / free a slot (counter `D[0x15dc]`; the free decrements unconditionally) |
| `0x217c8` + `0x21ad8` | front volley (kinds 0 and 2; spread table above; x offsets 17/25, 12/20/28, 8/16/24/32, 6/12/17/23/28/34 (the middle pair 25 px higher), 1 shot centred); needs `count <= 60-n` |
| `0x21538` + `0x21a00` | beam-kind volley: four 3x60 segments, vy -30, start ticks `tick + 2*i`, spawned 60 px above the ship |
| `0x2169c` + `0x21e1c` | two rear shots, vy +20, x offsets 12 / 20 from `x+7`, y = ship bottom |
| `0x22170` | shot update: skip kind-1 shots until their start tick, move, free when fully off screen, else test enemies with `0x2ead4`; non-piercing shots are freed on a hit (effect 13), piercing ones lose hp and end below 1 (effect 1) |
| `0x2ead4` | box-vs-enemies: slots 39 down to 0, skips inactive or `state >= 1`, inclusive overlap with the integer enemy box (`+56/+60/+64/+68`), damage by `0x2ec14` (or `0x41c88` when `+168` is set), stops when the attacker's hp is used up; returns the total damage |
| `0x26cf8` | **button-1 control**: new burst only more than 4 ticks after the last volley, volleys 2+ ticks apart; sound 10 / 33 / 7 per kind; side missiles, rear shots and drones are triggered from here; keeping the button down (latch) stops shooting and charges the beam |
| `0x27080` | over-charge: after 120 held frames a heat counter runs, stage threshold `max(6, 30-6*stage)`, flashes 3 frames before it, sound 26; stage > 7 destroys the ship (`0x26fd8` + `0x24544`) |
| `0x271bc` | release the beam (charge > 20 frames): size/sprite/power by charge: <35: 17x48 power 3, <50: 19x72 power 6, else 21x96 power 16; power = base*spread + k*(level-1), k = 2/3/4; `k` sounds of id 24 |
| `0x27434` | beam update: appears after 5 frames above the ship, vy -20, ends off screen or when its power is used up |
| `0x2dc20` | ship-vs-enemy body hit box: sprite shrunk 6 px on each side (`x+6, y+6, x+w-6, y+h-6`, integer positions) — **read from the disassembly, not covered by vectors** |
| `0x2b884` | ship-vs-pickup hit box: `x+2, y+2, x+2+(w-4), y+2+(h-4)` (verified by the pickup vectors, including exact-edge contacts) |

Input: the keyboard/InputSprocket poll (`0x8fec`) maps axis value `< 0x3000_0000` to the low-end flag and `> 0xC000_0000` to the high-end flag;
X low = left (`+98`), X high = right (`+99`), Y low = down (`+97`), Y high = up (`+96`). The `tset` key order (left, right, down, up, b1, b2) therefore
matches the flags (verified; the earlier "down, up" order of the flags was wrong).

### Ship life cycle (ported and verified: `damage`, `destroy`, `reset_weapons`, `init`, `shield_reset`, `session_step`)
| Address | Role |
|---|---|
| `0x243a0(ship, amount)` | damage intake (callers `0xe6f4`/`0xe720` for P1/P2, `0x2dc20` deals 5 per touching enemy): state must be 3; shield `+112 <= 0` -> `0x24544`; else shield -= amount, hit flash, HUD `0x1f69c`, sound 66 (set 1) / 67 x3; shield exactly 0 starts the low-shield warning (`+128`, 10 ticks later it blinks, sound 69) |
| `0x24228(ship, keep)` | shield reset: 6 (a kept value 6..12 survives when `keep`), clears hit/warning animation |
| `0x24544(ship, quiet)` | destruction: sounds 31x3, 4x3, 11x5, effects (kinds 3, 14, 6, 11, 7/8, 2 with staggered delays 0..30; `quiet` removes the delays and the first explosion), then **drops the weapons as pickups** at the ship centre (`x+hit_w, y+hit_h`): kind 5 x(spread-1), 6 x(level - start level), 18 x(special - start level), 7 x missiles, 10 if rear shots, 12 x drones, 19 if speed-up active (speed back to 7.2), 2 x (coins % 10), 21 x (coins / 10); weapons are reset with `0x26680`. If the **`+340` insurance flag** is set the weapons are kept: only the drones drop and the flag clears. Common tail: drones removed (`0x26a3c`), HUD, state = 2, state time, invulnerability blink (`+64/+65/+68`), position back to the start |
| `0x26680(ship, keep)` | weapon reset: kind 0, spread 1, level and special charges = start level (`D.13ec`, via `0xd7a0`), missiles 0, rear off, drones 0; level raised to the start level (max 16); clears charge, burst, latch and the special-weapon state |
| `0x23de4(ship, keep)` | (re)start at level start: sizes 48x49 / hit 24x24, start position, trail, **lives + 1**, state 1, timers, speed 7.2 unless `keep`, shield + weapon reset, `D.13f0 > 0` adds one weapon level / special charge, appear flashes `0x256b4` |
| `0xc9d0..0xcc9c` (session loop, per player) | state 1: after 60 ticks (120 when `lives == 1`) a life is used (`lives -= 1`): `lives > 0` -> state 3, else the player is out (game over if solo or the other player is out). State 3: invulnerability ends 90 ticks after entering. State 2: after 60 ticks (40 when `lives == 1`) -> state 1; on the last life the ship is deactivated, game over screen (`0x28510(3)`, sound 28) if solo or the other player is out; otherwise appear flashes + shield reset |
`D.13e4` is the game tick, `D.13e3` the display mode byte, `D.13f4` the "game running" byte (cleared on game over).
Positions P1/P2 are `r2+17496` / `r2+17088`, i.e. data offsets `0xC458` / `0xC2C0`.

Not ported / open (the ship draw routine `0x257b4` and beam drawing `0x275c8` are ported now, see "Drawing"; side missiles and drones are ported, see "Drones and side missiles"): the button-2 special (`0x26b58`), the ship draw routine `0x257b4` (also holds the timers for the flash/blink animations), the enemy-body collision loop `0x2dc20`
(only the ship-side hit box is read), beam drawing (`0x275c8`), and everything on the enemy side (`0x2ec14`, `0x41c88`).
Sound effects go through `0x45bac(id, priority, arg)`; ids so far: 7/10/33 shots, 24 beam release, 26 over-charge warning, 41 missiles, 11 ship destroyed.
Lifter caveat: `tools/re/lift.py` prints a callee-saved copy (`r26 = r3`) after an `r3 = ...` write in some prologues; the raw disassembly is authoritative.

## Power-ups (ported: `src/pickup.rs`, verified against the executable by `tests/pickup_golden.rs`, 1000 vectors)
Pool: 20 records of 128 bytes behind the pointer `D.0fc4`, live counter `D.1754` (signed; `0x29ebc(slot)` clears `+0` and decrements it unconditionally).
| Address | Role |
|---|---|
| `0x2a040(r5 kind, r6 variant, r7 scatter, r8 announce, f1 x, f2 y)` | spawner. Kind 1 is ignored. **Substitutions:** kind 4 and kind 20 become 10 coins (3x sound 59,5,5 for kind 20); kinds 5 (spread), 6 (level), 7 (missiles), 9 (insurance), 10 (rear), 18 (special), 19 (speed) become kind 20 when *both* players are inactive or already maxed (queries `0xe81c..0xea24` -> `0x2680c`, `0x26ccc`, `0x26898`, `0x26948`, `0x26964`, `0x26b2c`, `0x240fc`: spread >= 5, level >= 16, missiles >= 3, keep, rear, special >= 10, speed == 9.0); kind 3 with variant 5/6/7/8/9 becomes 2/3/4/5/10 score items of variant 4. Then: counter >= 20 or no free slot -> nothing. The record is filled (stale bytes of a freed slot survive wherever the handler does not write), `+60` = scatter flag, `+88` = 1, sprite data `113/5/48/48`, `+100` = tick. Sizes: coins 26x26 (30x28 with look flag `D.13d8`, `0xe090`), score items 24x20 / 29x20, kind 8 29x28, everything else 48x29. **Quirk:** the handlers store `r7` into `+104` (animation frames) without reloading it, so for kinds 7, 9, 10, 12..19 it is the constant 5 left by the prologue, not the caller's argument. Common tail `0x2b4bc`: centre on (x, y), y clamped to >= 0, sprite offsets `(spr - size) / 2` (+3 on y when h == 29), and for scatter pickups a random velocity: `Random(1,3)` speed, `Random(10,16)` x, `Random(0,1)` sign, `Random(10,16)` y, sign only when y > 120; direction normalised with `fmuls/fmadds/sqrt/fdivs`, acceleration `0.4 * direction` (double multiply), counter + 1, sound 16 when `announce` |
| `0x2b884(p1, p2)` | per frame: life 185 ticks (290 for kind 4); expired -> free + effect `0x22670(cx, cy, 14, 0, 39, scatter == 0, 0, vx, vy)`, centre from `x + w/2`; the last 45 ticks toggle the visibility byte `+88`; per-kind frame animation (jump table `D.6adc`: coins count down every 3 ticks, score items count up every 3, kinds 5/6/12 flip 1/2 every 4, kind 11 every 3, kinds 7/9/10/13 ping-pong every 2, armor/special/speed kinds 14..19 ping-pong every 4); movement `0x2bd78` (still pickups drift with the scroll speed `D.13c4`; scatter pickups bounce inside the screen, speed limit 6 per axis flips the acceleration); collision against each player in state 3 (player 1 first; a collected pickup is gone for player 2) |
| `0x2c134(slot, ship)` | collection: sound 34 (coin) / 1 (score item) / 8, hit flash (state 3 only), aura effect `0x29714`, then per kind: 2 `0x24348` +1 coin (max 99); 3 `0x26210` score (variant 2..9 = 500 1000 2000 4000 6000 8000 10000 20000, larger = the points, x armor level `+172`, extra life at `+156`, thresholds 30000 then +30000+10000*awarded); 5 `0x26838` spread +1 (max 5); 6 sound 63 x2 + level +1 (max 16); 7 `0x268c4` missiles +1 (max 3); 8 `0x24164` life +1 (max 12, HUD `0x1f848`, sound 65 x3); 9 insurance (`0x268f4`, sound 22); 10 sound 25 + rear on; 11 `0x26fd8` smart bomb (HUD hooks `0x3fbfc`, `0x2e6a8`, `0x38a1c`, sound 11 x3); 12 `0x2699c` drone (state 3, fewer than 2, hook `0x2c760`); 13 sound 40 + shield +6 (max 12); 14..17 `0x265ec` armor level 2..5 (sound 17, or 53 when not higher); 18 sound 64 + special +1 (max 10); 19 `0x24128` speed 9.0; 21 (big coin) sound 59 x2 + 10 coins |
Score fields of the player struct: `+152` score, `+156` next extra-life score, `+160/+161` refresh/flash bytes, `+164` flash tick, `+168` extra lives earned.

## Frame loop and pacing (used by `src/game.rs`)
`0xc6f0` loop, per frame: `0xed04` (tick `D.13e4` += 1, so **one tick per frame**), input poll `0x8fec`, per-player state machines (`0xc9d0..0xcc9c`), then the update order listed above
(`0x25074/0x250a0` input, `0x26cf8` button 1, `0x26b58` button 2, `0x3d0fc`, `0x2dc20`, `0x4127c`, `0x22170`, `0x27434`, `0x27dfc`, `0x38b0c`, `0x203a4`, `0x2b884`, `0x25264`, `0x2cb40`, ..., `0xc3e0` scroll),
the draw routines (`0x2242c` shots, `0x275c8` beams of both ships, `0x2c480` pickups, `0x2d1d8` drones, `0x257b4` ships, `0x2633c` HUD ...) and finally the frame limiter at `0xd3b4`: with `D.13ca` set (default 1)
it adds 2 to the last `TickCount` value and spins until `TickCount` reaches it => **2 ticks of 1/60.15 s = 30 frames per second** (the game's read-me says the same). All speeds in this document are per frame.

## Drawing (ported: `src/draw.rs`, `src/ship_draw.rs`, `PickupPool::draw`, `src/blit.rs`; verified by `tests/draw_golden.rs` (840 vectors, 18 mutations) and `tests/blit_golden.rs` (600 vectors, 15 mutations))
| Address | Role |
|---|---|
| `0x2f88 / 0x3170 / 0x3264 / 0x344c / 0x3540` (group, x, y, w, h, anim, frame) | the five blit wrappers: plain `Anim` (blitter `0x3f08`), 50 % `Trans` (`0x4078`), checkerboard `Dither` (`0x486c`), `Shadow` (`0x46a8`), `Flash` (`0x4a90`). Each adds a 64 px margin to x/y, **rejects** (does not clip) a sprite that crosses the edge of the 704 x 608 off-screen GWorld (irrelevant for <= 64 px sprites: only fully off-screen ones are rejected), treats `frame <= 0` as "draw nothing" and fetches the compiled sprite through `0x3e70(group, anim - 1, frame - 1)`. Sprite id = `sets.json` set 1000 `entries[anim-1].first_id + frame - 1` |
| compiled sprite | s16 width, 2 unused bytes, then 4-byte commands `[cmd][u24 n]`: 1 = n literal pixels (padded to 4 bytes), 2 = n transparent, 3 = end of row, 4 = end. All 1248 streams use maximal, non-empty, alternating runs and cover the full row width (checked), so the opacity mask of a PNG determines the stream exactly |
| `Trans` | `dest = TRAN[dest * 256 + src]` (orientation now verified), table `TRAN 1000` loaded at start (`0xbdb8`) through the handle `D.8a04` |
| `Flash` | opaque area of the sprite: `dest = DARK129[dest]` (orange); `Shadow`: `dest = DARK128[dest]` but the sprite is **squashed**: columns `c % 3 == 2` and rows `r >= 3 && r % 3 == 0` are dropped and the rest closed up (2/3 x 3/4 size) |
| `Dither` | parity flag starts at 0 and flips per pixel / odd run / row end; 4- and 2-pixel chunks copy the odd positions when the parity is 1 and the even ones when it is 0, the leftover single pixel of an odd run is copied when the parity is 1 (the opposite phase) — quirk reproduced exactly |
| `0x257b4(ship)` | draw step *and* the ship's cosmetic timers (HUD refresh, muzzle flashes anim 49, charge glow anim 28 + aura anim 113, invulnerability blink, hit flash (2x `Flash`), low-shield warning anim 19/20 + sounds, shield ring anim 18, bomb sight anim 22/25, 23 over a ground target via `0x3fec8`); shadow at (x-24, y+44) when `D.12bc` |
| `0x275c8(ship)` | beam: `pat` segments of 32 px, anim `beam.anim` frame `i+1`, hidden while `delay > 0` |
| `0x2242c` | shots at integer positions; `0x2c480` pickups (offscreen free test `x < -64 || x+w > 480 || y < -64 || y+h > 544`, shadow, aura when `D.12be`, caption anim) |
Options `D.12be` (translucency: `Trans` instead of `Dither`; pickup auras) and `D.12bc` (shadows) are read through `0x29a4` / `0x2994`; their defaults are assumed to be on.

## Enemy damage and ship contact (read from the disassembly; used by the debug targets of `src/game.rs`, not covered by vectors)
`0x2ec14(slot, amount, owner)`: `+100 += amount`; it dies when `+100 > +96` (max hp) and the damage reported back is `max - taken_before + 1`, otherwise `amount`. `0x2dc20`, for every living enemy: if the ship is in
state 3 and not invulnerable (`+64`), and the boxes overlap with strict inequalities (ship box `x+6, y+6, x+w-6, y+h-6`, enemy box truncated to ints), the ship takes `(max - taken) + 1` damage (`0x243a0`) and the enemy
takes 5 (`0x2ec14(slot, 5, 1)`).

## Sound engine (ported: `src/audio.rs`; verified by `tests/audio_golden.rs`, 360 vectors from the original code in the emulator, 26/26 non-equivalent mutations caught)
| Address | Role |
|---|---|
| `0x45928(6)` (called from `0xba7c`) | `init(maxVoices = 6)`: checks Sound Manager >= 3.0, calls `0x51424(6, stereo = D.1be5 = 0, rate 0)`; **mono**, output rate `0x56EE8BA3` (22254.545 Hz, 16.16), 16 voice slots, voices mixed <= 6, two 1024-sample double buffers (`0x524c8`); sets `D.1be4` (sound option) = 1; clears the delay queue |
| `0x45b14` | loads the 72 effects: `table[i] = 0x51d98(10000 + i)` (`snd ` resource widened to s16 `byte - 128`, header +4 = chunk count `ceil(len/1024)`, +8 = rate 16.16, data at +12, zero padded) |
| `0x45bac(id, prio, delay)` | play: refused while `D.13e3 == 1` (display mode byte) or `D.1be4 == 0`; priority outside 1..5 becomes 1; `id` must be 0..71 (sign-extended byte); `delay != 0` puts it in the 5-slot queue at `D.10d8` (12-byte slots: id byte (-1 = free), due tick `+4`, priority `+8`) or, when the queue is full, plays it at once |
| `0x45a6c` (main loop, after `0x25264`/`0x2cb40`/`0x40b50`) | queue pump: slot plays when `due <= tick` (unsigned) |
| `0x45ca4(id, prio)` -> handle, `0x45d54(id, prio, rate)`, `0x45e24(id, prio, req, callback, param)` -> handle | same gating; `0x45d54` takes a playback-rate multiplier (16.16, player 2's low-shield warning passes 36864), `0x45e24` a callback + parameter (over-charge stream: callback `0x2766c`, parameter = player number -> `0xe79c/0xe7ac` set that ship's `+360` "stream ready"; enemy streams: callback `0x4123c`) |
| `0x517f4` / `0x51844` | request -> voice. Volume 256 is clamped to 128 (= `(s*vol)>>7` is the identity). Voices are kept in priority order: a request goes before the first voice with `prio <= req.prio` and `volL+volR <= req.volL+req.volR` (always true for equal priorities, so **newer sounds go first among equals**); all 16 slots taken and nothing outranked -> dropped; otherwise the last voice falls off. Handle = counter (starts 1, +2 each). `ratio = FixDiv(soundRate >> 8, outRate >> 8)`, `step = FixMul(ratio, playRate)`, duration `max(1, floor(chunks / step))` buffers |
| `0x52030` -> `0x52294` | buffer callback, mono mixer: per output sample every voice adds `(s * 128) >> 7` for the first 6 voices; position accumulator (16.8) `acc += step >> 8`, sample index `acc >> 8`; when the index did not move since the previous output sample (slowed playback) the sample is averaged with the next; sum clamped to +-127, +128 -> unsigned 8 bit. After the buffer each voice's remaining count drops by 1 and finished voices are removed (callback selector 1) |
| `0x51b4c(handle)` / `0x51d2c(handle)` | stop / count voices; **handle 0 = every voice**. The game calls both with the charge / stream handles even when they are still 0 (see below). Stop makes callback selector 2 with the *stale* parameter block (`D.11c8`, +16) of the last voice that had a callback |
Quirks kept: (1) releasing a charged beam asks "is stream handle 0 playing" when the over-charge stream (charge > 54) was never started: that is true whenever any voice plays, so **all sounds are stopped** at that moment; (2) a stopped stream's callback uses a stale player number and may flag the other ship; (3) with only `FixMul` documented (rounds to nearest, ties up, Apple Tech Q&A OPS 20) the `FixDiv` truncation is an assumption that only affects the sounds recorded at 11127 Hz.
The master volume (`D.1bdc`, 0..256 in steps of 32 from the preference key 51) is only passed to `SetDefaultOutputVolume`; it is not part of the mix. The sound option is toggled by a menu item (`0x45f30`).

## Enemy runtime (ported: `src/enemy.rs`, `src/enemy_ai.rs`, `src/enemy_draw.rs`; verified by `tests/enemy_golden.rs` and `tests/enemy_draw_golden.rs`)
All 22 per-type init/update routines (types 1-21, 23), the spawn/damage/frame framework (`0x2da30`, `0x2dc20`, `0x2ec14`), the draw/cull pass (`0x2e118`), the smart bomb (`0x2e6a8`) and the group-wipe bonus are run
from the original code in the emulator (`tools/re/gen_enemy_vectors.py`, ops `S F H U X K D P B b W V`) and must match bit for bit, pool and group digests plus every outgoing call (effects, sounds, score, shots).
Vector kinds: 400 short random vectors, 32 long ones (types 15-23), 51 `K` (smart-bomb box edges), 220 `poke` (random slot pokes: position, velocity, acceleration, counters) and 260 `poke2`/type-4 vectors
(velocity just above 0, window edges, state 6/7/24/25, enemy next to a ship at +-45..62 px).
Mutation sweep over every numeric literal of `enemy_ai.rs` (1118 mutants; `x * 1.25 + 0.05` or `n + 1`): 1096 caught. The 22 that survive are equivalent or unreachable:
- `-0.2 -> -0.2` (9 times): the mutation is the identity, an artefact of the sweep's rounding.
- `160 -> 161` (type 6 init): stores 0.0 into freshly zeroed memory.
- `r <= 3` (type 6 init): `random(0, 3)` is masked to u16 and is always <= 3.
- `s < 24 -> s < 25` (type 15): the branch is entered for `s >= 23`; for s = 24 it stores 24 again.
- `turret_turn` bounds (`step < 1`, `step > 24`, `s > 24`, wrap to 1): the angle is 0..359, so step is 1..24, and `st < step` implies `st + 1 <= 24`.
- type 23 `y > 80` / `y < 380` (and the `r4` branch with its 0.2 accelerations, 2 mutants): the two comparisons can never both fail, `r4` stays set only for NaN (the original is the same).
- the distance measure of the nearest-ship helper (`dx + dy < 0.0`): only a sum in [0, 0.05) differs; a boundary tie that needs an exact coincidence of float positions.

## Enemy shots (ported: `src/enemy_shot.rs`; verified by `tests/enemy_shot_golden.rs`)
Pool `D.1050` -> 0x196f0: 30 records of 80 bytes, live count `D.1a04` (< 30 to spawn), gate `D.1a00` (no spawn before this tick; set to tick + 90 by "clear all"). Record: +0 kind, +4 active, +5 muzzle-flash flag, +8 anim, +12 frame,
+16 frames, +20 stamp, +24 period, +28 shadow, +29 draw-on-top, +32/+36 pos, +40/+44 vel, +48/+52 size, +56/+60 half size, +64/+68 flash pos, +72 flash tick, +76 damage. Freeing (0x37fb0) clears only +4 and decrements the count.
* `0x383b8(kind, target, flag, x, y)`: tick >= gate, target ship alive, count < 30, x in 0..416, y in 15..400; one 9x9 shot aimed at the ship centre at speed 6. `0x37fd4`: the same with x in 32..480, plus a muzzle flash and sound (used by boss code).
* `0x38754(kind, count, sound, mode, p, flag, x, y)`: mode 1 / 2 = ship 1 / 2 must be alive, 3 = either (and then the target is ship 1, `li r28,1`), anything else refuses; kinds > 9 refuse; jump table at 0x6d3c; the sound plays at priority 2 (3 for kinds 1, 2, 5, 7, 8) when the byte is >= 0.
  Kinds 0 / 2 (9x9, damage 1), 3 (11x11, +12 = p), 4 (8x8) and 9 (8x8) fire `count` aimed shots with speeds base + 0.5, + 1.0 ... (bases 6, 9, 8, 7). Kinds 1 (8x16, damage 3, vy 14), 5 (8x56, damage 4, vy 16), 6 (16x16, shadow, vy 2.2), 7 / 8 (8x64, vy 16) fall straight down.
  The aim is `(half size) + ((ship + ship half size) - origin)` per axis, normalised in f32 (fmuls + fmadds, f64 sqrt rounded).
* Update `0x38b0c`: animation (period +24), flash (2 ticks, follows the scroll), move, drop when x+w < 0 / x > 416 / y+h < 0 / y > 480, ground-object test `0x2ca38` (a destructible object: the shot explodes), ship hit boxes (box inset by 6, strict overlaps, only a living, unshielded ship): free, `0xe6f4` / `0xe720(damage)`, effect 5.
  The original calls `0x2ca38` a second time with the same box when ship 1 is hittable; it is deterministic, so that call can never succeed (the port repeats it for fidelity).
* Draw: pass A (0x39020) draws the shots with +29 == 0 before the enemies, pass B (0x39338) the others after them: shadow (if +28), kinds 4 / 9 get a 13x13 glow (anim 115 / 116, translucent when the option is on), the shot, the flash (20x20, anim 50).
Mutation sweep (408 mutants): the survivors are the initial values of `ShotPool::new` (overwritten by init), the high bytes of two zero words, `live >= 30` inside the burst loops (no free slot is left at 30), animation `nf > 0` (no kind has one frame), tiny-boundary cases of the left/top cull, and the dead second `0x2ca38` branch.

## Title screen (ported: `src/menu.rs` - the buttons, verified by `tests/menu_golden.rs`; `src/title.rs` - pictures and pointer / key driver, not verifiable)
The application's main window is the title screen; its loop is `0x1a500` (pointer tracking block `0x1a6f0..0x1a770`), the window set-up `0x1aa68`.
* **Set-up (`0x1aa68`)**: `0x1b054` fills eleven art rectangles on an off-screen sheet (`SetRect`, slot k = (0, 32k) .. (right_k, 32k + 32), widths 79 79 126 123 57 77 82 112 105 66 36) and reads the screen rectangles from the application's `'Rect'` 1000..1010 (`0xa05c`);
  the pictures 9100..9109 (+ 9110 = Controls) are drawn with `DrawPicture` (`0x1a0c`, destination rectangle = the sheet slot, i.e. **scaled** to it) at x = 0, the hilited pictures 9200..9210 at x + 200;
  then the background 9000 (`0x1970` + `0x7c`), the logo 9300 into the interface file's `'Rect'` 1 (133, 116, 339, 301), an extra icon (resource 129 drawn by `0x17e8`, 32 x 31 at the Quit button's (left + 13, top + 53), i.e. below it) when `0x1e260` says so, then every button is copied from the sheet to its screen rectangle with `0x170` (`CopyBits`, transfer mode 36 = "transparent", `0x170`'s callers pass the mode).
* **Hit test (`0x1c098`)**: points are made window-local (origin `D.9368/936c`), rectangles are tested in the order 1 Player (1), 2 Players (2), Register (6; skipped when `D.153a` != 0 = registered copy), Controls (7), Preferences (8), High Scores (9), Credits (10), Quit (11). Host / Join / Editor (3..5) exist as rectangles and pictures but are never tested (network and editor were never shipped). `PtInRect`: top / left inside, bottom / right outside.
* **Hover (main loop)**: entering a button un-hilites all (`0x1c5cc`), hilites it (`0x1c6f8`) and plays sound 20 (priority 3); leaving all buttons un-hilites; both restart the idle timer (`TickCount` stored in `r26`). Moving inside the same button does nothing.
* **Click (`0x1c998`)**: sound 29 (priority 3), then while `StillDown`: hilited when the pointer is inside, all normal when outside; after the release everything is drawn normal and the result is "released inside". The hover state `r25` is not touched, so the button stays un-hilited until the pointer leaves and re-enters.
* **Dispatch table `D.5bb8`**: 1 -> `0x1cb80` (one-player game at level 1; with the option key held a level chooser `0xfcb8` / `0x1b254` comes first), 2 -> `0x1cbe8` (two players), 6 -> `0x1cc8c` (registration), 7 -> `0x1ccbc` (controls), 8 -> `0x1cd10` (preferences), 9 -> `0x1cd5c` (high scores), 10 -> `0x1ce24` (credits), 11 -> `0x1cee0` (quit); 3..5 -> `0x1be48` (nothing).
* **Verification**: `tools/re/gen_menu_vectors.py` runs `0x1b054`, `0x1c098`, `0x1c5cc`, `0x1c6f8`, `0x1c998` and the hover block natively in the emulator (Toolbox calls mocked and logged) on 150 random layouts / pointer paths (registered and unregistered, origin offsets, overlapping rectangles, 106 mutants of `menu.rs` swept). The Rust code returns the same `Blit` / `Sound` / `Refresh` requests.
* **Not verified (assumptions of `title.rs`)**: the extracted pictures are opaque; the transparent colour of the copies is taken as the green (0, 238, 0) of the normal pictures and the white of most hilited ones (the original remaps to the game palette, which has one value); the buttons are drawn 1:1 instead of through the scaled sheet; the rotating text (9001 / 9002, idle cycle), the copyright box (`'Rect'` 2: 25, 384, 347, 403, picture 9400) and the icon 129 are not drawn. Keyboard / pad navigation (`Menu::focus_step`) and "Esc = back to the title" are additions of the port (the key handler `0x1be60` is not read yet).

## Title-screen dialogs (Credits ported: `src/credits.rs`; verified by `tests/credits_golden.rs` (300 text layouts, 35 whole pages, 14 wipes from `tools/re/gen_credits_vectors.py`) and `tests/credits_flow.rs`)
The dispatch targets of `D.5bb8` (see "Title screen") are thin wrappers:
* **Controls (`0x1ccbc`)**: stops the music option check (`0xae6c(57)` -> `0x456d0(2)`), shows the menu bar (`0x9768`) and calls `0x8f7c` = **`ISpConfigure`**, Apple's Input Sprocket configuration window (an operating-system dialog, no game resource): nothing to port; the port has its own key bindings (the title screen help line, F5 overlay).
* **Preferences (`0x1cd10`)**: same preamble, then `0x6810` (1856 bytes, 464 instructions: a real dialog with the preference array `D.932c`; not ported yet).
* **High Scores (`0x1cd5c`)**: `0x194ac` (see "High scores" below; returns "play" which starts a one player game with `0x1cb80(0)`), then the title is redrawn (`0x1aa68`) and shown with `0x1bafc(12)`.
* **Register (`0x1cc8c`)**: `0x5bf60(1)`, the shareware registration tool; not in a registered copy.
* **Credits (`0x1ce24` -> `0x10b80`)**: the key state is read with `GetKeys` (`0x9858` tests a bit): Control (59) -> 1, Option (58) -> 2, Command (55) -> 3, the last one down wins; `0x10b80(mode)` shows the pages `0..16` (mode 0), `100..109` (1: recommended reading), `200..203` (2: viewing, playing, listening, not recommended), `300..302` (3: human rights abusers, things to do, "In memorium" with the picture `'IMAG'` 128).
  Each page (`0x10dcc(n)`; 35 pages in all plus page 400, the Donne quotation, which no caller of `0x10b80` reaches; `0x1e808` shows it alone, see the `X` key) is nothing but calls of `0x29054(string, x, y, hilite, mono)` and for page 302 `0x17e8(128, topleft, bottomright)`; the strings, positions and the glyph table are extracted by running the original (`gen_credits_vectors.py --data`, called by `mr_extract.py` -> `layout/title_text.txt`).
  The text font is the sheet of pictures 9001 (normal) and 9002 (highlighted, 52 lines lower), 338 x 52 each: capitals and lower case share the glyphs of the first row (A..Z), the second row holds digits and punctuation (`0x28750` fills 128 rectangles with `SetRect`); white is the transparent colour (CopyBits mode 36 into the off-screen buffer, `0x1cc`); glyph advance = width + 1 (12 with the mono flag), x = -1 centres on 640, -2 on the 416 wide playfield + 64 (and moves y down 64); bytes >= 128 are skipped.
  Each page is built on the off-screen buffer (background 9000, the text) and shown by **`0x1bafc(8)`**: two bands of 8 lines (rows 0..8 and 472..480) move by 8 lines per tick towards each other, each tick the rows under the bands are copied from the off-screen buffer to the window, until the moved distance reaches 480 - 16 (58 ticks; the new picture has closed in from both edges after 29, the rest is the original's idle loop; rows 0..8 and 472..480 are never copied). A page stays 240 ticks counted from the end of its wipe (the first page: from the start), the loop ends when a mouse button is pressed (sound 29, priority 3) or any key goes down (sound 29; the key N starts a one player game instead); events during a wipe wait for its end; after the last page it ends without a sound. The title is then drawn again (`0x1aa68`) and shown with `0x1bafc(12)` (38 ticks).
  On the port: ticks = 2 per logic step; Mac Control / Option / Command = Ctrl / Alt / Shift held when the button is released (the Windows key belongs to the system).
* Credits text of the shareware pages (`0x10dcc`) is data of the game: it is extracted from the executable into `assets/layout/title_text.txt` and not committed.

## High scores (ported: `src/scores.rs`; `tests/scores_golden.rs` against `tools/re/gen_scores_vectors.py`: 80 qualify, 120 name entry, 40 screen vectors; 18/18 mutants caught)
* **Table** (`D.c674`, 332 bytes, the `SCOR` 128 resource, copied there by `0x19400`; saved with the preferences in `0xa4d4` / loaded in `0xa75c`): 12 Pascal strings of 21 bytes (`+0` / `+21`: the last name player 1 / 2 entered = the edit field's start text; `+42 + 21 i`: the 10 names), 10 scores `+252 + 4 i`, 10 levels `+292 + 4 i` (`0xd7a0` = `D.13ec` = the game's current level), all big endian.
  Factory table: 10 x "Anonymous", 10000 .. 1000 (steps of 1000), levels 10 .. 1; last names "Philip K. Dick" / "P.J. O'Rourke".
* **End of a game** (`0x1b508(start level, 0, players, 0, 0)`, called by `0x1cb80` from the title; two players = `(level, 0, 2, 0, 0)`): runs the session loop `0xc6f0` (it ends when the last ship is out of lives - `ship::session_step` -, the last level is won, or the quit key `0x98cc`), stores the scores (`0x261e0` -> `D.1524` player 1, `D.1520` player 2). No high scores when the start level is above 1 (`r25`; the level chooser) - the port also refuses after the debug keys / R / [ ] . Then `0x199dc(player)`: **qualifies = score strictly greater than the 10th row**; the ship flag `+206` (`0xe74c` / `0xe774`) is only ever written 0 in this program. Two players: the higher score enters first (ties: player 2 first), 40 ticks between the entries when the first one was made; before the first entry 15 ticks and `0x19a08` (every other line black when Line Skipping is on, then a `patOr` paint of the system pattern `GetIndPattern(0, 4)` over the playfield; the pattern list is a System file resource, not in the executable: not ported). Afterwards sound 36 (priority 4); if any name was entered the High Scores screen follows, otherwise the title.
* **Name entry** (`0x19b28(player)`, dialog `DLOG` 1000 / `DITL` 1000: text, edit field, OK = 1, Cancel = 6, portrait 11000 / 11001 drawn by `0x1a0c`): leaves at once when the score is less than the 10th row (the entry itself accepts a score *equal* to it); sound 5 (priority 3) on opening; the text is "You are a High Scorer!..." for one player, "Player 1 / 2 is a High Scorer!..." for two (strings `D.5aa5 / 5ad4 / 5b07`, in `layout/title_text.txt`); the field starts with the player's last name. The key filter `0x1a0cc`: Return / Enter = OK, Backspace = sound 20 (priority 5), any other key: sound 29 (priority 5) while the field holds fewer than 20 characters, else a beep and the key is refused. After OK: insertion by shifting rows down while `score > row` (a score equal to a row goes below it; the 10th row falls off), level stored, name = typed (empty -> "Swoop", exactly "david wareing" -> "Swoop!" and sound 5 again), sound 30 (priority 3), the name becomes the player's last name (when it has exactly 20 characters its length byte is set to 19), the row is remembered in `D.1518 / D.151a` for the blinking.
* **High Scores screen** (`0x194ac`, rows by `0x19894(row, y, hilite, refresh)`): picture 9000, rows at `y = 70 + 35 i`: rank + "." at x 120, name at x 160, the score at x 450 with the fixed width flag (all with `0x29054`). With new entries: the background is drawn again, the row rectangles (x 100..580, 26 high) are saved into a strip buffer, all rows are drawn again, then 20 ticks, then 5 times: 8 ticks (rows restored from the strip = hidden, shown), 8 ticks (rows drawn again, shown) - the blinking. Then the events are flushed and the screen waits until 600 ticks after the start: a **mouse click or any key leaves, N / n leaves with sound 30 (priority 3) and starts a one player game** (`0x1b880` -> `0x1cb80(0)`), otherwise it leaves after the time-out. The title is then drawn again with the wipe `0x1bafc(12)` (the port shows it at once).
* The name entry is an operating system dialog; the port draws its own panel (same items, larger because the game's bitmap font is twice as high as the system font) and keeps the table in its own file (`highscores.bin` in `%APPDATA%\mars_rising_rs`, or `$XDG_DATA_HOME` / `~/.local/share`; the original keeps it in its preferences file).

## Ground-object runtime and bombs (ported: `src/ground.rs`, `src/ground_init.rs`, `src/bomb.rs`; verified by `tests/ground_golden.rs`, `bomb_golden.rs`, `clear_golden.rs`)
Everything below was read from the disassembly and then checked by running the original routines in `ppcemu.py` on synthetic levels (`tools/re/gen_ground_vectors.py`,
`gen_bomb_vectors.py`, `gen_clear_vectors.py`; `ground_dump.py` dumps the original's pool after an operation to find the first differing record).
* **Data:** 250 records of 168 bytes (pool `0x1a818`), the active list `0x1a050` (50 x i32, `-1` = free), the decal list `0x1a118` (50 x 32 bytes), muzzle tables `0x1a758` / `0x1a7b8`
  (towers of kind 1/24 and kinds 4/25), counters `D.1a60` (targets), `D.1a64` (destroyed), `D.1a68` (activated).
* **Level load** `0x3ade0` (records from the `__GS` resource, conversions `0x191ac` / `0x19248`, the per-kind set-up `0x3b9b0` = `ground_init.rs`, generated from the disassembly by
  `tools/re/gen_ground_init.py`, anchor `0x3b248`), target count `0x3acb0`. Variants 6..8 start at X = 416 (not 448).
* **Activation** `0x3b450` / `0x3b4f8`: called from the scroll step `0xc3e0` at every 32-px boundary, **before** the spawn trigger `0x2d9bc`; it activates the markers of row `counter + 1`
  (and the partner `next_link`). **The row counter starts at 16**, not 0 (see `scroll.rs`): rows 0..15 are the first screen and are never activated. `START_DELAY` (+144) = load tick plus activation tick.
  With the real level 5 the objects activated this way land on exactly the pixels of the verified static layout (kinds 2, 4, 7, 21).
* **Update** `0x3d0fc` (nothing happens while the scroll step `D.13c4` is 0): kind -> handler by the jump table at `0x6f58` (kinds 1, 2, 4, 7, 8, 15, 22, 24, 25 have their own; the rest use
  the default shooter, which needs the flag byte +50 that no initialiser sets, so those objects are inert). Shots: `0x383b8` aimed shots, `0x37fd4` beams, muzzle flash positions from the tables.
* **Damage** `0x40040` / `0x3fec8` (box hit, returns -1 when nothing is there) / `0x3fbfc` (smart bomb: every object takes damage). Destruction pays the object's score, effects, a wreck
  decal (`0x40954`), pickups (kinds 5 / 6 / 20 are hidden pickups: `+104 != 0`, not drawn, not counted as targets) and, for the last target of the level, the reward of `0xea4c` (`pickup::clear_reward`).
* **Drawing:** the original keeps a 512-px circular background buffer (`D.9394`); `0xc0c8` redraws the baked objects (`0x3fa24`) and decals (`0x40bf4`) when it wraps, `0x3f554` moves every object
  with the scroll, culls, draws the live ones on the screen (`0x2f88` + shadow `0x3358` = full-size shadow, hit flash `0x3540`) and burns static ones into the buffer (`0x3728`, shadow `0x3910`,
  translucent `0x381c`). The port has no buffer: `Ground::render_background` draws the same baked objects / unclipped decals from the state, then the live primitives (`Prim::Cast` is `0x3358`).
* **Bombs (button 2)** `0x26b58` control -> `0x201c0` launch -> `0x203a4` update -> `0x205ec` draw -> `0x2015c` clear; pool `0xcb74` (30 x 44 bytes), counter `D.157c`. A burst of `special_max`
  bombs, one every 3 ticks, next burst 25 ticks after the last and only after the button was released. A bomb flies straight up from the ship's nose (x does not follow the sight, only the height
  does) 4 px per tick and explodes when it reaches the sight's height (which scrolls down with the ground): effect 2, damage 1 to the ground objects in a 2-px grown box, a scorch decal
  (anim 44) when nothing was hit. **Bombs are the only thing that damages ground objects** (apart from the smart bomb); shots and beams do not.
* **Main-loop order** (`0xc6f0`, call sites at `0xd060..0xd298`): input, `0x26cf8` button 1, `0x26b58` button 2, **`0x3d0fc` ground**, `0x2dc20` enemies, `0x4127c` bosses, `0x22170` shots, `0x27434`
  beams, `0x38b0c` enemy shots, **`0x203a4` bombs**, `0x2b884` pickups, `0x25264` movement, `0x2cb40` drones, **`0x40b50` decal scroll**, `0x45a6c` audio, `0xc3e0` scroll step (activation + spawn
  trigger), **`0x3f554` ground draw, `0x205ec` bombs draw**, `0x39020` enemy shots, `0x2e118` enemies ... `0x2242c` shots, `0x275c8` beams, `0x257b4` ships. `game.rs` follows it.
* `0x2ca38` (called "ground objects that stop shots" earlier) is **not** part of this system; enemy shots still pass through everything.
* Mutation check of `ground.rs`: 52 single-literal mutations; 47 are caught by the committed vectors. The five that no vector (of 234 generated) catches: the float limits `y > 15.0 && y < 400.0` and
  `y > 0.0 && y < 480.0` (differ only for a y within one pixel of the limit), the +-60 chain-explosion box (`f2i(x) - 60`, differs only when an object sits exactly on its edge), the
  smoke decal offset `yoff = 5` for tower states 5 / 9 (rare state) and the 1-in-100 big-score pickup `pickup(3, 8, 1, 1, ..)` (needs two consecutive zero rolls; re-read in the disassembly at `0x3e418`: kind 3, variant 8, param 1, announce 1).

* **Bake frame (order of copy and ground pass):** the main loop copies the 512-line background buffer to the window (`0xC5B4`, a CopyBits of the 416x480 rect at `D.9394`)
  *before* the ground pass `0x3F554`. An object that becomes part of the background (`+49` set when `y > 0`, `+48 == 0`) is therefore drawn live on that frame and burnt
  into the buffer by `0x3728` / `0x3910` for the frames after it. The port draws the background from the state (`Ground::render_background`), so it has to leave the
  objects baked in the current `move_draw` call out for that one frame (`Ground::baked_now`); otherwise the sprite is drawn twice and the drop shadow (a darkening
  through DARK 128) is applied twice, which shows as a darker spot for one logic step each time a shadow-casting object scrolls in. Test:
  `game::tests::a_ground_object_is_drawn_exactly_once_per_step_also_when_it_becomes_background` (all 26 kinds).
## Effects and debris (ported: `src/effects.rs`, `src/debris.rs`; verified by `tests/effects_golden.rs` (24 vectors) and `tests/debris_golden.rs` (16 vectors))
Two pools make every explosion, and **both play the effect's own sound**: that is why a destroyed object was silent as long as they were missing.
* **Effect pool** `0x22618` init (+ direction table `0x224e8`), `0x22670` spawn, `0x23440` particle burst, `0x236b4` update + draw: 60 records of 180 bytes at `D.dd6c`, 50 spark directions at `D.dbdc`.
  15 kinds (sprite sets 1..17, 1 to 15 frames); kinds 6..12 are sparks that leave a trail and a shadow and loop until they leave the screen. The spawner plays `0x45bac(snd, 4, 0)` when
  delay == 0, the update plays it when the delay runs down to 1 (`snd != -1`). Pool and star digests, every random number and every blit are identical with the original.
* **Debris pool** `0x2942c` init (direction table `0x29470`: 62 random unit vectors x 6; reset `0x2957c`; colour grid `0x29668` = `GetCPixel` / `Color2Index` of PICT 960, 13 x 10: one ramp of ten
  colours per id 0..12), `0x29714` spawn (outwards), `0x298ec` spawn (inwards, 40 px away, used for the ship's appearance), `0x29b10` update / draw: 10 records of 976 bytes, 40 particles of 24 bytes,
  3 x 3 blocks plotted straight into the 704 x 608 buffer (the port emits `Prim::Pixels` requests in playfield coordinates), live 40 steps, colour moves one ramp step every 4 ticks. Same sound rule
  as the effect pool (`0x45bac(snd, 4, 0)`).
  Callers: ship destruction `0x24704` (id 9, scrolls 0), ship appearance `0x25750 / 70 / 90` (inwards, id 9, delays 16 / 22 / 24, sound 32 on the first), power-up pickup `0x2c248` (id 5), enemy death
  `0x2ed00` (id = record +124, scrolls 0), ground-object death `0x401d0..0x402dc` (scrolls 1; the big wreck explosion = id 2 four times with delays 8 / 14 / 22 / 30, sound 11 on the first three),
  bosses (`boss_destroyed`, see "Bosses").
* Mutation sweeps: effects, all non-equivalent mutants caught (the one survivor, `from = i + 1`, is equivalent); debris 33 mutants, 29 caught, the other four are equivalent: `end <= 129` / `start <= 120`
  can only see 9..129 / 0..120, and the ramp clamps `c > end` / `c < end` do nothing when `c == end`.

## Bosses (ported: `src/boss.rs`; verified by `tests/boss_golden.rs` against `tools/re/gen_boss_vectors.py`)
Special groups (group word 22, `+88`, != 0) are not ordinary enemies: `0x40dc4` registers them in a **boss pool** (`D.109c` = emulator `0x24c28`, 10 records x 228 bytes, counter `D.1abc`) and `0x40e7c` spawns them
into *part slots* of the normal enemy pool (flag enemy `+168` = 1, `+172` = boss index, `+169` = the part carries the boss HP).
* **Registration / free:** `0x2d7a8` frees the pool (`0x40d6c`) then registers every group with byte0 != 0 and `+88 != 0`; the copy is group `+44` -> rec `+4` (type), `+68` -> `+108` (HP index), `+76` -> `+152` (bonus count),
  `+80` -> `+148` (bonus kind), `+64` -> `+104` (interval), group index -> `+168`. More than 10 registrations return -1. `0xd9b0` (level setup) frees the pool *again* (leaves the data, clears byte0, count = 0): this
  is why a registered record is "inactive" until spawn sets byte0, which resolved the apparent register/spawn paradox.
* **Spawn `0x40e7c`:** needs 4 / 5 / 2 / 2 / 2 free enemy slots for types 1 / 2 / 3 / 4 / 5; types 1 and 3 have no init (spawn cancelled, record deactivated). HP = convert_b(`+108`) + level x 10, interval via convert_a. Plays sound 52 (priorities 0 and 40), then
  starts the type's looping alarm (`0x45e24`, sound id from rec `+193`; the handle is stored at `+196`); the sound-end callback `0x4123c` sets the restart flag (`+195`).
* **Types:** 2 (`0x433e0`, update `0x42344`): 58 x 58 body plus four orbiting satellites (5 parts), falls at 5, accelerates towards the middle; 4 (`0x43bcc` / `0x4274c`): body 61 x 124 plus a gun pod (2 parts); 5 (`0x44048` / `0x42f48`): 64 x 128 plus a cannon (2 parts).
  Start x is random 32..416-width, y = -128; the x acceleration points to the middle (threshold `x + half width > 208`).
* **Frame hooks:** update `0x4127c` runs after the enemy update, draw `0x417a4` after the enemy draw, damage `0x41c88` is reached from the enemy damage path (`Enemies::boss_hit`, bombs included); destruction fires part explosions, pickups and the bonus group.
* **Quirks kept on purpose:** type 4's target choice `0x2e7a8` is called with a stale `f31` (the port passes the emulator's value 0.0); in the destruction the part explosions use the sound byte (`+126`) of the *hit enemy slot* (register-reuse bug in the original; an out-of-range slot reads 0);
  the shadow `fnmsub` in the draw is fused (`-(0.66f64.mul_add(h, -h))`); pickups from bosses are deferred through `Env.pickup_events` like the group bonuses; sin / cos are Python `math` in the generator (the original MathLib may differ by an ulp, invisible in f32).
* **Verification:** 70+ random vectors (registrations including overflow, spawns, steering, shots, hits, deaths, culling, pickups, sounds) plus boundary vectors (steering threshold, exact kills, culling edges, odd sound / aux / particle bytes, timing edges, out-of-range boss indices), bit-for-bit including the digest of both pools; `ppcemu.py` gained fused `fnmsub` / `fnmadd`.
* **Mutation sweep:** 593 mutants of `boss.rs` (operators, integer and float literals): 27 do not compile, 532 are caught, and 34 survive; none of the survivors can change behaviour: loop / clamp bounds that equal the fixed part counts (5 / 2) or the maximum of 10 records, the handle word that the looping alarm overwrites, `62 / 2` vs `63 / 2` (integer division), the padding byte at record +116, the sound id checks that only ever see ids 12 / 57 / 58, a threshold clamp that only rewrites the value it already has (`a > 3.0`, `a > 5.0`, `new > hp` on the boss's own record), `tick > last + 32` (the equal case is taken by the match arm before it), the target-choice arguments of type 4 (the stale `f31`, see above), the `damage(e, 5)` branch for a part without the boss flag (a state that cannot occur: the death of such a part would index the group table with -1).
  Found while writing boundary vectors: when the body (member 0) of a *living* type-2 boss is gone, the original still writes the animation state of "slot -1" (it reads group-table memory before the pool); the body carries the boss's hit points, so this cannot happen in play and the port skips it.

## Drones and side missiles (ported: `src/drone.rs`, `ShotPool::spawn_drone`; verified by `tests/drone_golden.rs` against `tools/re/gen_drone_vectors.py`)
Everything below was read from the disassembly and then *executed* in the emulator: 44 sessions (`tests/vectors/drone_vectors.txt`, about 4 MB: 30 random, 9 with the ships placed on the drone edge-clamp and frame-turn thresholds (`DRONE_EDGE=1`), 5 with hand-placed missiles on the exact edges of the culling box, the step limits and trail states (`DRONE_BOUND=1`)) mixing spawn, remove, fire, update, draw, launch, missile update / draw,
pokes into the records and both ship structures; after every operation the drone pool, missile pool, player-shot pool, the three counters, both complete ships and the exact call log (hit tests, effects, sounds, blits) must be identical.
**Drones** - pool pointer `TOC 0xfc8` = data `0x149f0`, 4 x 44 bytes, counter `D.17a0`. Record: `+0` active, `+1` mode (1 = flying in, 2 = docked; any other value makes the update return at once), `+4` tick,
`+8` owner byte (ship number), `+9` side (1 = left), `+12/+16` x/y, `+20/+24` vx/vy, `+28` hit byte, `+29` muzzle-flash byte, `+32` hit tick, `+36` anim (21), `+40` frame (1..6).
* `0x2c760(number, which)` spawn: needs count < 4 and a free record; x = 200.0, y = 0, vy = 8.0, mode 1, anim 21, frame 1. `0x2c70c` clears only `+0` and `+29`. Session start (`0xc884`) clears the pool, so drones persist across levels.
* `0x2c87c(number)` remove: deactivate, count - 1, effect(int x+15, int y+17, id 1, 0, -1, 0, 0, vx, vy).
* `0x2c960(ship)` fire: every active mode-2 drone of that owner calls the shot spawner `0x21288(ship, 15+x, 17+y)` (always kind 3) and sets the muzzle byte when a shot was created. Shot: 5 x 16, vy -20, sprite 36, frame 1, 1 hit point, not piercing, x = -2 + (15 + droneX); fails above 59 shots.
* `0x2cb40(ship1, ship2)` update (only while count > 0): mode 2 follows the ship (x = side 1 ? shipX - 41 : 10 + shipX + width; y = shipY + 10); mode 1 steers toward that point at speed 12 using the MathLib sqrt (`0x6260c`, single precision) and docks when `y >= 10 + shipY`; then x += vx, y += vy, clamp x to 0 / 386;
  hit test `0x2ead4(x, y, 30+x, 35+y, 1, owner)`, on a positive result: hit byte, hit tick, sound (15, 2, 0).
* `0x2d1d8` draw: frame follows the owner's horizontal speed, muzzle flash `Anim(x+8, y-10, 14, 14, 49, 1)`, hit byte expires 3 ticks later, shadow `(x-24, y+44, 30, 35)`, anim, flash while hit.
**Side missiles** - pool pointer `TOC 0xf88` = data `0x1079c`, 16 x 172 bytes, counter `D.16ec`, cleared by `0x27780` (also on every level setup `0xd9b0`).
* `0x277e4(ship)` launch by missile level (`ship+332`): 1 -> 2, 2 -> 4, 3..5 -> 6 missiles, otherwise nothing; needs count <= 16 - n. Offset, speed and wait tables are in the code (copied verbatim).
* `0x27dfc` update: waiting missiles count down; otherwise the kick curve `D.6854` = [0,0,1,2,2,2,1,0,0,0,0,0,-2,-3,-5,-7,-9,-11,-13,-15,-17] is added to y, step 12 lights the flame, steps 14 and 16 advance the flame frame, step capped at 20, vx decays by 0.1 from step 20;
  six trail points per missile; culled outside the 416 x 480 field; hit test with 7 hit points: four staggered explosions (sound 39) and the missile is freed.
* `0x28260` draw: missile anim 45, flame (anim 46, translucent or dithered by the option), trail points (anim 47) when the effects are "fancy".
Main-loop order: missiles after the beams, before enemy shots; drones after ship movement; draw: missiles before the player shots, drones after pickups and before the ships.
Quirks kept: mode 0 / > 2 freeze the update; the remove effect uses the *current* velocity. `game.rs` wires both (`Env::drones`, `Env::missiles`, `DroneWorld for Env`, `launch_missiles`, `drone_add`, `drones_reset`, `drones_fire`).
Mutation check (sweep over every comparison, operator and numeric constant of `impl Drones` / `impl Missiles`, plus the 13 field values of `ShotPool::spawn_drone`): 367 mutants, 20 did not compile, 333 caught, 14 survivors that are all equivalent (`x < 0` vs `<=` and the `> 416` clamp at exactly the clamp value, the dead delay value written only when the wait flag is 0, the kick-table index clamp `0` / `20` bounds that no reachable step touches, `> 20` vs `>= 20` rewriting the same step, the vx-decay `<` / `>` tests that only differ on a negative zero); all 13 `spawn_drone` mutants caught. A negative missile step (impossible in play) reads before the kick table in the original and is clamped in the port.
Game-level test: `drones_dock_fire_and_missiles_fly`.

## HUD (ported: `src/hud.rs` logic, `src/hud_panel.rs` pixels, wired in `src/game.rs`; verified by `tests/hud_golden.rs` against `tools/re/gen_hud_vectors.py`)
The side panel is drawn with three surfaces: the *panel* buffer (`D.8200`, the 704 x 608 working buffer every sprite goes into; panel picture `PICT 10000` at buffer position (64, 480)), a *store* strip bitmap
(`D.81fc`, clean copies of the backgrounds, made by init `0x1e950`) and the *screen* buffer (`D.9340`: `+64/+66` = panel top/left in the buffer = 64/480, `+72/+74` = on the screen = 0/416). The blitters `0x307c` (Anim) and `0x3634`
(Flash) write straight into the panel buffer whatever the QuickDraw port is, so on the 640 x 480 picture the "panel" and "screen" are the same pixels (panel coordinates - 64): `HudPanel::paste_onto` just shows them.
Every update: `Restore` (store strip -> panel rectangle), sprites, `Present` (panel -> screen). The routines and their rectangles (screen coordinates, player 1 / player 2):
| routine | what | rectangle |
|---|---|---|
| `0x2633c(ship, force)` | 7 score digits (anim 84, frame digit + 1, 15 px apart, 12 x 28 each; a flash copy for a few ticks after a bonus), only when `+160` is set or `force` | 104 x 28 at (444, 95) / (444, 328) |
| `0x1ef5c(player, value, flag)` | wave-motion power bar: shrinks by 3 per call toward `value` (`ship+208`), grows at once, one 9 x 15 segment per 3 points above 20, `flag` = `ship+288` (overheating) | 108 x 15 at (443, 149) / (443, 382) |
| `0x1f51c` / `0x1f5c4` / `0x1f69c` | shield bar: target, one step per tick, draw `n` segments | 108 x 15 at (443, 197) / (443, 430) |
| `0x1f848(player, lives, flash)` | lives (small ships, 35 px apart; `flash` = 1 only for the extra life `0x24164`) | 104 x 28 at (445, 48) / (445, 281) |
| `0x1fe28` / `0x1ff94` | keep-weapons icon, armour level icon (frame = level, 1 draws nothing) | 31 x 20 at (488, 11) / (487, 244), and (522, 11) / (522, 244) |
| `0x1fc50(player)` | hit / absent flash: pen mode 32 (blend) black at 50 % over the player's panel (two rectangles) | player 1: 134 x 215 at (429, 19) + the 37 x 6 title bar; player 2 likewise at +233 |
Sequences: level set-up `0xd7b0` = panel picture, **init `0x1e950`**, ... ships `0x23de4` (weapon icons), ..., then for each ship `score(force 1)`, `lives(flash 0)`, `shield(0)`, and `flash` when the ship is not in the game; afterwards (`0xc8ec`, `0xd5e4`) `0x1f51c(set, ship.shield)` for
the players in the game. Every frame after the draw routines: for each player `score(0)` then `power(set, charge, heat flash)` (`0xd2e4..0xd380`). The ship code's own calls (`session_step`, pickups, weapon reset) were already hooks; the
lives hook now carries the flash argument (`World::hud_lives`: 1 for the extra life, 0 for the session loop, the draw step and the set-up) and the flash hook the player number (it was missing).
Verification: 24 random + 6 edge sessions (30 vectors: ships with noisy fields, random ticks / origins, every operation) compare both ship structures, the HUD variables `D.154c..D.157c`, the rectangle table and the complete
log of copy / blit / port / QuickDraw calls bit for bit; mutation sweep 436 mutants, 27 do not compile, 402 caught, 7 equivalent survivors (mutants of `Hud::new` and of a few score / shield lines whose effect is overwritten).
`0x23c04(ship 2, solo)` sets `active = 0` and **lives = 1** (otherwise `0x24140`: lives = 3), so the absent player's panel shows no spare ships (as in the original screenshot).
Game level: `Env::hud` is the HUD state, `Env::hud_calls` the log (`Game::take_hud_calls` returns it with the epoch; the epoch changes at every `0x1e950`, i.e. the host restarts the panel from the picture), test `hud_follows_the_game`.
The panel geometry agrees with `pictures/screenshots/level_5_screenshot.png` (score digits at x = 444, y = 95; bars at y = 149 / 197; player 2 dimmed in a one-player game).

## Sound audit (every `0x45bac` / `0x45d54` caller of the original against the port)
Checked function by function (call sites found by scanning the disassembly): ship `0x24164 0x243a0 0x24544 0x26cf8 0x26fd8 0x27080 0x271bc 0x265ec 0x268f4 0x257b4`, power-ups `0x2a040 0x2c134`,
bombs `0x201c0 0x203a4`, enemy damage `0x2ec14`, enemy shots `0x37fd4 0x38754`, ground `0x3dfb4 0x40040`, effects `0x22670 0x236b4`, debris `0x29714 0x298ec 0x29b10` are all present in the port with the same ids,
priorities and counts (the ones inside verified routines are checked by their golden vectors; the call logs include sounds). Not played yet because the code is not ported (the drone sound id 15 of `0x2cb40` is done):
the level flow `0xc3e0` (21) / `0xc6f0` (28, 30, 36, 24) / `0xe098` (71, 56, 29; the level end `0x208fc` is done), and the menus
(`0x1be60` and friends: 29 = click, 20 = hover, handled by `menu.rs`).

## Level end and next level (ported: `src/levelend.rs`, `PickupPool::convert_coins`, `Game::next_level`; verified by `tests/levelend_golden.rs` (40 vectors, 32 mutants, the only survivor is unreachable) and `tests/coins_golden.rs` (60 vectors))
* `0xc3e0` (scroll step) returns 0 / 1 / 2 (normal / last five rows / over). On the step the row counter reaches `height - 5` it plays **sound 21 (priority 5)**, sets the HUD mode `D.172c = 4`, runs
  the **silent smart bomb of player 1** (`0xdb54` = `0x26fd8(ship1, 0)`: `0x3fbfc`, `0x2e6a8`, `0x38a1c`, no sound) and starts the coin bonus `0x20764`. While the answer is 1 or 2 the bonus `0x208fc` runs
  after the HUD draw of every frame; when it returns 1 `D.13e8` is set: the level is complete.
* The coin bonus (`levelend.rs`): 45 ticks wait, panel opens (sounds 54, 21 delayed 5), 45 ticks, then one coin per 3 ticks and player is paid `min(level * 100, 1000)` points (sound 34 each;
  `ship +320` is the coin count, not reduced), then `0x2b73c` turns the coins still on screen (kinds 2 and 20) into effect 14 with sound 39, sound 54 (something paid) or 53, 60 ticks, done. Nobody in the game
  (`+0` clear or no lives left) means the tally never runs and returns 1 at once. The panel (`0x20be0`, number drawers `0x21090` / `0x21150`) is drawn with sprite sets 14 / 15 (player icons, only with two players),
  114 (ring, translucent), 135 / 137 (coin), 84 (digits, 12 x 28, 15 px apart), 85 / 86 (the "x" and "="), at y 298 (+ 70 for player 2).
* Level change (`0xc6f0` at `D.13e8`, then `0xd7b0`): sounds 28 (p5) and 36, `D.13f0++` (levels done), `D.13ec++` (level number), tick back to 0, scroll reset, every pool reset (player shots `0x21224`, pickups `0x29e78`,
  enemies, enemy shots `0x37f60`, bombs `0x2015c`, ground), effects re-initialised (`0x22618`), debris records freed (`0x2957c`), the ships re-initialised with `keep = level > 1 && levels done >= 1` (`0x23de4`: weapons kept,
  coins cleared, a life added that the entering ship uses again), the delayed-sound queue emptied (`0x45a38`). After level 28 (`D.13ec == fcb8()`) the ending `0xe098(6000)` shows picture 130, plays 71 and 56 x 3.
  The ending and the message overlay are ported (next section). **Not ported:** the level-name string (`0x284a8` reads STR# 4000 into a buffer that is never read or drawn: nothing to port), the shareware nag (`0x1e488`, the port is "registered").
* Found on the way: the port never set the level number (`D.13ec`): difficulty, the ships' starting weapon level (`0xd7a0`) and `level_bonus` (`0xd7a8` = `D.13f0`) were those of level 1 at every level. `play`
  now passes the level (`Game::new_at`). The smart-bomb pickup did not clear the enemy shots (the `0x38a1c` hook was missing in `Env::hud`); fixed.

## Music (ported: `src/madh.rs` loader, `src/madplay.rs` engine, `src/music.rs` game rules; verified by `tests/madh_parse.rs` and `tests/madplay_vectors.rs` against `tools/re/gen_music_vectors.py`)
The original links the stock PlayerPRO library (MADDriver, `MADH` modules) and plays through a Sound Manager double buffer. Preferences as `Music_Init` (0x444f8)
leaves them: 16 bit, 44.1 kHz, mode 3 ("deluxe"), driver mode 4, 732 frames per callback, no oversampling / interpolation / surround, repeat on (pref55 = 1 MADH music).
* **Format** (loader 0x540b4, MAD1 unpacker 0x551f0): see the header of `src/madh.rs` and `ASSET_FORMATS.md`. Header 16044 bytes = 556 + 64 instruments x 242; patterns with a 48-byte header
  (`rows`, tag `MAD1` = packed with a flag byte per command: bit0 ins, bit1 note, bit2 cmd, bit3 arg, bit4 vol, absent fields = ins 0, note 255, cmd 0, arg 0, vol 255);
  then the samples of every instrument (56-byte header + 8-bit data). The player reads instrument fields straight from the header block (out-of-range reads see the neighbouring instrument).
* **Driver start** (what `Music_Start` 0x4560c does): channel volumes 64 (0x449dc), rewind 0x54c0c (tick 128, row 0, pos 0, speed 6, tempo 125), 0x54a84 (stop, reset, read speed/tempo,
  fill both buffers with `playing` = 0 -- those two silent-state buffers are the first audio), then `playing` = 1. Stop = 0x54b5c + `playing` = 0; resume = 0x54a84 + `playing` = 1.
* **Play** (0x56410): tick length = (44100 * 125 / 50) / tempo frames; effects on ticks 1..speed-1; `ReadNote` (0x55788..) on a new row; at the last tick of the last pattern position the song wraps
  (repeat on) and the channels are reset. Effects 0-15 (arpeggio, slides, portamento, vibrato, tremolo, sample offset, volume slide, position jump, set volume, pattern break, extended, speed/tempo),
  the volume column, the volume envelope (sustain / loop points, fade-out) and the sample loops are all in `madplay.rs`.
* **Mixer** (0x55e48.., the 8-bit path): step = 0xda779000 / (period * 44100) (u32); fast path when `cur + ((frac + step * n) >> 8) + 1 < end`, otherwise the wrap path (`cur -= loopSize`, clamped to the
  sample base); volume tables ((x * g) << 2) as s16 with x = s < 128 ? s : s - 255; channel pan alternates 30 / 70 (L = 100 - pan, R = pan); output clipped to +-32767.
  Period lookup (0x55e48): note 255 / 254 or no sample give 4242; `(note + relNote)` split into octave / semitone, finetune table; a negative note+relNote reads memory outside the period table in
  the original (undefined), so the port clamps it and the test generators never produce it.
* **Sample pointer:** the mixer reads the channel's base pointer [4], which only a note that starts a sample sets (not tone portamento, effects 3 / 5); the channel's instrument / sample numbers [32] / [40] are updated by every note with an instrument. A tone-portamento row with an instrument of its own therefore keeps mixing the old sample's data (found by a synthetic-module vector; `Chan::src`).
* **Not ported (not reachable with the game's settings and assets):** the 16-bit sample path, the interpolation / oversampling / surround mixers.
* **Rig:** `tools/re/music_rig.py` runs the original driver creation (0x53418), loader, rewind, prime and the double-buffer callback 0x56d44 inside `ppcemu.py`; only the Memory Manager,
  BlockMove and Sound Manager calls are mocked. `gen_music_vectors.py` makes the vectors (`tests/vectors/music_vectors.txt`): the five game tracks for 1000 callbacks (about 16 s of music each, followed by a
  stop / resume / restart sequence), the complete songs of level_3 and special (3500 callbacks: a whole loop and the wrap, 55 / 64 s), and 22 random synthetic modules (all effects, envelopes, loops, jumps, speed changes), each stream as FNV-1a hashes of up to 16 buffers. The Rust engine matches all of them.
* **Game rules** (`Game_Run` 0xc6f0..0xd5e0 and the music API 0x45488 load / 0x4560c start / 0x456d0 stop / 0x45594 unload, loader 0x44bb0 with track 1 = interface, 2 / 3 / 4 = level 1 / 2 / 3):
  the title screen plays the interface track (the special track on December 24 / 25 -- date test 0x1e260 / 0x1e2a0; the port reads the date in UTC, the original the local date); a new game stops it and
  loads level group 1; after every third finished level the group alternates 1 -> 2 -> 1; finishing the last level plays group 3 (ending); game over stops the music. The pause key does not touch the
  music in the original (the port does pause it).
* Unverified (cannot be tested here): the real audio device path (rodio buffering / latency), the level balance of music against the sound effects (both are mixed by the output at full scale).

## Screenshot verification summary
Levels 5, 8, 15, 17 each align to their level at scroll 2755 / 2019 / 3738 / 3644. Predicted ground objects (strip + anchor + frame rule) match
11 of 34 visible objects at 100 % (kinds 2, 4, 8, 16, 18, 21); Rust test `ground_objects_match_original_screenshots` encodes six of them.

## Next targets
The `ASGM`/`_ASG` consumers near `0x2d478…0x2d748`, the `TRAN`/`DARK` blend users (`0x3d04`, `0x29cc`), the dialogs / ending (the sound effects and the MADH music are done, see "Sound engine" and "Music"); title screen: key handler `0x1be60`, option-key level chooser `0xfcb8` / `0x1b254`, the dialogs `0x1cc8c..0x1ce24`, menu commands `0x1d478`, idle text cycle.

## Drawn-in shadow layer (checkerboard)
The shadow layer (sprite ids 15000 + index - 1, drawn as group 3 / map pass 3, `0x3728`) consists of pure 1-pixel checkerboards in all 28 levels: every opaque pixel has an odd `x + y` inside the (32-px aligned) sprite. The original draws the three map passes into its 512-line background buffer (`0xc170`, called from the row step `0xc3e0`, which adds the scroll speed `D.13c4` (always 0 or 1: written only at `0xc05c`, `0xc0ac`, `0xc46c`, `0xc478`) to the sub-row counter `D.13c0` and draws a new row at 32) and copies it to the window shifted by the scroll (`0xc0c8` moves the source rectangle `D.9394` up by `D.13c4`),
so the pattern moves with the ground like everything else; the port does the same. (An attempt to keep the checkerboard still on the screen made the shadows shimmer and was reverted.) Level 11 has a shadow cell with index 54 (the set has 53): nothing is drawn there.

## Title key handler, idle cycle and the attract-mode demo (ported: `src/demo.rs`, the `Rng` of `src/game.rs`, `play.rs`; replay check `tests/demo_replay.rs`)
* **Key handler `0x1be60`** (jump table `D.5be8`, key = character - 3): Return / Enter / `1` / `N` / `n` -> one player (sound 29, button 1 hilited by `0x1c1ec`, `0x1cb80(0)`), `2` -> two players, `R` -> Register (unregistered copies only), `P` -> Preferences,
  `Q` -> Quit, `C` -> Controls, `H` / `S` -> High Scores (no click sound), `D` -> the demo (`0x1cdd0`, no click sound), `X` -> `0x1cef0` (sound 65, then `0x1e808`: the title background and page 400 (the Donne quotation) wiped in like a Credits page, waiting for a click or key for ever: sound 29, and N starts a game; ported in `Credits::single`), `i` -> a screenshot (`0x9e48`, Save dialog). All other keys do nothing.
* **Idle loop** (`0x1a500`): `r26` = `TickCount` of the last click / key / hover change (`ResetIdle` of `menu.rs`); after 600 ticks with the title window in front the loop posts a key event, `D` the first time and then alternating `S` / `D` (`r24` toggles),
  and restarts `r26`. Every 160 ticks (`D.1540`) the text line below the logo changes (`0x1dcc8`, stage `D.1544` 0..2, redraw of the title after the third; strings at `0x5fd4..0x6580`): the copyright line, "THIS COPY IS NOT REGISTERED." / "Registered To: <name> [n Copies]",
  www.MarsRising.com, shareware reminders, season greetings by date and a list of quotations. Not ported: it is shareware / registration text (the port is a registered copy without a name).
* **The demo**: `0x1cdd0` -> `0x1b508(D.13d4 + 1, demo = 1, 1, 0, 0)` -> the session loop `0xc6f0` with `r28 = 1` (`D.13e3 = 1`). `0xc7b0` loads `'FILM'` resource `D.13d4 + 1` with `0xee60` (12068 bytes to `D.939c`), `D.13d4` counts on and wraps at the number of films (`D.13d0`, counted by `0xedc0`:
  consecutive ids from 1); level, player count and `randSeed` come from the recording (`qd + 76` := `+0`). Layout of the film: `+0` seed, `+4` level, `+8` players (byte), `+44` the level number at the end, `+52` last recorded frame, `+56` / `+64` read positions (0 in the file), `+68` 6000 bytes ship 1,
  `+6068` 6000 bytes ship 2: bit 0 left, 1 right, 2 up, 3 down, 4 button 1, 5 button 2 (the recorder `0xf728` ORs them in, the reader `0xf640` splits them again; the ship input routine `0x250a0` only polls / records while the ship is **playing**, state 3).
  The loop ends at `pos > end` (`0xf5d4`), or when a mouse button / key goes down (`0xd648`, `D.13f4 := 0`). In a demo every sound entry point (`0x45bac`, `0x45d54`, `0x45ca4`, `0x45e24`) and the music start (`0x449dc`) return at once (`0xf5b8`), the level and game-over texts (`0x28510`) are skipped, the music is not touched
  (the interface music keeps playing), no high scores; the end plays sound 36 and redraws the title.
* **Copyright box** (`0x1aa68` end, `0x1dcc8`): picture 9400 (326 x 21) is drawn into the titles' `'Rect'` 100 (23, 383); the inner `'Rect'` 2 (25, 384 - 347, 403) is painted with `PenMode(32)` (blend) in black with `OpColor` 50000, then the line is written centred, baseline 7 above the bottom, Geneva 9, white (yellow for the greeting lines). Stage 0 of the text cycle is the copyright notice (`string copyright`, extracted from `0x5fda`). **Port:** the box is darkened to 1 - 50000/65535 of each pixel (the reading of the blend mode is an assumption) and the line is written with the port's 5x7 font (Geneva is not available).
  The icon 129 (`'IMAG'`, below the Quit button) is only drawn when `0x1e260` (the registration / nag state) says so: not ported.
* **High Scores -> title**: both paths (`0x1cd5c` and the end of a game) redraw the title (`0x1aa68`) and show it with the band wipe `0x1bafc(12)`; ported (the same `Wipe` as the Credits).
* **`Random()`**: the original's only random source is the Toolbox `Random` (`0xa0d4` is its only caller): `randSeed := randSeed * 16807 mod (2^31 - 1)`, result = the low 16 bits (0x8000 -> 0), scaled by `0xa0d4` to `lo..hi`. The port's old xorshift was replaced by this generator.
* **Verification** (`tests/demo_replay.rs`): the three recordings are replayed on the real levels with the stored seed. All three run to the end of the recording without a game over (scores 10-12 thousand, extra lives collected); with the seed off by one every run ends in a game over well before that.
  So the random number stream (algorithm, call order, count) and the frame logic stay in step with the original over about 4000 frames. This is a consistency check, not a bit-for-bit comparison: the ship is hit a few times late in films 1 and 3, which may or may not be what the original shows.

## In-game messages and the ending (`src/overlay.rs`; read from `0x28518`, `0x28508`, `0x28510`, `0x284a8`, `0xe098`)
* `0x28518` is called every frame from `0xd384` (after the ships' score / charge bar) and draws by the state byte `D.172c`, set by `0x28510(n)` (read by `0x28508`); `D.1728` = 206 is its y (set by the level reset `0x284a8`, which also clears the state).
  State 1 "Sector N" (highlighted font, y 206) + "Get Ready!" (y 246): the level start (`0xdb04`). 2 "Game Paused" + "Press CAPS LOCK to Resume": the pause (`0xcf78`; the old state is kept in a stack variable and put back by `0xdfac`).
  3 the GAME OVER sprites (animation 34, frames 1 / 2, 64 x 33 at x 153 / 217, y 206; `0x3170` with translucency, `0x2f88` without): the last ship destroyed (`0xcc0c`, `0xcc38`, `0xceb0`, with sound 28) **and the level start of a demo** (`0xdaf8`: the demo shows GAME OVER the whole time, nothing clears it). 4 "Sector N Cleared" (y 220): five rows before the end, right after sound 21 (`0xc568`).
  The state is cleared (`0x28510(0)`) when a ship enters play (`0xcafc`, `0xcdac`) unless `D.13e3 == 1` (demo). Texts: `0x29054(str, -2, y, hilite, 0)`; the strings are at `0x6901` ("Sector "), `0x6a01`, `0x6a0c`, `0x6a18`, `0x6a32` (" Cleared"); N = `D.13ec` through `NumToString`.
* Found: the port passed `mode: 1` (the display-mode byte) to `Ship::session_step` always, so the clear after a respawn never ran. It is now 1 only in a demo. The port's pause key is P, so its pause text says "Press P to Resume" (the original's key is Caps Lock).
* The ending (`0xe098(6000)`, after the last level, `D.13ec == fcb8()`): window painted black, 'IMAG' 130 drawn at the top left (640 x 480), sound 71 (priority 4); it waits 3200 ticks (1/60 s) or a mouse click / key press (events 1 and 3; the queue is flushed first); a click plays sound 29; then sound 56 three times (delays 0, 1, 2) and the end of game flow (`0xdb48`, the scores) follows.

## Preferences (dialog 0x6810)
* **The block**: 10240 bytes at `*D.932c` (the preference file's first part; the high score table follows it in the original's file). Byte preferences `n = 1..100` at `n + 1` (`0xae6c` get / `0xa9ac` set), short preferences at `102 + 2 * (n - 1)` (`0xaed4` / `0xaa20`), key sets at `702 + 36 * (set - 1)` (12 byte name + 12 key codes, `0xab10`). `0x7868` copies the block to a backup (`*D.130c`), `0x7834` copies it back (Cancel / Revert).
* **Meaning**: bytes 55 resize screen, 56 line skipping (draws the playfield with every other line, `0x270` / `0xc5b4` / `0xdb80` / `0x19a08`), 57 interface music (`0x456cc(2)` is only called when it is set: title, Credits, High Scores, end of game), 58 use the system sound volume, 59 escape key delay, 60 translucency (`0x29ac` -> `D.12be`), 61 extra detail (`0xe35c` -> `D.13c8`, the `fancy()` getter `0xe364`), 62 realtime shadows (`0x299c` -> `D.12bc`), 63 surround, 64 music filtering. Shorts: 51 volume 0..8 (`0x45f40`: `v * 32`, else 128; `0x51710` = `SetDefaultOutputVolume`, `0x45f6c` applies it, or reads the system volume when byte 58 is set), 55 music type (1 MOD track, 2 CD, 3 none; menu 1002), 56 quality (menu 1003: 7 entries with a separator, default 7), 57 mode (menu 1004: mono / stereo / deluxe, default 3), 53 / 54 the key set of player 1 / 2.
* **Escape key delay** (`0xd3fc`, once per session loop iteration): the quit key (`GetKeys`, key 53 = Esc) ends the session at once without the option; with it a counter (`D.13dc`) counts the iterations the key is held (reset when released) and the session ends when it passes 30.
* **Factory settings** `0xb064(first_run)`: first run clears bytes 51..54 and 59 first; then `0xb0d0` (byte 58 = 1, 63 = 0, 64 = 0; shorts 51 = 4, 52 = 4, 56 = 7, 57 = 3, 55 = 1), `0xb14c` (shorts 53 = 54 = 1; key set 1 named from the first 12 characters of "1 Player Default" with the codes 123 124 126 125 55 58 86 88 91 87 121 119) and `0xb1e4` (bytes 56 = 0, 55 = 1, 57 = 1, 59 = 0, 60 = 1, 61 = 1, 62 = 1).
* **The dialog** (`DLOG` 190, items 1 Save, 2 Cancel, 3 Factory Settings, 4 Revert, 11 the Section pop-up of menu 20000; item list 191 appended for page 1 = items 18..23, 192 for page 2 = 18..29 by `0x752c` / `0x761c`): opening saves the backup, plays sound (30, 3, 0) and fills page 1 (`0x789c`). Every item hit changes its preference at once (check boxes toggle: `byte = (byte == 0)`; the pop-ups store `value` / `value - 1` for the volume) and applies the side effect (`0x45f6c` after the volume / music type / quality / mode, the three display switches right after their check boxes). Items of page 2: 19 volume pop-up (sound (30, 3, 0) after a change), 20 system volume (enables / disables item 19), 22 music type (disables 24, 26, 27, 28 unless it is 1: `0x7984`), 24 quality, 26 mode, 27 surround, 28 filtering, 29 interface music (disabled without the MOD player, `0x45840`); the CD entry of the music type menu is enabled only when `0x45838` says a CD can be played. Save and Cancel both leave through `0x6f00`: Cancel first restores the backup; then the file is written (`0xa4d4`), the volume (`0x45f6c`) and the music (`0x45848`) applied and the key set menu rebuilt (`0x1d180`).
* Verified: `tools/re/gen_prefs_vectors.py` runs `0x6810`, `0x789c`, `0x7a38`, `0x7984`, `0x7660`, `0xb064` and the accessors natively (ModalDialog scripted, the control calls, sounds and side effects logged) -> `tests/vectors/prefs_vectors.txt`; `tests/prefs_golden.rs` compares the block, the key record, the display switches and every operation in order (197 vectors); a 25 mutant sweep of `src/prefs.rs` was killed completely. The hover help is ported: the filter proc `0x7d24` takes the first dialog item under the mouse (`0x7c6c`; none = the text stays), and shows `GetIndString(135, item)` for items up to 17 (buttons, the section menu = item 11) or `GetIndString(131 + page `D.1314`, item - 17)` (STR# 132 general, 133 audio) in item 10; the strings are written by the extractor as `help` lines of `ui/dialog_text.txt`, the port shows them in a box under the page frame (greyed rows too). Not ported: the music quality / mode / surround / filtering effects, line skipping, screen resizing (the panel greys them).

## Level chooser (0x1b254, filter 0x7e04)
* Opened by `0x1cb80` (1 Player) / `0x1cbe8` (2 Players) when the Option key is held at the click (key 58): `0xfcb8` counts the `DIME` resources (id 1, 2, ... until one is missing = the number of levels), `0x1b254(count)` returns the level (0 = nothing) and a positive result starts the game with `0x1b508(level, 0, players, 0, 0)` (above level 1 = no high scores). Without the Option key the game starts at level 1.
* **Dialog** `DLOG` 160 "Level Select" (350 x 104; items 1 OK, 2 Cancel, 3 "Start at which level?\r(High Scores will be disabled)", 4 edit field `^0`, 5 icon 997): opens with "1" in the field, all selected (`0x488a8`), Return = default item 1 and Esc = cancel item 2 (`0x486cc` / `0x48778`), sound (34, 4, 0). OK: `StringToNum`; below 1 the field is set to "0", a 30 tick delay (`0x967c`) and the result is 0; otherwise the number is clamped to the count, shown for 30 ticks and returned. Cancel returns 0 at once.
* **Key filter** `0x7e04` (keyDown 3 / autoKey 5 only): with Command held: upper-cased `.` -> item 2 (the button flashes, `0x4a240`), any other key is swallowed silently; Return (13) / Enter (3) -> item 1 (no flash); Esc -> item 2 with the flash; digits and Backspace pass to the field; everything else: `SysBeep`, swallowed.
* Verified: `tools/re/gen_levelsel_vectors.py` -> `tests/vectors/levelsel_vectors.txt` (480 vectors: the dialog with scripted hits and typed text, and the filter over event types, characters and modifiers), `tests/levelsel_golden.rs`. The port's field takes at most nine digits (the original's `StringToNum` overflow is not modelled). The panel layout is the port's own.
