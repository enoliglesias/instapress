---
title: UI Design
status: active
---

# UI Design

The frontend is static HTML, CSS and JavaScript (ADR 0002). The interface is in English. The only Spanish text the app writes is the "Ver el vídeo" link inside the WordPress post, which is the content of the site and not of the interface.

## Palette
The main color is the teal **#00A498**. White text on it has a contrast of only 3.1:1, which is below the 4.5:1 that text needs, so the palette separates what the teal is used for:

| Token | Light | Dark | Used for |
|---|---|---|---|
| `--accent` | `#00a498` | `#00a498` | Fills: buttons, the line under the selected tab |
| `--accent-fg` | `#052a26` | `#052a26` | Text on an accent fill (4.9:1) |
| `--accent-text` | `#00776e` | `#2dc9bc` | Text, borders and focus rings that use the accent (4.9:1 or more on the page, 5.4:1 on a card; 8.5:1 in dark) |
| `--bg` / `--surface` | `#eff5f4` / `#ffffff` | `#0c1312` / `#121b1a` | The page and the cards |
| `--fg` / `--muted` | `#14211f` / `#526866` | `#e5f0ee` / `#8fa6a3` | Text (15:1) and secondary text (5.4:1 or more) |
| `--field-line` | `#768986` | `#5f7c77` | The outline of fields, the editor and secondary buttons, at 3:1 or more, which is what tells a control from the page |
| `--line` | `#d0deda` | `#24352f` | Soft separators of cards only |
| `--error` | `#b3263e` | `#ff7b8f` | Errors, with 6.4:1 or more |

The dark theme follows the system setting (`prefers-color-scheme`). The neutrals lean to the teal so the page does not look grey next to it.

## Menu bar
- **File > Exit** closes the app.
- **View > 2 columns** is a check item that switches between the two layouts below. The menu flips its own check mark and the backend emits a `layout` event with the new state; the window applies it and remembers it in `localStorage`. When the window starts it applies the remembered layout and calls `set_layout_menu` so the check mark agrees with it. On macOS an Edit menu (undo, redo, cut, copy, paste, select all) is added as well, because the copy and paste shortcuts of a webview work through it there.

- **Help > About Instapress** makes the backend emit an `about` event with the app version; the window opens a modal `<dialog>` with what the app does, the author (Enol Iglesias), the MIT license and a no-warranty note. It needs no new permission and no dialog plugin.

## Layouts
- **One column** (the default): the tabs under a header that stays on screen; the content up to 760 px wide. In **Editing** the list of posts is a box of exactly four rows that scrolls inside, with the draft below it.
- **Two columns** (View > 2 columns, from 880 px of window width; below that the one-column layout is used whatever the choice): the content widens up to 1240 px. In **Editing** the posts are on the left in a card that stays in place while the draft on the right scrolls, and the list takes the height that is left under the header instead of four rows; until a post is selected the right column says "Select a post to start a draft." In **Credentials** the Instagram and WordPress cards sit side by side.

## Pieces
- **Tabs** follow the WAI-ARIA tabs pattern (see [`system-context.md`](system-context.md)); a dot on **Credentials** marks that the token or the WordPress settings need attention.
- **The editor toolbar** groups its buttons with the browser's editing commands (`execCommand`) and marks the ones that apply at the cursor with `aria-pressed`: a paragraph style menu (paragraph, headings 2 to 6, quote, code block); bold, italic, underline, strikethrough, superscript, subscript, inline code and highlight; bullet and numbered lists with indent and outdent (only inside a list); a link dialog (address, text, title and "open in a new tab"; only `http`, `https` and e-mail addresses are accepted, and Ctrl+K opens it); remove link, a table dialog (rows, columns and a header row), a horizontal line, clear formatting, undo and redo. Whatever the toolbar produces is cleaned again by the backend (see [`../domain/publication-draft.md`](../domain/publication-draft.md)). Pasted text still goes in as plain text.
- **The list of posts** has fixed-height rows (a 72 px picture and two lines of text), so four rows are always the same height. Shadows at the top and the bottom appear only on the side that has more to show.
- **Controls** all have a visible focus ring in `--accent-text`.
