# gbsp - a Source engine map compiler in Rust

Compiles Hammer `.vmf` maps into Source engine BSP V20
with three tools that mirror Valve's pipeline:

| binary | replaces | crate | does |
|--------|----------|-------|------|
| `gbsp` | vbsp | `crates/gold-bsp` | VMF -> BSP: brushes, BSP tree, portals, faces, detail, displacements, water, areaportals, overlays and water transition overlays, occluders, cubemap patches, static props, detail props (grass sprites), physics collision incl. precomputed displacement hulls |
| `gvis` | vvis | `crates/gold-vis` | portal-flow PVS/PAS, sky leaf flags, leaf→water distances |
| `grad` | vrad | `crates/gold-rad` | lightmaps (LDR + HDR, bumped), switchable light styles, texture lights (`lights.rad`), radiosity bounces, leaf ambient cubes, world lights, static prop vertex lighting, detail prop lighting, vertex normals |

## Usage

```bash
cargo build -r
```

```bash
target/release/gbsp -game "C:/Program Files (x86)/Steam/steamapps/common/Source SDK Base 2013 Singleplayer/hl2" sdk_vehicles.vmf
```

```bash
target/release/gvis sdk_vehicles
```

```bash
target/release/grad sdk_vehicles
```

`-game` is optional: without it the tools use `$VPROJECT`, then the usual Steam
install locations (Source SDK Base 2013 Singleplayer, then Half-Life 2).

A leaking map writes `<map>.lin` (leak path points) and compiles without filling the outside.

## Known bugs
* Some meshes are not lit properly, causing props to receive incorrect lighting.
