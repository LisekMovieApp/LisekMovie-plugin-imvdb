# LisekMovie-plugin-imvdb

IMVDb metadata provider extension for [LisekMovie](https://github.com/LisekMovieApp/LisekMovie-app), the self-hosted video-on-demand server.

| | |
|---|---|
| Provider key | `imvdb` |
| Covers | Music videos (in movie libraries) |
| Status | Basic — works, parts still missing |
| API key | Required — free key from https://imvdb.com/developers (`IMVDB-APP-KEY` header) |
| Fix match ids | `imvdb:121779770452`, or the number with IMVDb picked |

## What it does

IMVDb API: video search, lookup by id with artists, year, directors and stills. Titles read `Artist - Song`; automatic matching only takes a video whose `Artist - Song` equals the file name, so feature films are never matched as music videos.

## How it plugs in

The crate implements `lisek_plugin_api::MetadataProvider` and is compiled into `lisek-server` as a Cargo path dependency — there is no runtime plugin loading. Clone it **next to** `LisekMovie-app`:

```
LisekMovie/
  LisekMovie-app/
  LisekMovie-plugin-imvdb/
  ...
```

The server lists the provider under **Admin → Plugins** (disabled by default). Turn it on, add the key if it needs one, and set its priority — globally, or per library (for example AniList or aniDB first in an anime library). `install-linux.sh` and `update-linux.sh` clone and update it with the other modules.

## Development

```bash
cargo clippy --all-targets -- -D warnings
cargo test
```

Tests run against a local `wiremock` server; nothing calls the real service.

## License

[PolyForm Noncommercial 1.0.0](https://github.com/LisekMovieApp/LisekMovie-app/blob/main/LICENSE), as LisekMovie itself.
