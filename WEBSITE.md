# Publishing the website

The site lives in `docs/` and is served by GitHub Pages at <https://busmanager.yoz-team.online>.
`docs/CNAME` already contains the domain. You only do the steps below once.

## 1. Turn on GitHub Pages

1. Open the repository on GitHub: **Settings -> Pages**.
2. **Source:** *Deploy from a branch*. **Branch:** `main`, folder `/docs`. Save.
3. **Custom domain:** `busmanager.yoz-team.online` (it is already filled in by `docs/CNAME`). Save.

## 2. Point the domain at GitHub

At the company that manages the DNS of `yoz-team.online`, add one record:

| Type | Name / host | Value |
|---|---|---|
| `CNAME` | `busmanager` | `yozteam.github.io` |

If you use Cloudflare, set the record to **DNS only** (grey cloud) until GitHub has issued the certificate.
DNS changes can take from a few minutes up to a day.

## 3. HTTPS

When **Settings -> Pages** shows "DNS check successful", tick **Enforce HTTPS**.
(The certificate is issued by GitHub for free; this can take up to an hour.)

## 4. Protect the domain (recommended)

Organisation **YOZTeam -> Settings -> Pages -> Add a domain**: add `yoz-team.online` and create the
TXT record GitHub shows. Nobody else can then publish a page on your domain.

## 5. Updating the site

Every push to `main` that changes `docs/` publishes the site again (about a minute). The download
buttons read the newest release from the GitHub API, so a new release needs no change to the site.
The screenshots are in `docs/img/`.

## Checklist after the first publish

- [ ] <https://busmanager.yoz-team.online> opens and shows the lock icon
- [ ] The three download buttons lead to the latest release
- [ ] The link preview (Discord / social media) shows `docs/img/menu.jpg`
- [ ] `https://busmanager.yoz-team.online/robots.txt` and `sitemap.xml` open
