
# Goal

Have a GitHub Pages timeline at trevs.site/btc_foss of my Bitcoin FOSS
contributions on GitHub and the public git.rust-bitcoin.org Forgejo instance.

Used to audit my work and prove that I am active in the Bitcoin space.

## Design

- Runs a GitHub Action daily to collect GitHub contributions and public Forgejo
  org activity for `trev` in the configured `forgejo_repos` allowlist.
  Normal collection refreshes the last seven days of Forgejo activity; `--full`
  and a missing cache scan the configured window. Forgejo events have host-prefixed
  repository keys and IDs to distinguish them from GitHub contributions.
- Compact repo/day groups are collapsed by default; native expandable details
  retain each event's date, type, title, status, and source link.
- Desktop rows keep icon, title, repository, date, and type columns with a small
  expansion chevron. Mobile titles wrap, with repository, date, and type visible.
- Search matches original event text and repository together with the selected
  type; repository and year filters narrow groups. Summaries reflect matching
  events, counts name repo/day groups, and empty matches show guidance. Reset
  restores the original summaries and all events.
- Small-screen filters use two columns, with search and repository full width,
  comfortable touch targets, and a secondary Reset control.
- Keep the Bitcoin orange theme, legible muted labels, visible keyboard focus,
  decorative icons, and reduced-motion support; generate HTML/CSS/JS from source.
