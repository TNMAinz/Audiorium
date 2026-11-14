# Working on Audiorium

## Versioning

Audiorium uses a fixed three-part version, and every change ships with the next one.

- **A small change** bumps the last number: `1.0.0` → `1.0.1`.
- **A large change** bumps the middle number and resets the last: `1.0.0` → `1.1.0`.
- **No part counts past 9.** The last number runs `1.0.0` … `1.0.9`, and the next change after `1.0.9` is `1.1.0`, never `1.0.10`. The same carry applies upward: `1.9.9` → `2.0.0`.

Examples: `1.0.0` → `1.0.1` (a fix), `1.0.1` → `1.0.2` (another fix), `1.0.9` → `1.1.0` (the fix after nine), `1.1.4` → `1.2.0` (a feature).

The version lives in four places and they must agree: `package.json`, `package-lock.json` (both the root entry and `packages[""]`), `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml`. `Cargo.lock` follows from the next build. The built artefacts carry it too — `AUDIORIUM_<version>_x64_en-US.msi`, the exe's `FileVersion`/`ProductVersion`, the Apps & features `DisplayVersion`, and the first line of `%APPDATA%\AUDIORIUM\audiorium.log` — so rebuild after bumping. The MSI is the artefact a release ships; the NSIS setup is not built.
