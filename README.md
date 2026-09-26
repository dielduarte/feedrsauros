<p align="center">
  <img src="assets/logo.svg" alt="The feedrsauros dinosaur reading its feeds" width="160">
</p>

# feedrsauros

A local-first RSS reader: one small program that fetches your feeds in the background and serves a clean web app to read them.

Website: [feedrsauros.com](https://feedrsauros.com)

- Follows RSS, Atom and JSON Feed. Paste a site's address and feedrsauros finds its feed.
- Organise feeds into folders by dragging them in the sidebar.
- Checks busy feeds more often and quiet ones less, and backs off politely when sites ask it to.
- Updates the page live as new articles arrive.
- Imports and exports OPML, so you can move from or to any other reader.

## Run it on your computer

Requires [Rust](https://rustup.rs) and [pnpm](https://pnpm.io).

```bash
pnpm --dir web install && pnpm --dir web build
cargo install --path .
feedrsauros serve --open
```

This opens http://127.0.0.1:7777. feedrsauros fetches feeds while `feedrsauros serve` is running and catches up when you start it again. Your data is stored in your user data directory (on macOS, `~/Library/Application Support/feedrsauros/feedrsauros.db`); pass `--db` to use another file.

Everything also works from the terminal:

```bash
feedrsauros add jvns.ca --folder Blogs   # subscribe to a site or feed
feedrsauros ls                           # folders, feeds and unread counts
feedrsauros refresh                      # fetch every feed now
feedrsauros import subscriptions.opml
feedrsauros export > subscriptions.opml
```

## Self-host with Docker

```bash
docker compose up -d
```

Then open http://127.0.0.1:7777. Articles and subscriptions are kept in the `feedrsauros-data` volume, so they survive restarts and upgrades. To upgrade, pull the latest code and run `docker compose up -d --build`.

Run CLI commands inside the container with `docker compose exec`:

```bash
docker compose exec feedrsauros feedrsauros add jvns.ca --folder Blogs
docker compose exec feedrsauros feedrsauros export > subscriptions.opml
```

### Reaching it from other devices

feedrsauros has no login: anyone who can reach it can read and change your subscriptions. That is why `compose.yaml` only publishes it on `127.0.0.1`. To use it from your phone or another computer, put it behind a reverse proxy that adds authentication and HTTPS. With [Caddy](https://caddyserver.com), for example:

```caddy
feeds.example.com {
	basic_auth {
		# Generate the hash with: caddy hash-password
		you $2a$14$replace-with-your-password-hash
	}
	reverse_proxy 127.0.0.1:7777
}
```

A VPN such as Tailscale works too: keep the port private and reach the machine over the VPN.

### Configuration

| Variable | Default | What it does |
| --- | --- | --- |
| `FEEDRSAUROS_PORT` | `7777` | Port on the host (in `compose.yaml`) or the port `feedrsauros serve` listens on. |
| `FEEDRSAUROS_HOST` | `127.0.0.1` (`0.0.0.0` in the container) | Address `feedrsauros serve` listens on. |
| `FEEDRSAUROS_DB` | your data directory (`/data/feedrsauros.db` in the container) | Database file. |
| `RUST_LOG` | `feedrsauros=info` | Log detail, for example `feedrsauros=debug`. |

For example, `FEEDRSAUROS_PORT=8080 docker compose up -d` serves feedrsauros on http://127.0.0.1:8080.

## Development

How the pieces fit together (backend, web app, desktop app, data model, filters, releases) is in [docs/architecture.md](docs/architecture.md).

```bash
cargo test                    # backend tests
pnpm --dir web test           # frontend unit tests
feedrsauros serve                   # API on :7777 …
pnpm --dir web dev            # … and the web app with hot reload, proxying /api to it
```

### Desktop app

The desktop app (in `desktop/`, built with [Tauri](https://tauri.app)) runs the same server and web app inside a native window. It uses the same database as the CLI, so feeds you add from the terminal show up in it, and it only polls while its window is focused.

```bash
pnpm --dir web build                  # the window shows the built web app
cargo run -p feedrsauros-desktop      # FEEDRSAUROS_DB=… to point it at another database
```

`cargo run` shows up in the Dock as `feedrsauros-desktop`. To get a real app named feedrsauros, bundle it:

```bash
cd desktop && pnpm dlx @tauri-apps/cli build --bundles app
open ../target/release/bundle/macos/feedrsauros.app
```
