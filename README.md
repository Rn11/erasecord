# EraseCord

Delete your own Discord messages: from the servers and DMs you pick, in the time range you pick, whenever you
want. And see what you wrote: heatmaps, charts and word clouds from your Discord data package, calculated on your
computer. Open source, for Windows, macOS and Linux, as a desktop app and as a command line tool.

> [!WARNING]
> EraseCord logs in with your **user token**. Automating a user account is against
> [Discord's Terms of Service](https://discord.com/terms), so Discord could limit or ban your account. EraseCord
> keeps the risk low (it only deletes, sends one request at a time and waits whenever Discord asks it to) but
> cannot remove it. **Deleted messages cannot be restored.** Use at your own risk.

## Features

- **Pick where:** any of your servers (or single channels of them), DMs and group DMs, including DMs you have
  closed: EraseCord finds friends without an open DM and reopens the conversation for you.
- **Pick when:** messages older than N days/weeks/months/years, sent between two dates, or everything.
- **Pick what:** messages containing certain words or matching a regular expression, only messages with links,
  attachments, images, videos or audio, or everything except those (e.g. keep your photos).
- **Everything, even what search misses:** import Discord's data package to delete by message ID, including
  closed DMs and messages the search does not find.
- **Look before you delete:** counting shows the matching messages per server/DM and, while it reads them,
  statistics about them: when you wrote them, words and emoji you used most, files, links and how long deleting
  will take. A dry run lists every message that would be deleted; **Delete these messages now** then deletes
  exactly those. Messages are searched for only once: the dry run and the clean-up reuse what counting found.
- **Keep pinned messages** if you like, or **overwrite** each message with random text before deleting it.
- **Back up before deleting, encrypted:** the images and files of your messages and a list of the messages go
  into one encrypted archive before anything is deleted. A message whose files cannot be saved is kept. The
  archive is protected by a passphrase: twelve generated words, your own, or a passphrase file.
- **Pause, resume or stop** at any time; live progress with an activity log and time estimate. A clean-up that
  was stopped or cut short (crash, lost connection) **continues where it left off**, without searching or
  counting anything twice.
- **Presets:** the app remembers your last settings, and you can save settings together with the selected
  servers and DMs under a name.
- **Insights into your own messages**, from your data package and without logging in: a calendar heatmap of
  every day, messages per month or week by server, a weekday-by-hour heatmap, your busiest servers and DMs,
  a word cloud, emoji, the people you mention most, links, attachments and a search. Filter everything by time
  and place, and hand what you find straight to *Clean up*. Nothing is sent to Discord.
- **Keep a record:** export the messages with their text, date and attachment links as CSV, JSON or JSON Lines,
  encrypted if you like: right after counting (before anything is deleted), automatically while deleting, or
  afterwards.
- **In your language:** English, German, Spanish, Swedish and Ukrainian. The app follows your system language,
  and you can pick another one at any time.
- **Gentle with Discord:** strictly one request at a time, 2.5 s between deletions and 3 s between searches (each
  varying randomly), a longer break every 100 deletions, and even slower after every rate limit. See
  [How gentle is it?](#how-gentle-is-it)
- **Always know what it is doing:** a status line says whether EraseCord is counting, reading, deleting, backing
  up, taking a break or waiting for Discord, with a countdown.
- **Your token stays with you:** it is only sent to discord.com. Optionally it is remembered in your system's
  credential store (Keychain, Windows Credential Manager, Secret Service).
- **Updates itself:** the app offers new versions when they are out; updates are signed and only installed when
  you click.

## Install

Download the installer for your system from the [releases page](https://github.com/Rn11/erasecord/releases):
`-setup.exe` for Windows, `.dmg` for macOS, `.AppImage`, `.deb` or `.rpm` for Linux. The files named
`erasecord-cli-*` are the command line tool only (up to 0.3.0: `erasecord-*`), a single program for the terminal
without a window; most people do not need them.

The builds are not code-signed yet:

- **macOS:** right-click the app and choose *Open*, or run `xattr -d com.apple.quarantine /Applications/EraseCord.app`.
- **Windows:** in the SmartScreen dialog choose *More info → Run anyway*.
- **Linux (AppImage):** `chmod +x EraseCord_*.AppImage` and run it.

From 0.3.0 on, the app looks for a newer version a few seconds after it starts (one request to github.com) and
offers it; it is only downloaded and installed when you click *Update and restart*, and only if it is signed with
EraseCord's key.

### Uninstall

- **Windows:** *Settings → Apps → Installed apps*, search for *EraseCord*, then *⋯ → Uninstall*. The installer puts
  the app in `%LOCALAPPDATA%\EraseCord`, so running `%LOCALAPPDATA%\EraseCord\uninstall.exe` works too. Version
  0.2.0 also came as an `.msi`; if you installed that one, it is listed as well, or run
  `msiexec /x EraseCord_0.2.0_x64_en-US.msi` in the folder you downloaded it to. The uninstaller offers to delete
  your settings too; a remembered token is removed when you log out.
- **macOS:** move *EraseCord* from *Applications* to the Bin.
- **Linux:** `sudo apt remove erase-cord` or `sudo dnf remove erase-cord`, or delete the AppImage.

## Finding your token

1. Open [discord.com/app](https://discord.com/app) in your browser and log in.
2. Open the developer tools (<kbd>F12</kbd>, or <kbd>⌥</kbd><kbd>⌘</kbd><kbd>I</kbd> on macOS) and switch to the
   *Network* tab.
3. Type `api` into the filter box, then click on any channel so requests show up.
4. Select one of the requests. Under *Request Headers*, the value of `authorization` is your token.

Your token gives full access to your account: never share it. Changing your Discord password invalidates it.

## Using the app

1. Paste your token and confirm that you understand the risk.
2. Tick the servers and DMs to clean up and choose the time range. **Channels** next to a server lets you pick
   single channels; in the DM tab, **Find friends without an open DM** reaches closed conversations. Under
   *Content* you can narrow it down to words, links, attachments and so on.
   Under *Save before deleting*, **Back up attachments before deleting** saves images, files and the list of
   messages into one encrypted archive in a folder of your choice first (a dry run with this option only backs
   up), and **Export the messages to a file** writes every deleted message into a new file while deleting.
   Both are encrypted with one passphrase: twelve generated words that you confirm by typing three of them,
   your own, or a file. Keep it: without it the backup cannot be opened. **Open a backup…** decrypts and unpacks
   it again.
3. Click **Count messages**. EraseCord counts first, then reads the messages and shows statistics about them
   as it goes; **Stop reading** keeps what was read. **Export these messages…** saves them to a file before
   anything is deleted.
4. Click **Delete** (or first **List them first (dry run)**, and then **Delete these messages now**) and watch
   the progress; the status line says what is happening. You can pause or stop at any point. **Export deleted
   messages…** saves every message of the run as CSV (opens in any spreadsheet program) or JSON.

If a clean-up is stopped or the app is closed in the middle, the setup screen offers to **Continue** it later,
with exactly the servers, DMs and conditions it had (and asks for the backup's passphrase, if it has one). The settings you used last are filled in automatically;
**Save as preset…** above the time range keeps the current settings and selection under a name.

The language menu on the login screen and in the top bar switches the language; *System language* follows your
computer's setting.

### Using your data package

Discord's search only finds what it has indexed, and only in places you can still see. Your data package lists
every message you ever sent:

1. In Discord, open *User Settings → Data & Privacy* and request your data (*Request all of my data* in the
   phone app). *Messages* is needed; *Servers* and *Account* are recommended. It can take up to 30 days; the
   download link in the e-mail then works for 30 days. The app shows these steps under *How do I get my data
   package?*.
2. In EraseCord, click **Import data package…** above the server list and pick the `.zip` file (or **(folder)**
   for an extracted one), or simply drop it onto the window. Next time, **Open the last package** opens it again.
3. The list now shows every server and DM from the package with the number of your messages, also closed DMs
   and servers you have left (marked *left*; Discord no longer lets you delete there). Continue as usual; counts
   are exact and need no searching. **Back to live search** switches back.

### Insights

The **Insights** tab shows what is in your data package: an overview with streaks and your first message, time
charts, your servers and DMs, words and emoji, links and files, and a search. It works without logging in (*Open
Insights without logging in* on the login screen), and a package imported in *Clean up* is already there. The
filters at the top (period, servers and DMs) apply to everything; clicking a year, month, day, server or word
narrows it down. **Clean up…** next to a server or search result opens *Clean up* with it filled in, to be
counted and checked before anything is deleted. One million messages open in about a second.

The package stays on your computer and is only read, never uploaded. A package of another account is refused.
Messages you deleted after requesting it are simply counted as deleted again. The package knows the people in a
DM only by their ID, so DMs and group DMs you still have open are shown with their current names.

## Command line

```
erasecord list                                    # your servers and DMs with their IDs
erasecord preview --all-servers --before 30d      # count, delete nothing
erasecord delete --all-dms --before 1y --dry-run  # list what would be deleted
erasecord delete -t 81384788765712384 --after 2023-01-01 --before 2024-01-01 --skip-pinned
erasecord channels 81384788765712384              # the channels of a server, for --channel
erasecord delete -c 81384788765712390 --contains "party tonight" --without image,video
erasecord list --friends                          # also friends whose DM is closed, for --dm-with
erasecord delete --package package.zip --all-dms --has link --overwrite
erasecord delete --all-servers --before 1y --dry-run --export old-messages.csv
erasecord delete --all-dms --has file --backup ~/discord-backup --state run.json
erasecord resume run.json                         # continue a clean-up that was stopped
erasecord open-backup ~/discord-backup/erasecord-backup-*.tar.age --to ~/restored
erasecord stats package.zip --section words --from 2024-01-01 --to 2024-12-31
erasecord search package.zip party tonight        # your messages with both words
erasecord inspect-package package.zip             # its structure only, safe to share
```

- Choose targets with `--target/-t <ID>` (repeatable), `--channel/-c <ID>` (narrows its server down to those
  channels), `--dm-with <USER_ID>` (reopens a closed DM), `--all-servers` and `--all-dms`.
- Filter the content with `--contains <WORDS>` (all words, any case), `--pattern <REGEX>` (case-insensitive),
  `--has <KINDS>` and `--without <KINDS>`, where kinds are `link`, `file`, `image`, `video`, `sound`, `embed` and
  `sticker`, separated by commas.
- `--overwrite[=TEXT]` edits each message to TEXT (random letters if left out) and removes its attachments
  before deleting it.
- `--package <PATH>` takes the messages from your data package (`.zip` or extracted folder) instead of
  searching; `list --package` and `preview --package` work without a token.
- `--export FILE` saves every deleted message (with `--dry-run`: every message that would be deleted) with its
  text and attachment links, as JSON for a `.json` file, JSON Lines for `.jsonl` and CSV otherwise, encrypted as
  `FILE.age`. The file is written while deleting, so it is complete even if you stop early.
- `--backup DIR` saves the attachments of each message before deleting it, into one encrypted archive
  `DIR/erasecord-backup-<time>.tar.age` with the list of messages. A message whose files cannot be downloaded is
  kept. While the run is going the backup is written as encrypted parts (`….parts/`); a message is deleted only
  once the part with its files is safely on disk, and the parts are combined at the end.
- Backups and exports are encrypted unless you pass `--no-encrypt`. The passphrase is asked for (press Enter for
  twelve generated words, confirmed by typing three of them) or read from the first line of
  `--passphrase-file FILE`. `open-backup PATH --to DIR` decrypts an archive, a `.parts` folder or an export;
  the archives also open with [age](https://age-encryption.org) (`age -d FILE | tar x`).
- `--state FILE` records progress while deleting. If the run is stopped or cut short, `erasecord resume FILE`
  continues it with the same targets and conditions; the file is removed when the run finishes.
- `stats <PACKAGE>` prints statistics about your data package: `--section` `overview` (default), `time`,
  `places`, `words`, `links` or `info`; `--from`/`--to` (days, inclusive) and `--place <ID>` narrow it down;
  `--json` for JSON. `search <PACKAGE> <WORDS>…` lists your messages with all the words. Both need no token.
- `inspect-package <PACKAGE>` prints only the structure of a package (file and field names, counts, kinds of
  values; no messages, names, IDs or dates) and `anonymize-package <PACKAGE> <OUT.zip>` writes a copy with every
  value replaced, for bug reports and testing.
- Dates: `YYYY-MM-DD` (local midnight), an RFC 3339 timestamp, or an age like `30d`, `12w`, `6m`, `1y`.
  `--after` is inclusive, `--before` exclusive.
- `delete` shows the preview and asks for confirmation unless you pass `--yes`.
- The token is asked for (hidden) when the `DISCORD_TOKEN` environment variable is not set. Prefer the prompt:
  a token typed into a command line ends up in your shell history.
- Ctrl+C stops after the current request; press it twice to quit at once.

Run `erasecord help <command>` for all options.

## How it works

Discord has no API for bulk-deleting your own messages, so EraseCord does what you would do by hand, only
faster. It searches each server or DM for messages written by you (`author_id`) within the time range, where
the dates are turned into message IDs because every Discord ID contains its creation time. It then deletes the
results one by one, newest first, paging with an ID cursor. Afterwards it searches once more, because Discord's
search index can lag behind. Words and content types go to the search as well, but every result is checked
again before it is deleted: author, time range, channel, words, regular expression and content type must all
match, so a fuzzy search result is never deleted by mistake (it shows up as *skipped: excluded by filter*).

Messages are only searched for once. Counting reads them page by page and keeps them in memory (never on disk;
forgotten after 30 minutes, on logout and when the app closes). A dry run or clean-up afterwards takes them from
there and only checks once whether anything new turned up. The lists of servers, DMs and channels are kept in
memory for 15 minutes as well; the refresh button asks Discord again.

With a data package there is no searching: EraseCord takes the message IDs from the package, checks each channel
once (so a deleted channel or a server you left costs one request, not one per message) and deletes the
matching messages directly.

### How gentle is it?

Discord allows about five deletions in five seconds. Tools that go near that limit, like
[Undiscord](https://github.com/victornpb/undiscord) with its defaults of 1 s between deletions and 0.1 s
between searches, get rate limited all the time, and its users [settle on 2 to 2.5
s](https://github.com/victornpb/undiscord/discussions/414) to avoid that. EraseCord stays below it by default:

- strictly one request at a time;
- 2.5 s between deletions and 3 s between searches, each varying randomly by ±25 %;
- a longer break (about 30 s) after every 100 deletions;
- whenever Discord rate limits it, it waits as long as Discord asks and makes every later pause 25 % longer
  (up to four times the setting) for the rest of the run;
- messages are searched for once, not again for the dry run and the clean-up.

That makes about 1,100 deletions an hour. The pauses can be changed under *Speed* (app) or with
`--delete-delay` and `--search-delay` (CLI), but shorter ones make rate limits and attention from Discord more
likely.

Limitations:

- You can only delete messages in servers you are still a member of.
- The data package does not say which messages have embeds or stickers, so those two filters are not
  available with it. Images, videos and audio are recognised by their attachments; to be safe, *keep messages
  with images/videos* also keeps messages with links, which may show one.
- System messages (joins, calls, pins) and messages in locked, archived threads cannot be deleted and are
  skipped.
- Without a data package, EraseCord can only delete what Discord's search finds.
- When single channels are picked, threads in them are not included (they are channels of their own).

## Building from source

Requirements: [Rust](https://rustup.rs) (stable), [Node.js](https://nodejs.org) (LTS), and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) for your system. On Debian/Ubuntu:

```
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libssl-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev
```

```
cargo test                                   # core and CLI tests
cargo build --release -p erasecord           # CLI → target/release/erasecord
cd app && npm ci && npm run tauri dev        # desktop app with hot reload
cd app && npm run tauri build                # installers → target/release/bundle/
```

### Developing without a Discord account

- **UI only:** `cd app && npm run dev` and open http://localhost:1420 in a browser. A mock backend answers
  instead of Rust; log in with any token (`bad` is rejected).
- **Everything against a fake Discord:** start `python3 tools/fake_discord.py` (add
  `--write-package package.zip` for a matching data package), then point a debug build at it:

  ```
  ERASECORD_API_BASE=http://127.0.0.1:8765/api/v9 npm run tauri dev            # in app/
  ERASECORD_API_BASE=http://127.0.0.1:8765/api/v9 cargo run -p erasecord -- list
  ```

  The fake server holds a few hundred messages, answers every endpoint EraseCord uses (search, delete, edit,
  channels, pins, friends) and now and then replies with 429 so rate limiting can be watched. Release builds of the app ignore `ERASECORD_API_BASE`.
- **Insights in the browser with real numbers:** `python3 tools/big_package.py big.zip --messages 100000`
  writes a large made-up package; `python3 tools/fake_discord.py --insights-package big.zip` then answers the
  mock backend's Insights queries by running the CLI (`cargo build -p erasecord` first).

### Releasing

Raise the version in `Cargo.toml`, `app/package.json` (`npm version X.Y.Z --no-git-tag-version`) and
`app/src-tauri/tauri.conf.json`, add a section for it to `CHANGELOG.md` and merge into `main`. The *Release*
workflow sees the new version, tags the commit, builds everything and publishes the release with that
changelog section. Updates are signed with the key in the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (made with `npx tauri signer generate`; its public half is in
`tauri.conf.json`). Without them, releases are built without update files.

## Project layout

```
crates/core/      Discord client, rate limiting, search, deletion job (all logic, UI-independent)
crates/cli/       the `erasecord` command line tool
app/src-tauri/    desktop app backend: Tauri commands around the core
app/src/          desktop app UI (SvelteKit + TypeScript)
tools/            fake Discord API and a large made-up data package, for development
```

## Roadmap

- Logging in with your Discord account instead of a token.
- Code-signed builds for macOS and Windows.
- Insights from the *Activity* part of the data package (time online, devices, voice), once its format can be
  checked against a real package: `erasecord inspect-package` output is welcome.

## License

[MIT](LICENSE). EraseCord is not affiliated with or endorsed by Discord Inc.
