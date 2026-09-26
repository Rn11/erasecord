# EraseCord

Delete your own Discord messages: from the servers and DMs you pick, in the time range you pick, whenever you
want. Open source, for Windows, macOS and Linux, as a desktop app and as a command line tool.

> [!WARNING]
> EraseCord logs in with your **user token**. Automating a user account is against
> [Discord's Terms of Service](https://discord.com/terms), so Discord could limit or ban your account. EraseCord
> keeps the risk low (it only deletes, sends one request at a time and waits whenever Discord asks it to) but
> cannot remove it. **Deleted messages cannot be restored.** Use at your own risk.

## Features

- **Pick where:** any of your servers, DMs and group DMs.
- **Pick when:** messages older than N days/weeks/months/years, sent between two dates, or everything.
- **Look before you delete:** a preview counts the matching messages per server/DM, and a dry run lists every
  message that would be deleted.
- **Keep pinned messages** if you like.
- **Pause, resume or stop** at any time; live progress with an activity log and time estimate.
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
2. Tick the servers and DMs to clean up and choose the time range.
3. Click **Count messages** to see how many of your messages match.
4. Click **Delete** (or first **List them first (dry run)**) and watch the progress. You can pause or stop at any
   point.

## Command line

```
erasecord list                                    # your servers and DMs with their IDs
erasecord preview --all-servers --before 30d      # count, delete nothing
erasecord delete --all-dms --before 1y --dry-run  # list what would be deleted
erasecord delete -t 81384788765712384 --after 2023-01-01 --before 2024-01-01 --skip-pinned
```

- Choose targets with `--target/-t <ID>` (repeatable), `--all-servers` and `--all-dms`.
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
search index can lag behind.

Limitations:

- You can only delete messages in servers you are still a member of, and only in open DMs. To reach a closed
  DM, open the conversation in Discord first.
- System messages (joins, calls, pins) and messages in locked, archived threads cannot be deleted and are
  skipped.
- If Discord's search does not find a message, EraseCord cannot find it either.

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
- **Everything against a fake Discord:** start `python3 tools/fake_discord.py`, then point a debug build at it:

  ```
  ERASECORD_API_BASE=http://127.0.0.1:8765/api/v9 npm run tauri dev            # in app/
  ERASECORD_API_BASE=http://127.0.0.1:8765/api/v9 cargo run -p erasecord -- list
  ```

  The fake server holds a few hundred messages, answers the search and delete endpoints, and now and then
  replies with 429 so rate limiting can be watched. Release builds of the app ignore `ERASECORD_API_BASE`.

## Project layout

```
crates/core/      Discord client, rate limiting, search, deletion job (all logic, UI-independent)
crates/cli/       the `erasecord` command line tool
app/src-tauri/    desktop app backend: Tauri commands around the core
app/src/          desktop app UI (SvelteKit + TypeScript)
tools/            fake Discord API for development
```

## Roadmap

- Filters by keyword, links and attachments; selecting single channels of a server.
- Finding closed DMs through the friend list.
- Optionally overwriting messages before deleting them.
- Importing Discord's data package, to reach every conversation and message ID directly.

## License

[MIT](LICENSE). EraseCord is not affiliated with or endorsed by Discord Inc.
