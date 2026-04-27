#!/usr/bin/env python3
"""
HN keyword monitor — checks top/new stories for relevant posts.
Run periodically: python3 scripts/hn-monitor.py
"""

import json
import urllib.request
import datetime
import sys

KEYWORDS = [
    "rust", "hypervisor", "arm64", "aarch64", "bare metal", "bare-metal",
    "virtualization", "pkvm", "trustzone", "confidential computing",
    "secure enclave", "tee ", "arm cca", "hafnium", "gunyah",
    "no_std", "embedded rust", "type-1", "stage-2", "ff-a", "ffa",
    "spmc", "android virtualization", "microvm", "firecracker",
    "kvm", "qemu", "firmware", "uefi", "bootloader",
]

# Higher weight keywords — these alone are worth commenting on
HIGH_VALUE = ["hypervisor", "pkvm", "hafnium", "bare metal", "bare-metal",
              "no_std", "arm64", "aarch64", "trustzone", "confidential computing",
              "spmc", "microvm", "stage-2"]

SEEN_FILE = "/tmp/hn-monitor-seen.json"


def load_seen():
    try:
        with open(SEEN_FILE) as f:
            return set(json.load(f))
    except (FileNotFoundError, json.JSONDecodeError):
        return set()


def save_seen(seen):
    with open(SEEN_FILE, "w") as f:
        json.dump(list(seen), f)


def fetch_json(url):
    return json.loads(urllib.request.urlopen(url, timeout=10).read())


def check_stories():
    seen = load_seen()
    new_matches = []

    # Check top 200 + new 200
    for endpoint in ["topstories", "newstories"]:
        try:
            ids = fetch_json(f"https://hacker-news.firebaseio.com/v0/{endpoint}.json")[:200]
        except Exception as e:
            print(f"Error fetching {endpoint}: {e}", file=sys.stderr)
            continue

        for item_id in ids:
            if item_id in seen:
                continue
            try:
                item = fetch_json(f"https://hacker-news.firebaseio.com/v0/item/{item_id}.json")
            except Exception:
                continue

            title = (item.get("title") or "").lower()
            matched = [k for k in KEYWORDS if k in title]

            if matched:
                score = item.get("score", 0)
                comments = item.get("descendants", 0)
                ts = datetime.datetime.fromtimestamp(item["time"]).strftime("%Y-%m-%d %H:%M")
                high = any(k in title for k in HIGH_VALUE)

                new_matches.append({
                    "id": item_id,
                    "title": item.get("title"),
                    "score": score,
                    "comments": comments,
                    "time": ts,
                    "keywords": matched,
                    "high_value": high,
                    "url": f"https://news.ycombinator.com/item?id={item_id}",
                })

            seen.add(item_id)

    save_seen(seen)

    # Sort by relevance: high_value first, then by score
    new_matches.sort(key=lambda x: (x["high_value"], x["score"]), reverse=True)
    return new_matches


def main():
    matches = check_stories()

    if not matches:
        print(f"[{datetime.datetime.now():%Y-%m-%d %H:%M}] No new matches.")
        return

    print(f"[{datetime.datetime.now():%Y-%m-%d %H:%M}] {len(matches)} new match(es):\n")
    for m in matches:
        tag = "🔥 HIGH" if m["high_value"] else "   low"
        print(f"  {tag} | {m['score']}p/{m['comments']}c | {m['time']}")
        print(f"       {m['title']}")
        print(f"       {m['url']}")
        print(f"       keywords: {', '.join(m['keywords'])}")
        print()


if __name__ == "__main__":
    main()
