# Changelog

## 0.3.0

- **Insights.** A new tab shows what is in your Discord data package, calculated on your computer and without
  logging in: an overview with streaks, breaks and your first message; a calendar heatmap of every day,
  messages per month or week by server, weekday × hour and by hour; your servers and DMs with their channels;
  a word cloud with the exact ranking, emoji, the people you mention most and message lengths; links and
  attachments; and a search. Filter by period and place, click to zoom in, and hand what you find to Clean up.
  One million messages open in about a second. In the CLI: `stats --section …` and `search`.
- **Encrypted backups.** The backup is one encrypted archive (age: ChaCha20-Poly1305, scrypt), written while
  deleting as encrypted parts so that a message is only deleted once its files are safely on disk. Passphrase:
  twelve generated words confirmed by typing three of them, your own, or a file. Exports can be encrypted too
  (the default in the CLI; `--no-encrypt` to opt out). **Open a backup…** / `erasecord open-backup` decrypts them.
- **Getting the data package:** a guide in the app with links to Discord's help; drop a package onto the window;
  the last one opens again with one click.
- **Updates:** the app offers new versions and installs signed updates when you click.
- **Windows:** only the setup `.exe` is built (with the install date and publisher in *Installed apps*); it
  removes an `.msi` install of 0.2.0. The README explains how to uninstall on every system.
- Data packages are read twice as fast. `erasecord inspect-package` describes a package without any of its
  content, and `anonymize-package` makes a copy with every value replaced, for bug reports.

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
