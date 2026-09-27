#!/usr/bin/env python3
"""Writes a large, made-up Discord data package for testing speed and charts.

    python3 tools/big_package.py big.zip --messages 1000000

Messages follow daily and weekly rhythms, some conversations fade out and
others start later, and the text has words, emoji, mentions, links and
attachments in realistic proportions. Nothing in it is real.
"""

import argparse
import json
import math
import random
import zipfile
from datetime import datetime, timedelta, timezone

EPOCH_MS = 1_420_070_400_000
OWNER = 900000000000000000

WORDS = (
    "the i you to a it and is that of in for this lol on my me be not have so was just with but "
    "what do we are no yes ok like can it's all get if at your go know one up think now how he "
    "there good they time out when about would haha really then it's gonna see why yeah did got "
    "want more game play server tonight tomorrow anyone voice join stream bug fix build rust code "
    "release test music song movie party pizza sleep work school exam cat dog photo meme gg wp "
    "nice cool thanks sorry wait what's today later maybe sure right also still need new"
).split()
EMOJI = ["😂", "❤️", "👍", "😭", "🔥", "😅", "🙏", "👀", "✨", "🎉"]
DOMAINS = ["youtube.com", "github.com", "tenor.com", "reddit.com", "twitter.com", "example.org"]
FILES = ["png", "png", "jpg", "gif", "mp4", "pdf", "txt"]


def snowflake(ms, rnd):
    return ((ms - EPOCH_MS) << 22) | rnd.randrange(1 << 22)


def zipf_word(rnd):
    index = int(math.exp(rnd.random() * math.log(len(WORDS) + 1))) - 1
    return WORDS[min(index, len(WORDS) - 1)]


def text(rnd, people):
    words = [zipf_word(rnd) for _ in range(max(1, int(rnd.expovariate(1 / 7))))]
    if rnd.random() < 0.08:
        words.insert(rnd.randrange(len(words) + 1), f"<@{rnd.choice(people)}>")
    if rnd.random() < 0.12:
        words.append(rnd.choice(EMOJI))
    if rnd.random() < 0.02:
        words.append(f"<:blob{rnd.randrange(5)}:{880000000000000000 + rnd.randrange(5)}>")
    if rnd.random() < 0.04:
        words.append(f"https://{rnd.choice(DOMAINS)}/{rnd.randrange(10**6)}")
    sentence = " ".join(words)
    return sentence[0].upper() + sentence[1:] if rnd.random() < 0.3 else sentence


def moment(rnd, start, end):
    """A time between start and end, evenings and weekends preferred."""
    while True:
        t = start + (end - start) * rnd.random()
        local = t + timedelta(hours=1)
        weight = 0.15 + (0.85 if 17 <= local.hour <= 23 else 0.4 if 9 <= local.hour < 17 else 0.2 if local.hour < 2 else 0.03)
        weight *= 1.4 if local.weekday() >= 5 else 1
        if rnd.random() < weight / 1.4:
            return t


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("path")
    parser.add_argument("--messages", type=int, default=100_000)
    parser.add_argument("--seed", type=int, default=1)
    args = parser.parse_args()
    rnd = random.Random(args.seed)

    now = datetime.now(timezone.utc)
    begin = now - timedelta(days=6 * 365)
    people = [100000000000000000 + i * 7919 for i in range(60)]
    servers = [(200000000000000000 + i * 104729, f"Server {i + 1}") for i in range(25)]
    channels = []
    for server_id, server_name in servers:
        for c in range(rnd.randrange(2, 9)):
            channels.append({"id": str(300000000000000000 + len(channels) * 15485863), "type": 0,
                             "name": f"channel-{c + 1}", "guild": {"id": str(server_id), "name": server_name}})
    for i, person in enumerate(people[:40]):
        channels.append({"id": str(400000000000000000 + i * 32452843), "type": 1,
                         "recipients": [str(OWNER), str(person)], "_name": f"friend{i + 1}#0"})
    for i in range(6):
        channels.append({"id": str(500000000000000000 + i * 49979687), "type": 3,
                         "recipients": [str(OWNER)] + [str(p) for p in rnd.sample(people, 4)]})

    # A few busy places, many quiet ones; each active for a part of the time.
    weights = [1 / (i + 1) ** 0.9 for i in range(len(channels))]
    rnd.shuffle(weights)
    spans = []
    for _ in channels:
        a = begin + (now - begin) * rnd.random() * 0.7
        spans.append((a, a + (now - a) * (0.3 + 0.7 * rnd.random())))
    counts = [0] * len(channels)
    total = sum(weights)
    for i, w in enumerate(weights):
        counts[i] = int(args.messages * w / total)
    counts[0] += args.messages - sum(counts)

    with zipfile.ZipFile(args.path, "w", zipfile.ZIP_DEFLATED) as package:
        package.writestr("Account/user.json", json.dumps({"id": str(OWNER), "username": "tester"}))
        index = {}
        for channel, count, (start, end) in zip(channels, counts, spans):
            messages = []
            for _ in range(count):
                t = moment(rnd, start, end)
                ms = int(t.timestamp() * 1000)
                attachment = ""
                if rnd.random() < 0.06:
                    attachment = (f"https://cdn.discordapp.com/attachments/{channel['id']}/"
                                  f"{snowflake(ms, rnd)}/file.{rnd.choice(FILES)}")
                messages.append({
                    "ID": snowflake(ms, rnd),
                    "Timestamp": t.strftime("%Y-%m-%d %H:%M:%S"),
                    "Contents": "" if attachment and rnd.random() < 0.5 else text(rnd, people),
                    "Attachments": attachment,
                })
            messages.sort(key=lambda m: m["ID"], reverse=True)
            name = channel.pop("_name", None)
            folder = f"Messages/c{channel['id']}"
            package.writestr(f"{folder}/channel.json", json.dumps(channel))
            package.writestr(f"{folder}/messages.json", json.dumps(messages))
            index[channel["id"]] = (f"Direct Message with {name}" if name else
                                    f"{channel['name']} in {channel['guild']['name']}" if "guild" in channel else None)
        package.writestr("Messages/index.json", json.dumps(index))
    print(f"Wrote {args.messages} messages in {len(channels)} channels to {args.path}")


if __name__ == "__main__":
    main()
