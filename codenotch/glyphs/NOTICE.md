# Provider marks

The default Claude, Codex, Cursor, and GitHub icons are original provider assets, copied byte-for-byte:

| File | Official source | Original asset | SHA-256 |
|---|---|---|---|
| `claude-original.svg` | [Anthropic media resources](https://www.anthropic.com/news) | `Claude logos/4 Claude icon/SVG/ClaudeIcon-Rounded.svg` | `059E22F525D67C6258C4F64514F0B0E717C914DF8A706936D0299D5E6B8082D9` |
| `cursor-original.png` | [Cursor brand assets](https://cursor.com/brand) | `App Icons/PNG/APP_ICON_25D_DARK.png` | `3A298AAAA2D973C49A6BCF2CCFE085B5DA5CA9E5FE350B09BF29FF3CDAD263F6` |
| `codex-original.png` | Official Microsoft Store-signed `OpenAI.Codex` Windows package | `assets/icon.png` | `88DB066A873A76DB49922AC801EEBD1E606D3A87986165234675AB225F1E08CD` |
| `github-original.svg` | [Official GitHub brand kit](https://brand.github.com/foundations/logo) | `GitHub_Invertocat_White_Clearspace.svg` | `ED102562E49CA3EA6E7D79EA10D54360DBECC5BEB33DE6A60FA578B6C0FAC803` |

These four files are rendered without recolouring, masks, effects, or redrawing. When a provider
is installed, Code Center may instead extract that installed executable's own icon at runtime.

`gemini.svg`, `gemini-alt.svg`, and the legacy unused SVG alternatives come from
[`@lobehub/icons-static-svg`](https://github.com/lobehub/lobe-icons) 1.95.0 (MIT). MIT License —
Copyright (c) LobeHub. See that repository's LICENSE.

**Trademarks**: these marks are trademarks of Anthropic, OpenAI, Anysphere (Cursor), GitHub, and Google
respectively. They are used only to identify the product whose activity and usage are displayed;
their inclusion does not imply endorsement or partnership. Use and redistribution remain subject
to each owner's current brand and trademark terms.

**Overrides**: a file of the same name (`.svg` or `.png`) in `%APPDATA%\codenotch\glyphs\` takes
precedence over the built-in mark; it is picked up after "Refresh usage now" in the tray menu.
