# All Your BASE

Plug-in for Glyphs 3 and 4. After export, it adds a [BASE table](https://learn.microsoft.com/en-us/typography/opentype/spec/base) with script-specific MinMax values to the exported OTF/TTF files, so applications can adapt vertical metrics to the script in use.

The measuring is done by [autobase](https://github.com/simoncozens/autobase) (its `-m` mode), built into the plug-in as a Rust library: it shapes words from the bundled word lists (and your own words) for every script in the font, at every interesting location of a variable font, and records extents that exceed the OS/2 typo ascender/descender.

Every script in the font also gets the ideographic em-box baselines `ideo` and `idtp` (with `romn` = 0 as default baseline), so applications like InDesign can align the font with CJK fonts on the same line. By default, `ideo` is 12% of the em below the baseline (-120 at 1000 UPM), and `idtp` is `ideo` + UPM.

## Usage

Add custom parameters to an instance or a Variable Font Setting in *File > Font Info > Exports*:

| Parameter | Value | Example |
|---|---|---|
| `BASE Table` | Switches the plug-in on for this export. Required. | `1` |
| `BASE Tolerance` | Font units within which values count as equal to the font or script default. | `10` |
| `BASE Languages` | Comma-separated `language_Script` codes that get their own MinMax record. | `vi_Latn, fi_Latn` |
| `BASE Override` | Fixed min and/or max for a script or language. One parameter per entry. | `fi_Latn; max=1234` |
| `BASE Exclusions` | Comma-separated words or word fragments to ignore when measuring. | `Ằ` |
| `BASE Words Per List` | Words tested per word list. Default 1000. | `2000` |
| `BASE Words File` | UTF-8 file with extra words, path relative to the .glyphs file. | `base-words.txt` |
| `BASE Ideographic Bottom` | `ideo` in font units, instead of the default. `idtp` follows as `ideo` + UPM. | `-120` |

Results and problems are reported in the Macro Window. If a parameter cannot be read, no BASE table is added. WOFF/WOFF2 files are skipped.

### Words file

Section headers name a script (ISO 15924), optionally with a language (ISO 639). Words are separated by whitespace, lines starting with `#` are ignored:

```text
# extra words
[Latn]
ǺÅ Ǿ
[vi_Latn]
Ặp Ỹ
```

## Building

Requires Rust with the `aarch64-apple-darwin` and `x86_64-apple-darwin` targets. The first build downloads the word lists.

```sh
./build.sh
```

This puts a universal `libautobase_glyphs.dylib` into `All Your BASE.glyphsPlugin/Contents/Resources/`.

## Credits and licenses

- [autobase](https://github.com/simoncozens/autobase) by Simon Cozens, Apache-2.0.
- [fontheight](https://github.com/googlefonts/fontheight) and [static-lang-word-lists](https://crates.io/crates/static-lang-word-lists), Apache-2.0.
- AOSP word lists from [aosp-test-texts](https://github.com/googlefonts/aosp-test-texts), Apache-2.0.
- LibreOffice word lists from the [LibreOffice dictionaries](https://cgit.freedesktop.org/libreoffice/dictionaries), [MPL-2.0 / LGPL-3+](https://www.libreoffice.org/about-us/licenses).
