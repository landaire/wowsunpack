# wowsunpack

A utility for unpacking World of Warships game assets.

[![crates.io](https://img.shields.io/crates/v/wowsunpack.svg)](https://crates.io/crates/wowsunpack)  [![docs.rs](https://img.shields.io/docsrs/v/wowsunpack.svg)](https://docs.rs/wowsunpack/)

## Features

- Directly read and convert `GameParams.data` to JSON
- Dump IDX file resource metadata to a serialized format (JSON or CSV)
- Extract game assets using glob file patterns
- Use this utility as a library

Planned:

- [ ] Parsing assets.bin
- [ ] C FFI
- [ ] Refactoring of library APIs

## Usage

```
Usage: wowsunpack [OPTIONS] <COMMAND>

Commands:
  extract      Extract files to an output directory
  metadata     Write meta information about the game assets to the specified output file. This may be useful for diffing contents between builds at a glance. Output data includes file name, size, CRC32, unpacked size, compression info, and a flag indicating if the file is a directory
  game-params  Special command for directly reading the `content/GameParams.data` file, converting it to JSON, and writing to the specified output file path
  help         Print this message or the help of the given subcommand(s)

Options:
  -p, --pkg-dir <PKG_DIR>      Directory where pkg files are located. If not provided, this will default relative to the given idx directory as "../../../../res_packages"
  -i, --idx-files <IDX_FILES>  .idx file(s) or their containing directory
  -h, --help                   Print help
  -V, --version                Print version
```

## Motivation

World of Warships game files are packed in two custom file formats -- `.idx` files and `.pkg` files. `.idx` files contain serialized resource and volume (.pkg) metadata. There exists [an official utility](https://forum.worldofwarships.com/topic/183662-all-wows-unpack-tool-unpack-game-client-resources/) provided by the game developer, Wargaming, but has the following drawbacks compared to this utility:

- Is not open-source
- This utility's backing library can be easily adopted into other applications that would like to directly read game data
- Slower (~2x margin using the CLI tool, ~6x margin using the GUI)
- Does not expose meta information about the resources
- Does not expose data in a machine-serializable format

That two points are the big motivator for development of this utility. Applications like [minimap_renderer](https://github.com/WoWs-Builder-Team/minimap_renderer) depend on game assets and reading these assets isn't easily automated with today's tools.