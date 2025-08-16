# Path of Exile CDN Bundle Index - Developer Guidelines

This document provides essential information for developers working on the
poecdn-bundle-index project.

## Project Overview

This tool connects to Path of Exile CDN servers, downloads bundle information,
and creates a searchable index of game files. It supports both Path of Exile 1
and Path of Exile 2, generating:

1. SQLite databases with file, bundle, and directory information
2. CSV files with bundle and file data
3. SQL files for database population

## Key Components

- **Connection Protocol**: The tool connects to PoE CDN servers using a custom
  protocol (port 12995 for PoE1, port 13060 for PoE2)
- **Bundle Processing**: Bundles are downloaded, decompressed, and parsed to
  extract file metadata
- **Output Generation**: The tool generates SQL files, CSV files, and a SQLite
  database

## Usage Examples

### Basic Usage

```bash
# Process Path of Exile 1 files
cargo run --release -- poe1

# Process Path of Exile 2 files
cargo run --release -- poe2
```

### Output Structure

The tool creates an `output` directory with subdirectories for each game
version:

```
output/
├── poe1/
│   ├── bundle_index.sqlite # Full database created from the sql scripts
│   ├── bundles.sql         # SQL insert statements for bundles
│   ├── dirs.sql            # SQL insert statements for directories
│   ├── version.sql         # SQL insert statement for version info
│   ├── urls.json           # JSON file with all bundle URLs
│   └── patch.poecdn.com/   # Directory with CSV files
│       └── [version]/
│           ├── bundles.csv  # CSV file with bundle information
│           └── files.csv    # CSV file with file information
└── poe2/
    └── [similar structure]
```

The ./output/ directory is uploaded to Github Pages
at https://lvlvllvlvllvlvl.github.io/poecdn-bundle-index/ with the same
structure - e.g. the poe1 bundles index is
at https://lvlvllvlvllvlvl.github.io/poecdn-bundle-index/poe1/bundle_index.sqlite

## Database Schema

The SQLite database contains four tables:

1. **version**: Stores the CDN URL
2. **bundles**: Stores bundle information (id, name, size)
3. **dirs**: Stores directory information (id, name, parent)
4. **files**: Stores file information (hash, dir, name, bundle, offset, size)

## CI/CD Pipeline

The GitHub Actions workflow (.github/workflows/build.yml) runs every 4 hours to:

1. Process both PoE1 and PoE2 data
2. Generate SQL files for database population
3. Upload SQL files as artifacts
4. Deploy data to Cloudflare D1 databases
5. Generate and deploy a GitHub Pages site with the index

## Development Notes

- **Network Dependency**: The tool requires internet access to connect to PoE
  CDN servers
- **Version Updates**: Game version paths in URLs may change with game updates
- **Error Handling**: The tool includes robust error handling for network issues
  and malformed data
- **Testing**: Run `cargo test` to test any changes
