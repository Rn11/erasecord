# Changelog

## Unreleased

- **Save what the others sent:** a backup can also include the images and files the others sent in the chosen
  time range, in DMs and group chats only, never on servers (*Also save what the others sent*, CLI
  `--backup-others`). Their messages are not touched; exports list them as `saved_from_others` with a new
  `author` column.
- **Calmer downloads:** backup files are downloaded 1 s apart (40 % of the pause between deletions) instead of
  back to back, and a "too many requests" from Discord's file server is waited out and slows the run down.
- **Emoji:** newer emoji such as 🫠 and 🩷 no longer show as empty boxes on Windows 10; the app brings Noto Color
  Emoji along. The live statistics while counting keep emoji whole (👍🏽, 👩‍💻, flags) instead of splitting them.

## 0.4.0

- **Messages are searched for only once.** Counting reads them and shows statistics while it goes (messages per
  month, weekday × hour, words and emoji used most, files, links, busiest day, how long deleting takes). The dry
  run and the clean-up use what it found and only check once for anything new. Kept in memory only.
- **After a dry run:** *Delete these messages now* deletes exactly those; *Back to settings* keeps everything.
- **Save before deleting:** export the messages to a file (CSV, JSON, JSON Lines) while deleting, set up with
  the backup and encrypted with the same passphrase, or right after counting, before anything is deleted.
  *Save list…* is now *Export deleted messages…*.
- **Status line:** says what EraseCord is doing and counts down while Discord asks it to wait.
- **Gentler pacing:** 2.5 s between deletions and 3 s between searches (was 1.2 s and 2 s), randomly varied, a
  longer break every 100 deletions and slower after every rate limit.
- Server, DM and channel lists are kept in memory for 15 minutes.
- Release files: the command line tool is now `erasecord-cli-*`, and the release notes say which file to
  download.
- The language menu has a single-colour icon.
- The app icon has a transparent background (it showed a black square on Windows).
- More robust: randomized tests with broken data packages, backups, filters and a Discord that fails at
  random; a time range before 2015 is refused with a clear message, a search that ignores its cursor can no
  longer page forever, and data package statistics are counted on all cores.
- Updating: 0.3.0 offers this version by itself a few seconds after starting.

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
