# moviemail
> Get an email when a new movie by a favourite director is released.

### Summary
- reads `~/.config/moviemail/config.toml` to load
  - archive path
  - tmdb api key
  - email config
  - list of directors
- fetches all movies of configured directors from tmdb
- filters out previously archived movies (from `~/.config/moviemail/archive.json`)
- filters out placeholder titles, short films and films missing imdb id
- sends email with links to imdb entries

### Install
- `git clone https://github.com/lucashadfield/moviemail.git`
- `cd moviemail`
- `cargo install --path .`

### Usage
- `moviemail`

### Config
- `cp ~/.config/moviemail/example_config.toml ~/.config/moviemail/config.toml`
- edit `~/.config/moviemail/config.toml` to add your tmdb api key and email config and directors

### Build Docker Image
- `docker build -t moviemail .`
