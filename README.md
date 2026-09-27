# EraseCord

Delete your own Discord messages: from the servers and DMs you pick, in the time range you pick, whenever you
want. Open source, for Windows, macOS and Linux, as a desktop app and as a command line tool.

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
- **Look before you delete:** a preview counts the matching messages per server/DM, and a dry run lists every
  message that would be deleted.
- **Keep pinned messages** if you like, or **overwrite** each message with random text before deleting it.
- **Back up before deleting:** save the images and files of your messages to a folder first, together with a
  list of the messages. A message whose files cannot be saved is kept.
- **Pause, resume or stop** at any time; live progress with an activity log and time estimate. A clean-up that
  was stopped or cut short (crash, lost connection) **continues where it left off**, without searching or
  counting anything twice.
- **Presets:** the app remembers your last settings, and you can save settings together with the selected
  servers and DMs under a name.
- **Statistics:** see from your data package when and where you wrote how much: messages per month, a
  weekday-by-hour heatmap and your busiest servers and DMs.
- **Keep a record:** save the list of deleted messages (or, after a dry run, the ones that would be deleted)
  with their text and attachment links as CSV or JSON.
- **In your language:** English, German, Spanish, Swedish and Ukrainian. The app follows your system language,
  and you can pick another one at any time.
- **Gentle with Discord:** strictly one request at a time, honours rate limits and retries temporary errors.
- **Your token stays with you:** it is only sent to discord.com. Optionally it is remembered in your system's
  credential store (Keychain, Windows Credential Manager, Secret Service).

## Install

Download the installer for your system from the [releases page](https://github.com/Rn11/erasecord/releases):
`.msi`/`.exe` for Windows, `.dmg` for macOS, `.AppImage`, `.deb` or `.rpm` for Linux. The command line tool is
attached to every release as a single binary.

The builds are not code-signed yet:

- **macOS:** right-click the app and choose *Open*, or run `xattr -d com.apple.quarantine /Applications/EraseCord.app`.
- **Windows:** in the SmartScreen dialog choose *More info → Run anyway*.
- **Linux (AppImage):** `chmod +x EraseCord_*.AppImage` and run it.

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
   Under *Options*, **Back up attachments before deleting** saves images and files to a folder of your choice
   first (a dry run with this option only backs up).
3. Click **Count messages** to see how many of your messages match.
4. Click **Delete** (or first **List them first (dry run)**) and watch the progress. You can pause or stop at any
   point. When it is finished, **Save list…** saves every message of the run as CSV (opens in any spreadsheet
   program) or JSON.

If a clean-up is stopped or the app is closed in the middle, the setup screen offers to **Continue** it later,
with exactly the servers, DMs and conditions it had. The settings you used last are filled in automatically;
**Save as preset…** above the time range keeps the current settings and selection under a name.

The language menu on the login screen and in the top bar switches the language; *System language* follows your
computer's setting.

### Using your data package

Discord's search only finds what it has indexed, and only in places you can still see. Your data package lists
every message you ever sent:

1. In Discord, open *User Settings → Data & Privacy → Request all of my data* and wait for the e-mail (this can
   take up to 30 days). Only *Messages* is needed.
2. In EraseCord, click **Import data package…** above the server list and pick the `.zip` file (or **(folder)**
   for an extracted one).
3. The list now shows every server and DM from the package with the number of your messages, also closed DMs
   and servers you have left (marked *left*; Discord no longer lets you delete there). Continue as usual; counts
   are exact and need no searching. **Back to live search** switches back, and **Statistics** shows when and
   where you wrote how much.

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
erasecord stats package.zip                       # statistics from your data package
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
  text and attachment links, as JSON for a `.json` file and CSV otherwise. The file is written while deleting,
  so it is complete even if you stop early.
- `--backup DIR` downloads the attachments of each message into DIR (`attachments/<channel ID>/`) before
  deleting it, and writes a list of the messages there (`messages-<date>.json`). A message whose files cannot
  be downloaded is kept.
- `--state FILE` records progress while deleting. If the run is stopped or cut short, `erasecord resume FILE`
  continues it with the same targets and conditions; the file is removed when the run finishes.
- `stats <PACKAGE>` prints statistics about your data package (`--json` for JSON); it needs no token.
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

With a data package there is no searching: EraseCord takes the message IDs from the package, checks each channel
once (so a deleted channel or a server you left costs one request, not one per message) and deletes the
matching messages directly.

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

### Releasing

Raise the version in `Cargo.toml`, `app/package.json` (`npm version X.Y.Z --no-git-tag-version`) and
`app/src-tauri/tauri.conf.json`, add a section for it to `CHANGELOG.md` and merge into `main`. The *Release*
workflow sees the new version, tags the commit, builds everything and publishes the release with that
changelog section.

## Project layout

```
crates/core/      Discord client, rate limiting, search, deletion job (all logic, UI-independent)
crates/cli/       the `erasecord` command line tool
app/src-tauri/    desktop app backend: Tauri commands around the core
app/src/          desktop app UI (SvelteKit + TypeScript)
tools/            fake Discord API for development
```

## Roadmap

- Logging in with your Discord account instead of a token.
- Code-signed builds for macOS and Windows.

## License

[MIT](LICENSE). EraseCord is not affiliated with or endorsed by Discord Inc.
