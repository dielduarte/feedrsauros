# feedrsauros architecture

A local-first RSS reader. One Rust program owns the data and does all the work; everything you look at is a React app talking to it over HTTP.

## The big picture

```mermaid
flowchart TB
  subgraph clients["Ways to use it"]
    Browser["Browser<br/>(web app)"]
    Desktop["Desktop app<br/>(Tauri window)"]
    CLI["Terminal / AI agent<br/>(feedrsauros add, ls, refresh…)"]
  end

  subgraph core["feedrsauros (one Rust crate)"]
    Server["server::start<br/>axum HTTP API + SSE"]
    Poller["Poller<br/>fetches feeds in the background"]
    Logic["add_feed · filter · jev · opml"]
    DB[("SQLite<br/>feedrsauros.db")]
  end

  Sites["The sites you follow<br/>(RSS / Atom / JSON Feed)"]
  TypeSafe["typesafe.ai<br/>(only with AI features on)"]

  Browser -- "HTTP /api + SSE /api/events" --> Server
  Desktop -- "same, on 127.0.0.1:random port" --> Server
  CLI --> Logic
  Server --> Logic
  Poller --> Logic
  Logic --> DB
  Poller -- "HTTP" --> Sites
  Logic -- "folder picks, filters" --> TypeSafe
```

- **One backend, many front doors.** The browser, the desktop app and the CLI all run the same Rust code against the same SQLite file.
- **The web app is embedded** in the Rust binary (`web/dist` via `rust-embed`), so `feedrsauros serve` serves both the API and the UI.
- **Nothing leaves your machine** except fetching feeds, favicons, and, only if you turn AI features on, one request per judgment to typesafe.ai.

## Ways it runs

```mermaid
flowchart LR
  subgraph modes[" "]
    direction TB
    A["feedrsauros serve<br/><i>long-running: API + UI + poller</i>"]
    B["Desktop app<br/><i>same server inside a native window,<br/>polls only while focused</i>"]
    C["Docker<br/><i>serve in a container, data in /data</i>"]
    D["feedrsauros add / ls / refresh / import / export<br/><i>one-shot, then exits</i>"]
  end
  DB[("feedrsauros.db<br/>(shared)")]
  A --> DB
  B --> DB
  C --> DBV[("volume: /data/feedrsauros.db")]
  D --> DB
```

| Mode | Polls feeds? | Notes |
|---|---|---|
| `feedrsauros serve` | Yes, always | For the browser and self-hosting. `--host`, `--port`, `--open`. |
| Desktop app | Only while its window is focused | Starts the server on a random localhost port. |
| Docker | Yes | `docker compose up -d`, port `127.0.0.1:7777`. |
| CLI commands | No | Do one thing and exit. |

**Running several at once is safe:**
- Only one process polls a database file at a time, thanks to a lock file (`feedrsauros.db.poller.lock`).
- Each server checks SQLite's `data_version` every second. When another process has written, it tells its pages to reload.
- So a feed added from the CLI shows up in the open desktop app.

## Backend modules

```mermaid
flowchart TB
  main["main.rs"] --> cli["cli.rs<br/>commands"]
  cli --> server["server.rs<br/>start / shutdown"]
  server --> api["api/*<br/>HTTP handlers"]
  server --> poller["poller.rs<br/>schedule · batches · SSE events"]

  api --> add_feed["add_feed.rs<br/>find the feed, subscribe"]
  api --> filter["filter.rs<br/>apply a feed's filters"]
  poller --> filter
  add_feed --> jev["jev.rs<br/>typesafe.ai client"]
  filter --> jev

  add_feed --> discover["discover.rs<br/>find feed links in a page"]
  add_feed --> fetch["fetch.rs<br/>HTTP, redirects, ETags"]
  poller --> fetch
  fetch --> parse["parse.rs + sanitize.rs<br/>feed-rs, ammonia"]
  poller --> schedule["schedule.rs<br/>adaptive intervals, backoff"]

  api --> db["db/*<br/>sqlx queries"]
  poller --> db
  filter --> db
  db --> secrets["secrets.rs<br/>encrypts the API key"]
```

| Module | Job |
|---|---|
| `cli.rs` | Command-line parsing; picks the database path (platform data directory, or `--db` / `FEEDRSAUROS_DB`). |
| `server.rs` | Starts the API and poller together; shuts both down cleanly. Shared by `serve` and the desktop app. |
| `api/` | One file per resource: `feeds`, `folders`, `items`, `filters`, `settings`, `opml`, `events` (SSE), `web` (serves the UI). |
| `poller.rs` | Decides which feeds are due, fetches them in batches, stores results, broadcasts events. |
| `schedule.rs` | How often to check each feed: often for busy sites, rarely for quiet ones, backing off on errors. |
| `fetch.rs` | HTTP with conditional requests (ETag / Last-Modified), manual redirects, timeouts. |
| `parse.rs`, `sanitize.rs` | Turn RSS/Atom/JSON Feed into articles; clean their HTML. |
| `add_feed.rs`, `discover.rs` | Turn "a website address" into a subscription. |
| `filter.rs` | Filters: what a feed's articles should look like, and hiding/showing them. |
| `jev.rs` | The only code that talks to typesafe.ai. |
| `secrets.rs` | ChaCha20-Poly1305 encryption for the API key. |
| `db/` | All SQL, checked against the schema at compile time (`sqlx::query!`). |

## Data model

```mermaid
erDiagram
  folders ||--o{ feeds : contains
  feeds ||--o{ items : publishes

  folders {
    int id PK
    text slug UK "used in URLs"
    text name UK
    int position
  }
  feeds {
    int id PK
    text slug UK "used in URLs"
    int folder_id FK "null = outside any folder"
    int position
    text url UK
    text title
    text custom_title
    text etag "conditional requests"
    int next_fetch_at "adaptive schedule"
    int error_count
    text filter_wanted "what the reader wants to see"
    text filter_unwanted "what they don't"
  }
  items {
    int id PK
    int feed_id FK
    text slug "unique within the feed"
    text guid "unique within the feed"
    text title
    text content_html "sanitized"
    int published_at
    int fetched_at
    int read_at
    int starred_at
    int hidden_at "set while filters keep it out"
  }
  settings {
    int id PK "always 1"
    bool ai_enabled "only with a key (CHECK)"
    blob typesafe_api_key "encrypted"
    text typesafe_api_key_hint "last 4 chars"
  }
```

- **Slugs in URLs, integer ids internally.** Integer ids never appear in the API's JSON.
- **Hidden, never deleted:** filtered-out articles keep their row with `hidden_at` set. Lists, unread counts and mark-all-read skip them.
- **The database enforces the rules it can:** AI can't be on without a key, and a blank filter text is stored as "no filter".
- **Next to the database:** the encryption key (`feedrsauros.db.key`, readable only by you) and the poller lock.

## How new articles arrive

```mermaid
sequenceDiagram
  participant P as Poller
  participant S as Site
  participant J as typesafe.ai
  participant D as SQLite
  participant U as Open pages (SSE)

  P->>D: which feeds are due?
  P->>U: batch_started
  loop each due feed (8 at a time, 2 per host)
    P->>S: GET feed (If-None-Match / If-Modified-Since)
    S-->>P: 304 Not Modified, or new XML
    opt feed has filters and AI is on
      loop each article not seen before
        P->>J: does it fit "wanted"? does it fit "unwanted"?
        J-->>P: yes/no probabilities
      end
    end
    P->>D: store articles, hide the rejected ones, schedule next check
    P->>U: feed_refreshed
  end
  P->>U: batch_finished (online / offline)
```

- **Offline detection:** if nothing answers, including a feed known to be healthy, the problem is your connection. Failures are postponed instead of counted against the feeds.
- **Each article is judged once, on arrival.** If typesafe.ai can't be reached, the article is kept.

## Adding a site

```mermaid
sequenceDiagram
  participant UI as Web app
  participant A as API
  participant S as Site
  participant J as typesafe.ai
  participant D as SQLite

  UI->>UI: close dialog, show placeholder row + dino
  UI->>A: POST /api/feeds { url, folder }
  A->>S: fetch the page
  alt page is a feed
    S-->>A: feed
  else page is HTML
    A->>S: try linked feeds, then common paths (/feed, /rss.xml…)
  end
  opt no folder chosen, AI on, and you have folders
    A->>J: which folder fits? (one Choice + "none")
    J-->>A: pick + confidence
    Note over A: used only when confidence ≥ 0.5
  end
  A->>D: subscribe with its first articles
  A-->>UI: { slug, title, ai_folder }
  UI->>UI: placeholder becomes the real feed
```

## Filters

Each feed can have two plain-language texts: **What do you want to see?** and **What don't you want to see?**

```mermaid
flowchart LR
  art["An article"] --> q1{"'wanted' set?"}
  q1 -- "no" --> q2
  q1 -- "yes" --> w{"fits what<br/>you want?"}
  w -- "no" --> hide["hidden"]
  w -- "yes" --> q2{"'unwanted' set?"}
  q2 -- "no" --> keep["kept"]
  q2 -- "yes" --> u{"fits what you<br/>don't want?"}
  u -- "yes" --> hide
  u -- "no" --> keep
```

- **One request per article**, with one yes/no question per filled-in text.
- **Changing a feed's filters** re-judges every stored article in it, hidden ones too, four at a time in the background. Whatever the new verdict differs for is hidden or brought back.
- **Clearing both texts** brings everything back without any requests.
- **Starred articles** are never hidden.
- **When it's done**, a `feed_filtered` event tells open pages to reload.

## Live updates (SSE)

```mermaid
flowchart LR
  subgraph server["Server"]
    poller["Poller"] -- broadcast --> ch(("event channel"))
    watcher["data_version watcher"] -- resync --> ch
    filters["Filter re-check"] -- feed_filtered --> ch
  end
  ch -- "GET /api/events" --> page["Web app<br/>applyEvent()"]
  page --> cache["TanStack Query cache<br/>(invalidate / update)"]
```

| Event | The web app… |
|---|---|
| `batch_started` / `batch_finished` | shows or stops the refresh spinner; reloads lists when done |
| `feed_refreshed` / `feed_failed` | reloads the sidebar counts |
| `feed_filtered` | reloads the sidebar and lists |
| `resync` | reloads everything (another process wrote, or the page fell behind) |

## Frontend

React 19 + TypeScript, built with Vite, styled with Tailwind + shadcn/ui.

```mermaid
flowchart TB
  main["main.tsx<br/>QueryClient, providers"] --> shell["App.tsx · Shell<br/>URL → which page, shared state"]
  shell --> sidebar["AppSidebar"]
  shell --> pages["Pages<br/>List · Reader · Settings · Filters"]
  pages --> components["Components<br/>TopBar · ArticleList · Reader · dialogs"]
  components --> ui["components/ui<br/>(shadcn primitives)"]
  shell & pages & sidebar --> data["queries.ts · poller.ts · api.ts<br/>TanStack Query + SSE + fetch"]
  data --> logic["routes · lookup · pending · navigation · format<br/>(plain TS, unit tested)"]
```

- **Server data lives in TanStack Query:** the sidebar, lists, an article, settings, filters, the poller status, and even localStorage preferences.
- **Mutations update the page immediately and roll back on error:** starring, reading, adding a feed.
- **Routes:**

  | Path | Page |
  |---|---|
  | `/` | All articles |
  | `/unread`, `/starred` | Unread, Starred |
  | `/folders/:folder` | A folder |
  | `/feeds/:feed` | A feed |
  | `/feeds/:feed/:article` | An open article |
  | `/filters/:feed` | A feed's filters |
  | `/settings` | Settings |

## HTTP API

| Method & path | What it does |
|---|---|
| `GET /api/sidebar` | Folders, feeds, unread and starred counts |
| `POST /api/folders` · `PATCH/DELETE /api/folders/:slug` · `PUT …/position` | Create, rename, delete, reorder folders |
| `POST /api/feeds` · `DELETE /api/feeds/:slug` | Subscribe (with AI filing), unsubscribe |
| `PUT /api/feeds/:slug/position` · `PUT …/title` | Move, rename a feed |
| `GET/PUT /api/feeds/:slug/filters` | Read or change a feed's filters |
| `GET /api/items?feed=&folder=&starred=&unread=&cursor=` | Article lists, paged |
| `GET/PATCH /api/feeds/:feed/items/:item` | Open an article; mark read or starred |
| `POST /api/items/mark-read` | Mark everything seen so far as read |
| `POST /api/refresh` | Fetch now (all, a folder, or a feed) |
| `GET/POST /api/opml` | Export or import subscriptions |
| `GET /api/settings` · `PUT/DELETE …/api-key` · `PUT …/ai` | AI settings; the key never comes back, only a hint |
| `GET /api/events` | Server-sent events |

## Security and privacy

- **No login.** It listens on `127.0.0.1` by default. To reach it from other devices, put it behind a VPN or an authenticating reverse proxy.
- **API key:** encrypted with ChaCha20-Poly1305 under a key file kept *next to* the database. A copy of the database alone doesn't reveal the key.
- **Article HTML** is sanitized with `ammonia` before it's stored.
- **Desktop app:** links to other sites open in your normal browser. The page may only drag and zoom the window, nothing else native.

## Repository and delivery

```mermaid
flowchart LR
  subgraph repo["Repository"]
    src["src/ + migrations/<br/>Rust crate"]
    web["web/<br/>React app"]
    desktop["desktop/<br/>Tauri app"]
    website["website/<br/>landing page"]
  end

  web -- "vite build → web/dist (embedded)" --> src
  src --> desktop
  src --> docker["Dockerfile<br/>node → rust → debian-slim"]

  subgraph ci["GitHub Actions (on every PR and push to main)"]
    tests["Tests: web type check, lint, unit tests;<br/>cargo test"]
    release["macOS release<br/>(push to main, app files changed)<br/>universal .dmg → GitHub Release v0.1.N"]
    tests --> release
  end

  website -- "pnpm run deploy (SST → Cloudflare)" --> site["feedrsauros.com"]
```

- **Releases:** every push to `main` that changes the app builds a universal (Apple Silicon + Intel) `.dmg` and publishes `v<major.minor>.<run number>`, attached as `feedrsauros.dmg`.
