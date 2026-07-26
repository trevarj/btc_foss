# Agent Notes

## Project

- This repo generates a static GitHub Pages site for Bitcoin FOSS contribution activity.
- Source of truth is mostly [src/main.rs](/home/trev/Workspace/rust-projects/btc_foss/src/main.rs).
- Generated output lives under `public/` and is ignored. Do not treat generated files as source changes.
- Config is [config/site.toml](/home/trev/Workspace/rust-projects/btc_foss/config/site.toml).
- Fixture data is [fixtures/feed.json](/home/trev/Workspace/rust-projects/btc_foss/fixtures/feed.json).
- Deployment workflow is [.github/workflows/pages.yml](/home/trev/Workspace/rust-projects/btc_foss/.github/workflows/pages.yml).
- The intended contribution window is the last 24 months. `--full` should rebuild that configured window from scratch, not scan all history.
- Collection windows longer than one year must be split before calling GitHub `contributionsCollection`; GitHub rejects spans over one year.

## Environment

- User runs GNU Guix System. Do not use `guix install`.
- Prefer `guix shell <packages> -- <command>` for missing tools.
- Git works natively (`git status`, `git commit`, `git push`); no `--git-dir`/`--work-tree` needed.

- Use Conventional Commits for commit messages.
- Do not push, create repos, or trigger deployments unless the user explicitly confirms.

## GitHub CLI

- When using `gh`, invoke it through the user's Guix shell:

```sh
guix shell -L /home/trev/Workspace/dotfiles gh -- gh <args>
```

- Current remote is `git@github.com:trevarj/btc_foss.git`.
- GitHub Pages deploy can be triggered with:

```sh
guix shell -L /home/trev/Workspace/dotfiles gh -- gh workflow run pages.yml --repo trevarj/btc_foss
```

## Build And Verify

Run these after source changes:

```sh
cargo fmt -- --check
cargo test
cargo clippy -- -D warnings
cargo run -- fixture --config config/site.toml --feed fixtures/feed.json --out public
```

Useful local preview:

```sh
python3 -m http.server 8081 --directory public
```

Then open:

```text
http://127.0.0.1:8081/btc_foss/
```

If Python is unavailable, use a Guix shell rather than installing it.

## Styling And Themes

- The page imports the main site stylesheet from `/static/style.css`.
- It also generates and loads a page-local `theme-bitcoin.css`.
- The Bitcoin theme follows the same pattern as the user's site themes in `/home/trev/Workspace/trevarj.github.io/static`, especially `theme-jade.css`:

```css
html[data-theme="bitcoin"],
html[data-theme="bitcoin"]::backdrop {
  color-scheme: dark;
  ...
}
```

- The generated HTML must keep `data-theme="bitcoin"` on `<html>`.
- Keep global theme variables in `render_bitcoin_theme_css()`.
- Keep page-specific layout rules in `render_css()`.
- The Bitcoin theme should be dark, legible, orange-forward, and broader than the main site themes. It can use restrained teal, warm neutral, green, and red accents, but Bitcoin orange must remain the dominant accent.
- The H1 logo is the Wikimedia Bitcoin SVG:

```text
https://upload.wikimedia.org/wikipedia/commons/4/46/Bitcoin.svg
```

## UI Decisions

- The timeline should be compact and list-like.
- All items are collapsed by default.
- Collapsed rows should keep data aligned in stable columns:

```text
icon | title | repo | date | type
```

- Keep repository visible alongside the title.
- Group events that share the same repo and calendar date.
- For multi-event groups, the collapsed title should summarize the group, currently like `2 activities`.
- Single-event row titles are links to the event.
- Multi-event group titles should not be a fake link.
- Expanded group details should show each event with an event-type icon, not a numbered list.
- Do not add redundant link locations. In particular, do not reintroduce `btc-source-link` or a separate small source-link icon when the title already links to the source.
- Do not show redundant `n items` text in the collapsed row.
- Do not use “Open source thread” copy.
- Do not add duplicate `<p>` summary text inside `.btc-thread-detail`.

## Activity Semantics

- Commit entries must link to repo-scoped commit-day URLs, not to the GitHub profile.
- Avoid empty generic “Commits” rows. Commit titles should include useful context such as commit count and repo.
- Preserve event details when making rows more compact. Compact layout must not discard repo, title, type, or date context.
- Grouping currently happens by `repo + short_date(occurred_at)`.
- Normal collection should include reviews and comments from the configured two-year window, including older `bitcoin/bitcoin` activity that would be missed by a shorter cache refresh.

## Source Guidelines

- Prefer editing existing generator functions over adding separate static assets.
- Do not hand-edit ignored `public/` output as the primary fix. Regenerate it from Rust.
- Keep tests focused on render contracts when changing HTML/CSS generation.
- Use `rg` for searching.
- Avoid unrelated refactors.
