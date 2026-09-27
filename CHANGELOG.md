# Changelog

## 0.2.0

- **Back up attachments before deleting.** The images and files of each message are saved to a folder of your
  choice first, together with a list of the messages. A message whose files cannot be saved is kept. In the CLI:
  `--backup DIR`.
- **Continue interrupted clean-ups.** A run that was stopped, crashed or lost its connection continues where it
  left off, without searching or counting anything twice. In the CLI: `--state FILE` and `erasecord resume FILE`.
- **Presets.** The app remembers the settings you used last; presets save settings together with the selected
  servers, DMs and channels under a name.
- **Statistics.** For an imported data package: messages per month, a weekday-by-hour heatmap, your busiest day
  and the servers and DMs with the most messages. In the CLI: `erasecord stats PACKAGE`.
- The list of messages saved with **Save list…** / `--export` includes the backed-up files.

## 0.1.0

First release: delete your messages from chosen servers, channels and DMs by time range and content, with
preview, dry run, data package import, overwriting, export and five languages.
