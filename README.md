# Alumina UI

![Screenshot](doc/screenshot.png)

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
- implement picking for lines and vertices
- https://github.com/JeroenGar/jagua-rs for bin packing
- generate toolpaths from slices
- send toolpaths to firmware
- implement SD card support
- create tab for oscope
- create tab for node graph
