#!/usr/bin/env python3
"""A fake Discord API for trying EraseCord without a real account.

    python3 tools/fake_discord.py            # listens on http://127.0.0.1:8765
    ERASECORD_API_BASE=http://127.0.0.1:8765/api/v9 DISCORD_TOKEN=anything erasecord list

It serves two servers and three open DMs filled with messages from the last
two years, plus a friend whose DM is closed, and answers every endpoint
EraseCord uses: search (with content, has and channel_id), delete, edit,
channel lookup, pins, the friend list and reopening DMs. Now and then it
replies with 429 so rate-limit handling can be watched. The token "bad" is
rejected with 401. Only the Python standard library is needed.

    python3 tools/fake_discord.py --write-package package.zip

also writes a Discord data package of your messages, including a channel
that has since been deleted, to try `--package` and the app's import.
"""

import argparse
import json
import random
import re
import threading
import time
import zipfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

DISCORD_EPOCH_MS = 1_420_070_400_000
ME = "1000"
FRIENDS = {"2001": "Alice", "2002": "Bob", "2003": "Charlie", "2004": "Dana"}
# Server ID -> (name, {channel ID: (name, category)})
GUILDS = {
    "3001": ("Rust Enjoyers", {"3101": ("general", "Text"), "3102": ("help", "Text"), "3103": ("off-topic", None)}),
    "3002": ("Gaming Night", {"3201": ("lobby", None)}),
}
CATEGORIES = {"3001": {"3100": "Text"}}
# Open DMs: channel ID -> recipients. Dana's DM (4004) is closed.
DMS = {"4001": ["2001"], "4002": ["2002"], "4003": ["2001", "2002", "2003"]}
CLOSED_DMS = {"4004": ["2004"]}
# A channel only the data package remembers.
GONE_CHANNEL = "3999"

# Where attachments are served; set in main() once the port is known.
FILES_BASE = "http://127.0.0.1:8765/files"

lock = threading.Lock()
messages = {}  # id -> message dict (plus "_guild")
counters = {"deletes": 0}


def snowflake(ms, seq):
    return str(((ms - DISCORD_EPOCH_MS) << 22) + seq)


def user(user_id, name):
    return {"id": user_id, "username": name.lower(), "global_name": name, "avatar": None}


def seed(rng):
    now_ms = int(time.time() * 1000)
    two_years_ms = 2 * 365 * 24 * 3600 * 1000
    words = "hello ok lol nice see you tomorrow what about this party here gg thanks".split()
    places = [(gid, cid) for gid, (_, channels) in GUILDS.items() for cid in channels]
    places += [(None, cid) for cid in DMS] + [(None, cid) for cid in CLOSED_DMS]
    for seq in range(700):
        guild_id, channel_id = rng.choice(places)
        author_id = ME if rng.random() < 0.6 else rng.choice(list(FRIENDS))
        author = user(ME, "Me") if author_id == ME else user(author_id, FRIENDS[author_id])
        message_id = snowflake(now_ms - rng.randrange(two_years_ms), seq)
        content = " ".join(rng.choice(words) for _ in range(rng.randint(1, 8)))
        if rng.random() < 0.1:
            content += " https://example.com/" + rng.choice(words)
        attachments = []
        if rng.random() < 0.08:
            name = rng.choice(["cat.png", "clip.mp4", "notes.txt", "voice-message.ogg"])
            attachments.append({"id": message_id, "filename": name,
                                "url": f"{FILES_BASE}/{message_id}/{name}"})
        messages[message_id] = {
            "id": message_id,
            "channel_id": channel_id,
            "type": 7 if rng.random() < 0.01 else 0,
            "content": content,
            "author": author,
            "pinned": rng.random() < 0.03,
            "attachments": attachments,
            "embeds": [],
            "_guild": guild_id,
        }


def public(message):
    return {k: v for k, v in message.items() if not k.startswith("_")} | {"hit": True}


def has(message, kind):
    if kind == "link":
        return "http://" in message["content"] or "https://" in message["content"]
    if kind == "file":
        return bool(message["attachments"])
    extensions = {"image": (".png", ".jpg", ".gif"), "video": (".mp4", ".webm"), "sound": (".ogg", ".mp3")}
    return any(a["filename"].endswith(extensions.get(kind, ())) for a in message["attachments"])


def dm_json(channel_id, recipients):
    return {
        "id": channel_id,
        "type": 1 if len(recipients) == 1 else 3,
        "name": None,
        "icon": None,
        "last_message_id": channel_id,
        "recipients": [user(r, FRIENDS[r]) for r in recipients],
    }


def write_package(path):
    """A data package of my messages, in the newer JSON layout."""
    mine = [m for m in messages.values() if m["author"]["id"] == ME]
    # A message in a channel that no longer exists.
    now_ms = int(time.time() * 1000)
    mine.append({"id": snowflake(now_ms - 400 * 86_400_000, 9999), "channel_id": GONE_CHANNEL,
                 "content": "lost forever", "attachments": [], "_guild": "3001"})
    by_channel = {}
    for m in mine:
        by_channel.setdefault(m["channel_id"], []).append(m)
    index = {}
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as package:
        package.writestr("README.txt", "Fake Discord data package")
        package.writestr("Account/user.json", json.dumps({"id": ME}))
        for channel_id, items in by_channel.items():
            guild_id = items[0]["_guild"]
            if guild_id:
                guild_name, channels = GUILDS[guild_id]
                name = channels.get(channel_id, ("old-channel", None))[0]
                channel = {"id": channel_id, "type": "GUILD_TEXT", "name": name,
                           "guild": {"id": guild_id, "name": guild_name}}
                index[channel_id] = f"{name} in {guild_name}"
            else:
                recipients = DMS.get(channel_id) or CLOSED_DMS[channel_id]
                channel = {"id": channel_id, "type": "DM" if len(recipients) == 1 else "GROUP_DM",
                           "recipients": [ME, *recipients]}
                index[channel_id] = (f"Direct Message with {FRIENDS[recipients[0]]}#0"
                                     if len(recipients) == 1 else None)
            rows = [{"ID": int(m["id"]), "Timestamp": "", "Contents": m["content"],
                     "Attachments": " ".join(a["url"] for a in m["attachments"])} for m in items]
            package.writestr(f"Messages/c{channel_id}/channel.json", json.dumps(channel))
            package.writestr(f"Messages/c{channel_id}/messages.json", json.dumps(rows))
        package.writestr("Messages/index.json", json.dumps(index))
    return len(mine)


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        print(f"{self.command} {self.path} -> {args[1] if len(args) > 1 else ''}")

    def reply(self, status, body=None, headers=None):
        data = b"" if body is None else json.dumps(body).encode()
        self.send_response(status)
        for key, value in (headers or {}).items():
            self.send_header(key, value)
        if body is not None:
            self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def authorized(self):
        if self.headers.get("Authorization", "") in ("", "bad"):
            self.reply(401, {"message": "401: Unauthorized", "code": 0})
            return False
        return True

    def body(self):
        length = int(self.headers.get("Content-Length") or 0)
        return json.loads(self.rfile.read(length) or b"{}")

    def do_GET(self):
        # Attachments, like Discord's file servers: no token needed.
        match = re.fullmatch(r"/files/(\d+)/([^/]+)", urlparse(self.path).path)
        if match:
            data = f"contents of {match.group(2)}".encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        if not self.authorized():
            return
        url = urlparse(self.path)
        query = parse_qs(url.query)
        path = url.path
        if path == "/api/v9/users/@me":
            return self.reply(200, user(ME, "Me"))
        if path == "/api/v9/users/@me/guilds":
            return self.reply(200, [{"id": gid, "name": name, "icon": None} for gid, (name, _) in GUILDS.items()])
        if path == "/api/v9/users/@me/channels":
            with lock:
                return self.reply(200, [dm_json(cid, r) for cid, r in DMS.items()])
        if path == "/api/v9/users/@me/relationships":
            return self.reply(200, [{"id": uid, "type": 1, "user": user(uid, name)} for uid, name in FRIENDS.items()])
        match = re.fullmatch(r"/api/v9/guilds/(\d+)/channels", path)
        if match and match.group(1) in GUILDS:
            guild_id = match.group(1)
            channels = [{"id": cid, "type": 4, "name": name, "position": 0, "guild_id": guild_id}
                        for cid, name in CATEGORIES.get(guild_id, {}).items()]
            for position, (cid, (name, category)) in enumerate(GUILDS[guild_id][1].items()):
                parent = next((k for k, v in CATEGORIES.get(guild_id, {}).items() if v == category), None)
                channels.append({"id": cid, "type": 0, "name": name, "position": position,
                                 "parent_id": parent, "guild_id": guild_id})
            return self.reply(200, channels)
        match = re.fullmatch(r"/api/v9/channels/(\d+)(/pins)?", path)
        if match:
            return self.channel(match.group(1), bool(match.group(2)))
        match = re.fullmatch(r"/api/v9/(guilds|channels)/(\d+)/messages/search", path)
        if match:
            return self.search(match.group(1), match.group(2), query)
        self.reply(404, {"message": "404: Not Found", "code": 0})

    def channel(self, channel_id, pins):
        with lock:
            guild = next((gid for gid, (_, chans) in GUILDS.items() if channel_id in chans), None)
            dm = DMS.get(channel_id) or CLOSED_DMS.get(channel_id)
            if guild is None and dm is None:
                return self.reply(404, {"message": "Unknown Channel", "code": 10003})
            if pins:
                found = [public(m) for m in messages.values() if m["channel_id"] == channel_id and m["pinned"]]
                return self.reply(200, found)
            if dm:
                return self.reply(200, dm_json(channel_id, dm))
            name = GUILDS[guild][1][channel_id][0]
            return self.reply(200, {"id": channel_id, "type": 0, "name": name, "guild_id": guild})

    def search(self, scope, scope_id, query):
        time.sleep(0.2)
        first = {k: v[0] for k, v in query.items()}
        channels = query.get("channel_id", [])
        wanted = query.get("has", [])
        words = first.get("content", "").lower().split()
        with lock:
            found = [
                m for m in messages.values()
                if (m["_guild"] == scope_id if scope == "guilds" else m["channel_id"] == scope_id)
                and (not channels or m["channel_id"] in channels)
                and ("author_id" not in first or m["author"]["id"] == first["author_id"])
                and ("min_id" not in first or int(m["id"]) > int(first["min_id"]))
                and ("max_id" not in first or int(m["id"]) < int(first["max_id"]))
                # Like Discord: any of the words, so looser than EraseCord's own check.
                and (not words or any(w in m["content"].lower() for w in words))
                and (not wanted or any(has(m, kind) for kind in wanted))
            ]
        found.sort(key=lambda m: int(m["id"]), reverse=True)
        self.reply(200, {"total_results": len(found), "messages": [[public(m)] for m in found[:25]]})

    def do_POST(self):
        if not self.authorized():
            return
        if urlparse(self.path).path == "/api/v9/attachments/refresh-urls":
            urls = self.body().get("attachment_urls", [])
            return self.reply(200, {"refreshed_urls": [{"original": u, "refreshed": u} for u in urls]})
        if urlparse(self.path).path != "/api/v9/users/@me/channels":
            return self.reply(404, {"message": "404: Not Found", "code": 0})
        recipient = self.body().get("recipient_id")
        with lock:
            for cid, recipients in list(CLOSED_DMS.items()) + list(DMS.items()):
                if recipients == [recipient]:
                    DMS[cid] = CLOSED_DMS.pop(cid, recipients)
                    return self.reply(200, dm_json(cid, recipients))
        self.reply(400, {"message": "Unknown User", "code": 10013})

    def do_PATCH(self):
        if not self.authorized():
            return
        match = re.fullmatch(r"/api/v9/channels/(\d+)/messages/(\d+)", urlparse(self.path).path)
        if not match:
            return self.reply(404, {"message": "404: Not Found", "code": 0})
        body = self.body()
        with lock:
            message = messages.get(match.group(2))
            if message is None or message["channel_id"] != match.group(1):
                return self.reply(404, {"message": "Unknown Message", "code": 10008})
            message["content"] = body.get("content", message["content"])
            if "attachments" in body:
                message["attachments"] = []
            self.reply(200, public(message))

    def do_DELETE(self):
        if not self.authorized():
            return
        match = re.fullmatch(r"/api/v9/channels/(\d+)/messages/(\d+)", urlparse(self.path).path)
        if not match:
            return self.reply(404, {"message": "404: Not Found", "code": 0})
        channel_id, message_id = match.groups()
        with lock:
            counters["deletes"] += 1
            if counters["deletes"] % 25 == 0:
                return self.reply(429, {"message": "You are being rate limited.", "retry_after": 1.5, "global": False})
            message = messages.get(message_id)
            if message is None or message["channel_id"] != channel_id:
                return self.reply(404, {"message": "Unknown Message", "code": 10008})
            if message["type"] != 0:
                return self.reply(400, {"message": "Cannot execute action on a system message", "code": 50021})
            del messages[message_id]
        self.reply(204)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--write-package", metavar="PATH", help="also write a data package of your messages")
    args = parser.parse_args()
    global FILES_BASE
    FILES_BASE = f"http://127.0.0.1:{args.port}/files"
    seed(random.Random(args.seed))
    mine = sum(1 for m in messages.values() if m["author"]["id"] == ME)
    if args.write_package:
        count = write_package(args.write_package)
        print(f"Wrote a data package with {count} messages to {args.write_package}")
    print(f"Fake Discord on http://127.0.0.1:{args.port}/api/v9 with {len(messages)} messages ({mine} from you)")
    ThreadingHTTPServer(("127.0.0.1", args.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
