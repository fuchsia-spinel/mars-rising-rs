# Extracted asset formats

Status tags: **[V]** verified against data (exact decode / cross-checks), **[U]** structure kept, meaning unverified.

| Path | Contents |
|---|---|
| `palettes/*.json` | `clut` palettes: `{id,name,colors:[[r,g,b]..256]}`. `801_system_colours` is the Mac system palette used by every tile and sprite **[V]** (all 256 colours distinct). `140/141 Mort - White/Red` are alternate ramps **[U]** |
| `tiles/tiles.png` | 255 tiles, 32×32, **8-bit indexed PNG**, 16 per row **[V]**; `tiles_mirrored.png` = every tile flipped horizontally (the original `TILF`) **[V]**; `tiles.json` |
| `sprites/NNNNN.png` | 1,248 RGBA sprites (ids 10000…31987), sizes 8×8–64×64. Decoded exactly from the RLE-style `btSP` format **[V]**. Transparency is in alpha; colours map back to palette indices 1:1 |
| `sprites/index.json` | per-sprite `width,height,aux` — `aux` is an unidentified 16-bit header field **[U]** |
| `sprites/sets.json` | `SpIc` sprite-id ranges `(first_id,count)`; set 1002 = ids 10000–10253, 1003 = 15000–15052, 1001 = tile sheet, 1000 = 159 animation strips **[V-ish]** |
| `levels/level_NN.json` | 28 levels, 13 tiles wide × 150 tall (level 28: 300) **[V]**. Row-major layers; **row 0 is the bottom of the level** and the game draws bottom-up **[V]**. `base_tile` = 0-based index into tiles.png (the original's ids are 1-based) **[V]**; `base_flip`: 1 = flip vertically, 2 = take tile from `tiles_mirrored.png`, 3 = both **[V]**; `fringe` (255 = none) → sprite 10000+v−1 drawn over the base **[V]**; `shadow` (255 = none) → sprite 15000+v−1 drawn after fringe **[V]**; `ground_marker` → index into `ground_sprites` (x = pixel column, y = map row **[V]**; `kind` 0–24, `variant` 1–8, `prev_link`/`next_link` pair records **[V]**, `casts_shadow` (record byte 1) = draws a shadow: silhouette at (−6,+8) with underlying pixels remapped through `tables/dark_128_system_shadow.bin` **[V: 9/9 shadow casters, 100 % on 5 exact cases; 1 non-caster confirmed]**; `anim_index` = 1-based entry of animation table set 1000, `sprite_id`/`initial_frame`/`draw_offset` = static sprite, frame and top-left offset to add to the cell origin **[V pixel-exact for kinds 2, 4 (anchor only), 8, 16, 18, 21; decoded-from-code but not seen on screen for the rest; kind 1 never matched]**; `param`, `i32_c`, `flags` **[U]**); `spawn_trigger_by_row[row]` = group index to spawn when that row is reached, `groups` = 124-byte records as 31 u32: `[0]` active byte, `[11]` enemy type (see `src/enemies.rs`), `[12]` member count ≤10 **[V from the spawn code]**, `[21]` is a stale editor pointer (ignore), `[22]` special-spawn flag, `[15,16,17,19,20]` formation parameters **[U]** |
| `sounds/NNNNN_name.wav` + `index.json` | 74 sounds, 8-bit mono PCM, 10–22 kHz, loop points & base note in the index **[V]**. `exact_rate` is the resource header's 16.16 rate / 65536; the audio engine uses `round(exact_rate * 65536)` (`assets::load_samples`, resources 10000..10071 = effects 0..71; the engine ignores loop points) |
| `pictures/{titles,app,screenshots}/` | all 45 PICTs + IMAG credits images as PNG (title background, buttons + hilited versions, logo, score bar, portraits, 4 original gameplay screenshots) **[V]** |
| `layout/rects.json` | UI rectangles (left,top,right,bottom) in 640×480 space; sizes match their PICTs **[V]** |
| `tables/*.bin` | `tran_*`: 256×256 byte blend tables, `index = below_pixel*256 + sprite_pixel` **[V: blitter 0x4078 run in the emulator]**, the game loads `TRAN 1000`; `dark_*`: 256-byte palette remaps (stored in the original as sign-extended longs) **[V]** |
| `ui/` | strings (`STR#`,`STR`), menus, dialogs/alerts/controls as JSON, dialog text |
| `cursors/`, `icons/` | CURS cursors (+hotspots), `cicn` colour icons, `icl8` icons as PNG. Colour cursor `crsr` not exported |
| `misc/` | `film_*.bin` (3×12,068 B attract-mode recordings: seed, level, players, 6000 input bytes per ship; see RE_NOTES "attract-mode demo"), `spin.json`, `tset.json`, `setl_*.bin`, default high scores, versions, engine credit |
| `ui/dialog_text.txt` | the texts of the Level Select dialog (160) and the Preferences dialog (190..192) and the Preferences pop-up entries: `ditl <id> <item> <type> <l> <t> <r> <b> <hex>` (190 / 191 / 192) `menu <id> <item> <hex>` (1001..1004, 20000) and `help <STR# id> <index> <hex>` (the Preferences hover help: STR# 132, 133, 135), MacRoman |
| `misc/default_scores.bin` | the factory high score table: the `SCOR` 128 resource, 332 bytes (see RE_NOTES "High scores") |
| `layout/title_text.txt` | Credits slideshow data of the original's code: glyph rectangles of the text font (pictures 9001 / 9002) and the strings / positions of the 35 credit pages, one record per line (`font c top left bottom right`, `page n`, `text x y hilite mono hex`, `image id top left bottom right`, `string <name> <hex>` for the texts of the high score name entry; made by running the original in tools/re/ppcemu.py) |
| `music/*.madh` | 5 PlayerPRO **MADH** modules, copied unchanged (played by `src/madplay.rs`, see RE_NOTES "Music") |
| `docs/` | READMEs, FAQs, licence, release notes (UTF-8) |
| `manifest.json` | every file written + warnings |

Not exported on purpose (Mac-specific, not game assets): the installer app, InputSprocket/Display libraries, the MOD/XM
importer plug-ins, `WDEF` window code, `BNDL/FREF/SIZE/cfrg` Finder/loader resources, the game's PPC executable.

## Verified against the original game
Drawing level 5 with these rules (base + fringe + shadow, no units/objects) reproduces **90.3 % of the pixels** (91.7 % once the verified ground objects — two kind-8 pads and the kind-4 turret — are added) of the
original gameplay screenshot at scroll offset 2755 (base alone: 64 %, +fringe 85 %, +shadow 90 %); the remainder is the
player, enemies, bullets and ground objects. Rules came from the original PPC code (see `RE_NOTES.md`).
All four screenshots (levels 5, 8, 15, 17) align to their levels with sharp peaks (4–20× the median score; base-layer pixel match 64–86 %), so the map rules hold for every screenshot. Of 34 ground objects visible in them, 11 match their predicted strip + anchor at 100 % (kinds 2, 4, 8, 16, 18, 21); kind-1 objects (13 visible) never match, so they are presumably hidden/inactive in those frames or the kind-1 rule differs.

## Known open questions
1. Ground objects: kind-1 behaviour (13 visible, 0 matched), kinds 5/6/7 (not matched), runtime state (turret aiming — kind 4 has 24 rotation frames, active/destroyed flags, drop shadows drawn at (−6,+8)), and what `param`/`i32_c`/`flags` mean; enemy spawn `groups`. The sprite `aux` header field is *not* a draw offset.
2. Colour-cursor `crsr`, `FILM` data, `tset`/`setl`.
3. Where the other `dark_*` remaps (Pale Blue / Bright Tint) are applied. `dark_128` = System Shadow (object shadows; the ship/pickup shadow blitter also squashes the sprite to 2/3 x 3/4), `dark_129` = Orange (hit flash) and `tran_1000` are verified by the blitters.
4. The `aux` field of sprites; the two screen-space rules (scroll speed, player start position).
