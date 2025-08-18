# Alumina UI

Alumina is an integrated CAD/CAM, physics simulation, and motion control solution written entirely in Rust.  It is intended to control laser and plasma cutters, 3D printers, CNC routers and mills, and lathes.  There are two parts to Alumina: the [firmware](https://github.com/timschmidt/alumina-firmware) which targets the esp32c3 microcontroller, sets up a Wifi AP called "Alumina", serves the Alumina UI via HTTP, responds to commands from the Alumina UI via HTTP, and performs motion planning and step generation.  The [UI](https://github.com/timschmidt/alumina-ui) targets [WebAssembly](https://en.wikipedia.org/wiki/WebAssembly), draws geometry using WebGL and egui, and manipulates geometry using [csgrs](https://github.com/timschmidt/csgrs).  Both parts fit in the onboard flash of the esp32c3.

<img src="doc/alumina-diagram.png" width="40%" alt="Diagram"/>

<img src="doc/screenshot-design.png" width="30%" alt="Design screenshot"/><img src="doc/screenshot-control.png" width="30%" alt="Control screenshot"/><img src="doc/screenshot-diagnostics.png" width="30%" alt="Diagnostics screenshot"/>

Try the [Web Demo](https://timschmidt.github.io/alumina-ui/) by clicking the link.

## Community
[![](https://dcbadge.limes.pink/api/server/https://discord.gg/cCHRjpkPhQ)](https://discord.gg/9WkD3WFxMC)

## Development
### Set up toolchain
```shell
cargo install trunk wasm-opt wasm-tools
```

### Run locally
```shell
trunk serve --open --release
```

## Todo
- implement picking for lines and vertices and faces
- single-click for individuals and click-drag for multiples.
- https://github.com/JeroenGar/jagua-rs and/or https://github.com/JeroenGar/sparrow for bin packing
- generate toolpaths from slices
- send toolpaths to firmware
- echo sent / received commands in Diagnostics console
- finish SD card support
- enable persistence via https://docs.rs/eframe/latest/eframe/
- implement tweening for snap view
- ensure font picker in truetype text node works / gets pre-populated
- ensure pin logging works for OUTPUT and INPUT modes and reports apropriately per-pin
- add SD, command stream to architecture graphic
- API documentation
