# Stage art references

The stage backdrops, palettes and obstacle meshes are traced from the stage splash paintings (`havocmd/assets/splashes`) with [Ludo AI](https://ludo.ai)'s `ludo` CLI. `ludo-manifest.json` records every Ludo job behind these files: job IDs, inputs and prompts.

## Backdrops and palettes (`tNN_flat.png`)

For each terrestrial theme (00 lowland … 09 meteor):

1. `ludo image edit` removes the title lettering and the ship from the splash, producing a clean plate.
2. A second `ludo image edit` redraws that plate as a flat-shaded, few-color C64-style backdrop. That redraw is the `tNN_flat.png` kept here.
3. `python3 tools/derive-stage-art.py --apply` traces each flat reference into the existing SCENERY tables in `src/scenery.rs`:
   - the five-triangle horizon silhouette
   - three cloud centers and their shared outline
   - sky, horizon, cloud and terrain colors

   The tool enforces the limits the backdrop tests check:
   - the silhouette spans the whole horizon
   - every cloud corner stays on screen, between rows 24 and 89
   - the cave keeps its engine-specific mouth geometry

Colors are art-directed per theme in the tool's `ART_DIRECTION` table, with the derived rankings printed alongside as candidates. Table sizes do not change, so only data moves in the C64 image.

## Orbital backdrops (`t10_flat.png` … `t15_flat.png`)

The orbital themes (10 asteroid belt … 15 jump gate) draw their scenery in code, in `src/scenery/space.rs`, over a fixed black sky. They get the same clean plate and flat redraw, with a prompt asking for a black, starry sky and each painting's main objects as large flat facets.

`python3 tools/derive-stage-art.py --orbital` ranks each flat reference's C64 colors. From those candidates, the space scenery takes these hand-picked colors and placements:
- asteroid belt: a larger planet in the upper left with green land, brown debris
- shattered moon: a larger moon on the right
- ship graveyard: grey wrecks with rust undersides and light blue end caps
- orbital shipyard: white gantries with cyan light strips
- space carrier: light blue parked hulls with red undersides
- jump gate: light blue and white ring segments with cyan conduits

Only constants and table values change, so the C64 code size does not.

## Obstacles (`props/`)

The ROCK and BUILDING obstacle meshes are generated for the themes that use them:
- rocks: lowland, canyon, crystal cave, ash, storm, meteor
- buildings: fortress, coast, foundry

`props/tNN_<kind>.png` is the concept image: a `ludo image edit` that isolates one object from the clean plate.

1. `ludo model create --model prop --target-num-faces 1000 --texture-type simple --texture-size 1024` turns the concept into a textured GLB.
2. `python3 tools/stage-props.py --write --preview assets/stage-art/props/preview.png` builds a progressive convex hull of 10 points for rocks and 12 for buildings, at most 16 or 20 faces. It then:
   - fits the hull exactly into the box of the obstacle it replaces, so collision, clearance and shadow extents are unchanged
   - colors each face with one of three art-directed colors, ranked by the texture it covers under the light
   - writes `src/host/stage_props.rs`, which `src/host.rs` includes when baking the model banks

The GLBs are not committed. To regenerate them, save the `ludo model create` results as `tools/asset-pipeline/ludo-output/models/<name>_model-1.glb` (that folder is ignored by Git).
