#!/usr/bin/env python3
"""A fake Discord API for trying purgecord without a real account.

    python3 tools/fake_discord.py            # listens on http://127.0.0.1:8765
    PURGECORD_API_BASE=http://127.0.0.1:8765/api/v9 DISCORD_TOKEN=anything purgecord list

It serves two servers and three DMs filled with messages from the last two
years, answers the search and delete endpoints purgecord uses, and now and
then replies with 429 so rate-limit handling can be watched. The token
"bad" is rejected with 401. Only the Python standard library is needed.
"""

import argparse
import json
import random
import re
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

DISCORD_EPOCH_MS = 1_420_070_400_000
ME = "1000"
FRIENDS = {"2001": "Alice", "2002": "Bob", "2003": "Charlie"}
GUILDS = {"3001": ("Rust Enjoyers", ["3101", "3102"]), "3002": ("Gaming Night", ["3201"])}
DMS = {"4001": ["2001"], "4002": ["2002"], "4003": ["2001", "2002", "2003"]}

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
    words = "hello ok lol nice see you tomorrow what about this link here gg thanks".split()
    places = [(gid, cid) for gid, (_, cids) in GUILDS.items() for cid in cids]
    places += [(None, cid) for cid in DMS]
    for seq in range(600):
        guild_id, channel_id = rng.choice(places)
        author_id = ME if rng.random() < 0.6 else rng.choice(list(FRIENDS))
        author = user(ME, "Me") if author_id == ME else user(author_id, FRIENDS[author_id])
        message_id = snowflake(now_ms - rng.randrange(two_years_ms), seq)
        messages[message_id] = {
            "id": message_id,
            "channel_id": channel_id,
            "type": 7 if rng.random() < 0.01 else 0,
            "content": " ".join(rng.choice(words) for _ in range(rng.randint(1, 8))),
            "author": author,
            "pinned": rng.random() < 0.03,
            "attachments": [],
            "_guild": guild_id,
        }


def public(message):
    return {k: v for k, v in message.items() if not k.startswith("_")} | {"hit": True}


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

    def do_GET(self):
        if not self.authorized():
            return
        url = urlparse(self.path)
        query = {k: v[0] for k, v in parse_qs(url.query).items()}
        if url.path == "/api/v9/users/@me":
            return self.reply(200, user(ME, "Me"))
        if url.path == "/api/v9/users/@me/guilds":
            return self.reply(200, [{"id": gid, "name": name, "icon": None} for gid, (name, _) in GUILDS.items()])
        if url.path == "/api/v9/users/@me/channels":
            channels = []
            for cid, recipients in DMS.items():
                channels.append({
                    "id": cid,
                    "type": 1 if len(recipients) == 1 else 3,
                    "name": None,
                    "icon": None,
                    "last_message_id": cid,
                    "recipients": [user(r, FRIENDS[r]) for r in recipients],
                })
            return self.reply(200, channels)
        match = re.fullmatch(r"/api/v9/(guilds|channels)/(\d+)/messages/search", url.path)
        if match:
            return self.search(match.group(1), match.group(2), query)
        self.reply(404, {"message": "404: Not Found", "code": 0})

    def search(self, scope, scope_id, query):
        time.sleep(0.2)
        with lock:
            found = [
                m for m in messages.values()
                if (m["_guild"] == scope_id if scope == "guilds" else m["channel_id"] == scope_id)
                and ("author_id" not in query or m["author"]["id"] == query["author_id"])
                and ("min_id" not in query or int(m["id"]) > int(query["min_id"]))
                and ("max_id" not in query or int(m["id"]) < int(query["max_id"]))
            ]
        found.sort(key=lambda m: int(m["id"]), reverse=True)
        self.reply(200, {"total_results": len(found), "messages": [[public(m)] for m in found[:25]]})

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
    args = parser.parse_args()
    seed(random.Random(args.seed))
    mine = sum(1 for m in messages.values() if m["author"]["id"] == ME)
    print(f"Fake Discord on http://127.0.0.1:{args.port}/api/v9 with {len(messages)} messages ({mine} from you)")
    ThreadingHTTPServer(("127.0.0.1", args.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
