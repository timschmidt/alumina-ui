<img src="doc/alumina-diagram.png" width="40%" alt="Diagram"/>

# Alumina UI

Alumina is an integrated CAD/CAM, physics simulation, and motion control solution written entirely in Rust.  It is intended to control laser and plasma cutters, 3D printers, CNC routers and mills, and lathes.  There are two parts to Alumina: the [firmware](https://github.com/timschmidt/alumina-firmware) which targets the esp32 and esp32-c series microcontrollers, sets up a Wifi AP called "Alumina", serves the Alumina UI via HTTP, responds to commands from the Alumina UI via HTTP, and performs motion planning and step generation.  The [UI](https://github.com/timschmidt/alumina-ui) targets [WebAssembly](https://en.wikipedia.org/wiki/WebAssembly), draws geometry using WebGL and egui, and manipulates geometry using [csgrs](https://github.com/timschmidt/csgrs).  Both parts fit in the onboard flash of the microcontroller, reducing design complexity, part count, and cost.

<img src="doc/screenshot-design.png" width="30%" alt="Design screenshot"/> <img src="doc/screenshot-control.png" width="30%" alt="Control screenshot"/> <img src="doc/screenshot-diagnostics.png" width="30%" alt="Diagnostics screenshot"/>

Try the [Web Demo](https://timschmidt.github.io/alumina-ui/) by clicking the link.

## Community
[![](https://dcbadge.limes.pink/api/server/https://discord.gg/cCHRjpkPhQ)](https://discord.gg/cCHRjpkPhQ)

## HTTP API
```
/						GET index.html
/alumina-ui.js			GET alumina-ui.js
/alumina-ui.html		GET alumina-ui.html.gz
/alumina-ui_bg.wasm		GET alumina-ui_bg.wasm.br
/favicon.ico			GET favaicon.gif
/time					GET 
/files					POST 
/queue					GET, POST 
/board					GET json: {{"name":"{}","image_mime":"{}","image_url":"/board/image"}}
/board/image			GET PNG formatted board image
```

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
- add command stream to architecture graphic
- switch to shift-scroll for zoom, so two-finger scroll can be used for pan in X and Y for mobile
- figure out improper rendering in Chrome Android Pixel 6a
