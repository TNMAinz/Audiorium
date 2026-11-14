# Security policy

## Reporting a vulnerability

Please report privately through GitHub: open the repository's **Security** tab and
choose **Report a vulnerability**. That opens a private advisory which only you and
the maintainer can read, so nothing is disclosed while a fix is being made. Please
do not open a public issue for something that could be exploited.

A useful report says which version you ran — the installer's file name, or the
`AUDIORIUM <version> started` line in `%APPDATA%\AUDIORIUM\audiorium.log` — what you
did, what happened, and whether it needs a particular file to reproduce.

## Supported versions

The latest release is the one that gets fixes. Each version is published as a single
Windows installer under [Releases](https://github.com/TNMAinz/Audiorium/releases);
older installers are not patched. If you build from source, `main` is the supported
tree.

## What is in scope

Audiorium opens local files and writes local files. It has no network code, no
accounts and no telemetry, so the surface is what a file or a device name can reach:

- **Decoding and importing.** A malformed MP3, FLAC, WAV, ALAC, AAC, Ogg Vorbis or
  AIFF file — or an imported wavetable, Scala tuning file or equaliser patch — that
  causes a crash, a read or write outside the file's own buffers, or unbounded
  memory use.
- **Text taken from files.** Tags, cover art, track titles, patch names and device
  names are shown in the interface; anything there that could run script or escape
  the WebView is in scope.
- **Paths and the shell.** Anything that lets a track, a master destination, a
  preset or a library folder reach outside the location it is meant to, including
  writing a file somewhere the app should not.
- **The audio path.** The processors, the spatial renderer, the offline render and
  the exporters — a file that makes them read or write past a buffer, or a master
  that contains memory it should not.
- **The release itself.** The installer and `audiorium.exe` as published.

A crash on a malformed file is worth reporting even when you cannot see a security
impact: it is a robustness bug, and it is often the first sign of one.

## What is not in scope

- **The optional virtual audio device.** The kernel driver that can give Audiorium
  its own endpoints is a separate build and is not part of this repository or of the
  installer. Its own project is the place to report against it.
- **Windows, WebView2 and third-party crates.** Report those upstream; tell us as
  well if the issue reaches Audiorium through this app.
- **The installer's unsigned status.** Windows shows an unknown-publisher warning
  because no code-signing certificate is configured. That is a known limitation,
  recorded in the README, not a vulnerability.
- **Denial of service that needs control of the machine.** A local app that can be
  stopped by the person using it is not a finding on its own.

## What to expect

This is a small project, so fixes are best effort rather than promised on a clock.
You will get an acknowledgement, an assessment of whether the report is in scope, and
a note when a fix ships. If you would like credit, say so and the advisory will name
you; if you would rather stay anonymous, it will not. Please give a fix a reasonable
window before publishing details — 90 days from the report, or sooner if a release
carrying the fix is out.

## What is already on

Dependabot alerts and security updates watch the Rust and npm dependencies,
secret scanning and push protection are enabled for the repository, and CodeQL
code scanning runs over the TypeScript frontend on every push to `main`. Code
scanning does not yet cover the Rust side.
