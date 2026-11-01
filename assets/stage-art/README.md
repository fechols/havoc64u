# Stage art references

The stage backdrops, palettes, obstacle meshes and orbital scenery shapes are traced from the stage splash paintings (`havocmd/assets/splashes`) with [Ludo AI](https://ludo.ai)'s `ludo` CLI. Every one of the 18 stages has its own theme and painting. `ludo-manifest.json` records every Ludo job behind these files: job IDs, inputs and prompts.

## Backdrops and palettes (`tNN_flat.png`)

For each terrestrial theme (00 lowland … 09 meteor, 10 gate canyon for Canyon Gate, 11 assault coast for Carrier Assault):

1. `ludo image edit` removes the title lettering and the ship from the splash, producing a clean plate.
2. A second `ludo image edit` redraws that plate as a flat-shaded, few-color C64-style backdrop. That redraw is the `tNN_flat.png` kept here. For the two boss paintings (t10, t11) the prompt also leaves out the boss warship, which would otherwise fill the sky band the tracer reads.
3. `python3 tools/derive-stage-art.py --apply` traces each flat reference into the existing SCENERY tables in `src/scenery.rs`:
   - the five-triangle horizon silhouette
   - three cloud centers and their shared outline
   - sky, horizon, cloud and terrain colors

   The tool enforces the limits the backdrop tests check:
   - the silhouette spans the whole horizon
   - every cloud corner stays on screen, between rows 24 and 89
   - the cave keeps its engine-specific mouth geometry

Colors are art-directed per theme in the tool's `ART_DIRECTION` table, with the derived rankings printed alongside as candidates. Each theme table has one row per terrestrial theme (`TERRESTRIAL_THEMES`, 12).

Canyon Gate and Carrier Assault keep their partners' gameplay geometry, the canyon walls and the wide water lane (`scenery.rs` `edge` and `lane`), but each has its own palette, sky, horizon, clouds and terrain profile, so the crossings from Iron Canyon and Carrier Coast visibly change scenery.

## Orbital backdrops (`t12_flat.png` … `t17_flat.png`)

The orbital themes (12 asteroid belt … 17 jump gate, numbered like their stages) draw their scenery in code, in `src/scenery/space.rs`, over a fixed black sky. They get the same clean plate and flat redraw, with a prompt asking for a black, starry sky and each painting's main objects as large flat facets.

`python3 tools/derive-stage-art.py --orbital` ranks each flat reference's C64 colors. From those candidates, the space scenery takes these hand-picked colors and placements:
- asteroid belt: a larger planet in the upper left with green land, brown debris
- shattered moon: a larger moon on the right
- ship graveyard: grey wrecks with rust undersides and light blue end caps
- orbital shipyard: white gantries with cyan light strips
- space carrier: light blue parked hulls with red undersides
- jump gate: light blue and white ring segments with cyan conduits

Only constants and table values change, so the C64 code size does not. The planet's land masses and the moon's craters and fracture are placed by hand from `t12_flat.png` and `t13_flat.png`.

## Orbital shapes (`props/t1N_*.png`)

`python3 tools/derive-space-art.py --write` fits the orbital scenery and hazards to Ludo models of objects from each painting (concepts in `props/`, models made like the obstacles below):
- `tNN_asteroid`: each stage's tumbling ROCK hazard, written to `src/host/space_art.rs`. The six axis extremes stay exactly at the pinned collision, fragment and shadow bounds; their other coordinates and the relief of the six raised facets follow the model.
- `t12_asteroid`: the scenery rocks drifting past the asteroid belt and the moon (`ROCK_SHAPE` in `src/scenery/space.rs`).
- `t14_wreck` and `t16_hull`: the graveyard wrecks (`GRAVEYARD_HULL_POINTS`) and the parked carrier hulls (`HULL_POINTS`), fitted inside the original hull's box with mirrored sides and the keel kept as the prow.

Every point moves to the model's extreme point in the direction of the original vertex, then the shape blends back toward the original only as far as needed for every facet to keep facing outward. Vertex counts, face lists and per-frame face budgets do not change.

## Obstacles (`props/`)

The ROCK and BUILDING obstacle meshes are generated for the themes that use them:
- rocks: lowland, canyon, crystal cave, ash, storm, meteor, gate canyon
- buildings: fortress, coast, foundry, citadel, assault coast

The three terrestrial boss stages (Canyon Gate, Citadel Core, Carrier Assault) place three of their props on the approach, before the boss arrives; the encounter itself stays clear. A camera-clearance test checks that every obstacle's box passes outside the camera's near-plane rectangle.

`props/tNN_<kind>.png` is the concept image: a `ludo image edit` that isolates one object from the clean plate.

1. `ludo model create --model prop --target-num-faces 1000 --texture-type simple --texture-size 1024` turns the concept into a textured GLB.
2. `python3 tools/stage-props.py --write --preview assets/stage-art/props/preview.png` builds a progressive convex hull of 10 points for rocks and 12 for buildings, at most 16 or 20 faces. It then:
   - fits the hull exactly into the box of the obstacle it replaces, so collision, clearance and shadow extents are unchanged
   - colors each face with one of three art-directed colors, ranked by the texture it covers under the light
   - writes `src/host/stage_props.rs`, which `src/host.rs` includes when baking the model banks

The GLBs are not committed. To regenerate them, save the `ludo model create` results as `tools/asset-pipeline/ludo-output/models/<name>_model-1.glb` (that folder is ignored by Git).
