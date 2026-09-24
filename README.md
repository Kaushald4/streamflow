# Streamflow

I kept opening random movie sites and wondering the same thing: *how does this page actually get a playable URL?* Not the ads, not the UI: the stream. Half the time it was buried behind a decrypt step, a weird playlist hop, or a CDN that only answered if the `Referer` looked right.

So I started pulling those pieces apart. Small scripts at first. Then enough of them that I wanted a real shape around it: catalog on one side, pluggable extractors on the other, and a local process that could run the messy parts without me pasting URLs into curl forever.


> **Caution / what this project is not**
>
> I am **not** hosting movies, TV, or any media. I am **not** running a pirate streaming service. There is no CDN of mine, no catalog of files I uploaded, no “free Netflix” backend.
>
> This repo is about **curiosity**, figuring out how publicly reachable pages expose a playable link (decrypt steps, playlist hops, referer checks, WASM, the usual mess). Metadata comes from **TMDB**. Any actual stream bytes come from **whatever public source an adapter talks to on your machine**, if you choose to run that yourself.
>
> - Streamflow does **not** store or redistribute video content
> - Extraction and proxying only run **locally** via the companion; the web UI alone does not scrape or stream
> - Adapters are technical experiments against **publicly available** sources; they are not a license to ignore copyright
> - Use this only with content you are allowed to access. Misuse is on you, not this project
>
> If you came here looking for pirated libraries or a hosted free-stream site: wrong repo.

## What it is

A local-first playground for resolving **publicly available** stream links against a normal TMDB browse UI.

- **Web app** (Next.js): browse movies/TV via TMDB, pick something, hit play
- **Companion** (Rust, `127.0.0.1` only): runs installed adapters, proxies HLS/MP4 so the player can send the headers CDNs actually expect
- **Adapters**: small packages that know how one source works; install/uninstall like plugins

The UI is just a catalog shell. Nothing gets extracted or streamed unless *you* run the companion and install adapters on your own machine.

## Why I built it this way

Most "stream aggregator" demos hardcode one site and fall over the week the site rotates a key. I cared more about the *pattern*:

1. Find where the site exposes a URL (or ciphertext that becomes one)
2. Normalize it into something a player can eat
3. Keep site-specific crypto/WASM/referer quirks out of the UI

Adapters live in their own pack (`extractor/`). The companion loads them in QuickJS, with a few native hooks when pure JS isn't enough (AES-GCM, ALTCHA, WASM hosts, the stuff that kept breaking when I tried to fake it in a browser sandbox).

Playback goes through opaque session URLs on the companion so the player never has to know which CDN you're talking to, just that it needs a same-origin proxy that can attach `Referer` / `Origin` correctly, including the annoying cross-CDN segment cases.

## Layout

```
streamflow/          # this repo, UI + companion
  app/               # Next.js pages (browse, detail, watch)
  companion/         # Rust local server
  lib/               # TMDB client, companion API helpers, stream sessions

extractor/           # sibling package, adapter source + pack scripts
  src/extractors/    # one folder per source
```

If you're only cloning this folder, you'll need the extractor package next to it (or point `package.json` at wherever you keep adapters).

## Running it locally

You'll need Node 20+, pnpm, a TMDB API key, and Rust if you want the companion.

```bash
# UI
cd streamflow
cp .env.example .env   # add NEXT_PUBLIC_TMDB_API_KEY
pnpm install
pnpm dev               # http://localhost:3000

# Companion (separate terminal)
cd streamflow/companion
cargo run -p companion # http://127.0.0.1:4310
```

Pairing is automatic from the UI once the companion is up. Adapters are zip packages, pack them from the extractor repo (`pnpm pack-adapter <name>`) and upload them on the Plugins page.

## What's intentionally not here

Same idea as the caution above, in product terms: this is a **learning / reverse-engineering notebook with a UI**, not a streaming product.

- No hosted extraction farm, no media servers, no file mirrors
- No “unlimited free movies” branding, on purpose
- Sources break; adapters get rewritten; that’s part of the experiment
- I’m exploring *how* public pages wire up playback, not shipping content

There’s a longer write-up of the companion threat model in [`SECURITY.md`](./SECURITY.md) if you care about why the pairing token exists, why `/api/stream` isn’t an open relay, and what SSRF filtering actually covers.

## Stack, briefly

| Piece | Choice | Why |
|---|---|---|
| Catalog UI | Next.js + TMDB | Familiar browse surface so I could focus on extraction |
| Local runtime | Rust companion + QuickJS | Fast startup, real HTTP, room for WASM/native crypto |
| Adapters | Packed JS zips | Swap sources without redeploying the app |
| Player | PlayerJS over proxied sessions | HLS/MP4 with server-side headers |

## Status

Personal project. APIs and adapters move when sites move. If something here is useful for your own experiments, cool. If you're looking for a polished streaming service, this isn't that.
