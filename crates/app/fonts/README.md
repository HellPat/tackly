Fonts embedded in the app, so it looks right offline.

- `roboto-latin.woff2`: Roboto (variable, weights 400–600, Latin), from Google
  Fonts. SIL Open Font License 1.1.
- `material-symbols-rounded.woff2`: Material Symbols Rounded, only the icons the
  app uses (see `ICONS` in `src/ui.rs`), from Google Fonts. Apache License 2.0.

To add an icon, add its name to `ICONS` and download the subset again:
`https://fonts.googleapis.com/css2?family=Material+Symbols+Rounded:opsz,wght,FILL,GRAD@24,400,0..1,0&icon_names=<sorted,comma-separated>`
(with a current browser user agent; the CSS it returns links the woff2).
