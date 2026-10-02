
# Goal

Have a github pages off of my site "trevs.site/btc_foss" that is a timeline feed
of all my contributions to Bitcoin related open source projects on Github.

Used to audit my work and prove that I am active in the Bitcoin space.

## Design

- Runs a github action daily to farm my contributions and cultivate them in a
  presentable way.
- Compact repo/day groups are collapsed by default; native expandable details
  retain each event's date, type, title, status, and GitHub link.
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
