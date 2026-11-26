# Design

The extension's look, and why. The source of the components is the "Components" page of the
Figma file "tug for Chrome" (icons, tug's mark, buttons, status pills, the code card's states,
search, result rows, now playing, notices). The Figma plan's tool-call limit stopped the design
pass before the screens were drawn there, so the screens were finished in code from those
components. The PNGs here are screenshots of the real extension with made-up data
(`popup/popup.html?mock=<state>`).

| File | What |
|---|---|
| `wip-components.png` | The Figma components page |
| `popup-connected-light.png`, `popup-connected-dark.png` | The popup with a fresh code and music playing |
| `popup-copied-light.png` | After Copy |
| `popup-old-light.png` | A code over 5 minutes old: amber age, "may have expired", quieter Copy |
| `popup-nocode-light.png` | No code in the last 10 minutes |
| `popup-search-light.png`, `popup-search-dark.png` | Search with matches highlighted |
| `popup-disconnected-light.png` | iPhone not connected: a notice above the empty state |
| `popup-notrunning-light.png`, `popup-off-light.png` | tug not running; tug not letting the extension in |
| `options-light.png` | The options page after Check connection |
| `fill-toast.png` | Fill code from iPhone on a sign-in page, in a real browser |

## Decisions

- **tug's tokens, contrast-checked.** Colours are tug's own (`src/style.css`), mirrored in
  `styles/tokens.css`. Text is at least 4.5:1 on every surface it sits on: muted text is
  `#64625c` (tug's `#6c6a64` is 4.48:1 on the code card), and states have their own text and
  tint pairs (success, warning, error, accent) in light and dark.
- **The primary button rests on the deeper coral.** White on tug's `#cc785c` is 3.28:1, under
  the 4.5:1 needed for 14 px text; on `#a9583e` it's 5.06:1 (hover `#9e4f37`, pressed
  `#8f4631`), as tug's website does in light mode. The tug app's own buttons still use
  `#cc785c`; that's a separate change.
- **The code is the hero.** JetBrains Mono at 34 px, grouped the way people read codes aloud
  (482 913). The sender and Copy share the last row. Past 5 minutes the age turns amber and
  says the code may have expired (tug only hands out codes from the last 10 minutes).
- **Now playing stays device-chrome dark** in both themes, as in the app, with a round
  play/pause button.
- **No "Open tug".** An extension can't start a desktop app, so the footer has the fill
  shortcut (as the person set it) and settings instead.
- **The toast** sits in a closed shadow root over any site, with its own shadow, in the
  system font (a page can't load the extension's fonts without revealing that it's installed).
- **Icons** are Lucide, as in the app (`styles/icons.svg`); the toolbar icon is tug's mark
  rendered at 16, 32, 48 and 128 px (`icons/`).
