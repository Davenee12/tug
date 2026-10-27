# Releasing tug

How a tug release is built, published, checked and, if needed, pulled back. Everything here is
free: GitHub Actions costs nothing for a public repository, automatic updates are signed with
Tauri's own minisign key, and code signing comes from SignPath's free open-source programme once
it approves the project.

- **[.github/workflows/release.yml](../.github/workflows/release.yml)** builds the release on a
  Windows runner whenever a `v*` tag is pushed, and leaves it as a **draft**. Nothing is public
  until you publish the draft. It has three jobs: `build` (checks and builds, with no secrets at
  all), `sign` (code signing and the updater signature, in the `release` environment, which you
  approve each time and which only `v*` tags may use) and `release` (creates the draft). The
  signing key is never in a job that runs npm or cargo.
- **Automatic updates** (`src-tauri/src/updater/`) read `latest.json` from the newest *published*
  release, so publishing is also what offers the update to installed copies.

## One-time setup

Do these once. Steps 1 to 4 switch on automatic updates; step 5 is for code signing later. Nothing
here changes by itself: each step is something you run or click.

### 1. Make the updater signing key

On your own PC, in PowerShell (not in the repository folder):

```powershell
npx tauri signer generate -w "$env:USERPROFILE\.tauri\tug-updater.key"
```

It asks for a password: use a long one and keep it with the key (step 4). It writes two files:

- `tug-updater.key`: the **private** key. Secret. Never commit it, paste it into an issue or chat,
  or leave a copy in Downloads.
- `tug-updater.key.pub`: the **public** key. Safe to publish; it goes into tug.

### 2. Put the public key in tug

Open `tug-updater.key.pub` in Notepad and copy its whole content (one long line of letters and
digits). In `src-tauri/tauri.conf.json`, paste it between the quotes of `plugins › updater ›
pubkey`:

```json
"plugins": {
  "updater": {
    "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IC4uLg==",
```

(That value is only an example of the shape.) Commit it through a pull request like any change.
Until this is done every build says "Automatic updates aren't set up for this build" in Settings ›
About and never contacts GitHub, so it's safe to merge this feature first and add the key later.
`cargo test` checks the value is either empty or a real minisign public key.

### 3. Lock releases down, then give the `release` environment the private key

The signing key must be readable only by a release you approved, built from a version tag only
you can create. A plain repository secret isn't enough: any branch's copy of the workflow could
read it. So the key lives in a GitHub **environment** called `release`, which only `v*` tags may
deploy to and which waits for your approval, and a **tag ruleset** stops anyone but an admin
creating a `v*` tag. Do 3a to 3d in order, from PowerShell with the GitHub CLI signed in as you
(`gh auth status`). Each command prints JSON when it worked.

**3a. Create the `release` environment**, with you as its required reviewer and custom
deployment rules (you can approve your own runs, as the only maintainer):

```powershell
$me = gh api users/Davenee12 --jq .id
@"
{
  "wait_timer": 0,
  "prevent_self_review": false,
  "reviewers": [ { "type": "User", "id": $me } ],
  "deployment_branch_policy": { "protected_branches": false, "custom_branch_policies": true }
}
"@ | gh api -X PUT repos/Davenee12/tug/environments/release --input -
```

**3b. Let only version tags deploy to it:**

```powershell
gh api -X POST repos/Davenee12/tug/environments/release/deployment-branch-policies -f name='v*' -f type=tag
```

Check it: `gh api repos/Davenee12/tug/environments/release/deployment-branch-policies` lists one
policy, `v*`, of type `tag`. (In the browser: Settings › Environments › release shows "Required
reviewers: Davenee12" and "Deployment branches and tags: Selected… v* (tag)".)

**3c. Put the key and its password in the environment** (not the repository; the CLI never echoes
them):

```powershell
Get-Content -Raw "$env:USERPROFILE\.tauri\tug-updater.key" | gh secret set TAURI_SIGNING_PRIVATE_KEY --env release --repo Davenee12/tug
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --env release --repo Davenee12/tug
```

`gh secret list --env release --repo Davenee12/tug` shows both names. If you ever saved them as
*repository* secrets, delete those, because any branch could read them:

```powershell
gh secret list --repo Davenee12/tug
gh secret delete TAURI_SIGNING_PRIVATE_KEY --repo Davenee12/tug
gh secret delete TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo Davenee12/tug
```

**3d. Only admins may create, move or delete `v*` tags** (a ruleset; actor `5` is the repository
admin role, which may bypass it):

```powershell
@"
{
  "name": "Release tags",
  "target": "tag",
  "enforcement": "active",
  "conditions": { "ref_name": { "include": ["refs/tags/v*"], "exclude": [] } },
  "rules": [ { "type": "creation" }, { "type": "update" }, { "type": "deletion" } ],
  "bypass_actors": [ { "actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always" } ]
}
"@ | gh api -X POST repos/Davenee12/tug/rulesets --input -
```

Check it: `gh api repos/Davenee12/tug/rulesets` lists "Release tags" as `active` (Settings ›
Rules › Rulesets in the browser).

The `sign` job only exists on a real release run (a tag push, or a manual run on a tag with dry run
off); pull requests and dry runs run `build` alone, which has no secrets at all. Before it reads
anything, GitHub waits for you to approve the deployment to `release`, and it checks that the
signature it makes matches the public key in `tauri.conf.json` before the draft is created.

### 4. Back the private key up, offline

**If the private key or its password is lost, installed copies of tug can never update
automatically again.** Every installed tug only accepts updates signed with it; a new key means
everyone has to download and reinstall by hand. And anyone who gets the key (and password) could
sign an update every installed tug would accept.

- Copy `tug-updater.key` and the password to **two** offline places, for example a USB stick kept
  somewhere safe and your password manager (as an attached file plus the password).
- Check the copies open. Then delete any other copies (Downloads, the desktop, cloud-synced
  folders). The GitHub secret is not a backup: GitHub never shows it again.

### 5. Code signing with SignPath (later, optional)

When SignPath approves tug for free open-source signing (see the application draft), add these
as secrets of the `release` environment (`gh secret set <NAME> --env release --repo
Davenee12/tug`, like step 3c); until all three exist the signing steps are skipped.

| Name | Value |
|---|---|
| `SIGNPATH_API_TOKEN` | An API token for a SignPath CI user that may submit signing requests |
| `SIGNPATH_ORGANIZATION_ID` | Your SignPath organization id |
| `SIGNPATH_PROJECT_SLUG` | The project's slug in SignPath, for example `tug` |
| `SIGNPATH_SIGNING_POLICY_SLUG` | Optional; defaults to `release-signing` |

In the SignPath project, link the GitHub repository (GitHub.com as the trusted build system) and
create one artifact configuration with the slug `downloads` (adjust to SignPath's current schema if
it asks for more):

```xml
<artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
  <zip-file>
    <pe-file path="*-setup.exe"><authenticode-sign /></pe-file>
    <pe-file path="*-portable.exe"><authenticode-sign /></pe-file>
  </zip-file>
</artifact-configuration>
```

Each release then makes one signing request that waits (up to an hour) for you to approve it in
SignPath. The workflow fails if a signature isn't valid afterwards. Signing happens in the `sign`
job, which runs no npm or cargo, so the installer isn't rebuilt there: what's signed is the
installer you download and the portable app. The copy of `tug.exe` inside the installer isn't
signed on its own. The checksums, the updater
signature and `latest.json` are made after signing, so they match the signed files. Once the first
signed release is out, add the "Code signing policy" section from the application draft to the
README.

## Cutting a release

1. **Prepare it on a branch**, as today: bump the version everywhere (`package.json`,
   `package-lock.json`, `src-tauri/Cargo.toml`, the workspace crates, `Cargo.lock`,
   `src-tauri/tauri.conf.json`), add the `## v0.5.19 — YYYY-MM-DD` section to `CHANGELOG.md` and the
   entry in `src/lib/whatsNew.ts`. Open the pull request, let the checks pass, merge it.
2. **Tag the merged commit on main:**

   ```powershell
   git checkout main
   git pull
   git tag v0.5.19
   git push origin v0.5.19
   ```

   The tag must match the version files exactly, and the commit must be on `main`, or the workflow
   stops before building.
3. **Watch the run** under Actions › release (about 45–60 minutes: the full `npm run check`, then the
   release build). When `build` is done, the run waits on **Review deployments**: check it's your
   tag, then approve `release` so `sign` can run. With SignPath set up, approve its request too.
4. **Review the draft** under Releases › "tug 0.5.19". It carries `tug-0.5.19-setup.exe`,
   `tug-0.5.19-portable.exe`, `SHA256SUMS.txt` and, with the updater key set up,
   `tug-0.5.19-setup.exe.sig` and `latest.json`. The notes are the CHANGELOG section followed by the
   checksums. Edit the notes if you like.
5. **Publish it.** It starts as a **staged rollout** at 10% (below): installed copies whose
   rollout number is under 10 find it within 6 hours, download it quietly and show the **Update**
   button; Settings › About › Check for updates always takes it at once, so update your own PC that
   way first.
6. **Roll it out.** Watch it, then widen it (see Staged rollouts).

## Staged rollouts

Each installed copy keeps a number from 0 to 99, picked at random once and never sent anywhere.
`latest.json` says how far a release has rolled out (`"rollout": 10`), and the background check
only takes it once that's above the copy's number. **Check for updates** always takes the newest
release, so you (or anyone who wants it now) aren't held back. New releases start at 10%
(`INITIAL_ROLLOUT` in `scripts/release-assets.mjs`). Copies older than the version that brought
this in ignore it and update straight away.

From your PC, with the GitHub CLI signed in as you:

```powershell
node scripts/rollout.mjs status            # the newest release: its %, downloads and reported problems
node scripts/rollout.mjs status 0.5.22     # a given release
node scripts/rollout.mjs set 0.5.22 50     # roll it out to half of tug
node scripts/rollout.mjs set 0.5.22 100    # everyone
node scripts/rollout.mjs set 0.5.22 0      # pause: nobody new gets it
```

`set` rewrites the release's `latest.json` (only the `rollout` field; the signature is kept), which
isn't signed or in the checksums, so changing it never breaks a download. For a moment while it's
replaced there's no `latest.json`; a check then just tries again later.

**What to watch** (tug has no telemetry, so these are the signals):

- `status` shows GitHub's download counts and the issues filed against the version since it went
  out. **Report a problem** in tug (Settings › About) opens GitHub's bug report form titled
  `[v0.5.22] …` with the version filled in and the diagnostics on the clipboard, so those are
  counted; so are issues that just mention `v0.5.22`.
- When a version's last run didn't close normally (a crash, but also a power cut or End task), tug
  offers that report once ("tug didn't close normally last time"), titled `[v0.5.22] tug closed
  unexpectedly`; `status` counts these as crashes.
- Your own PC on the new version, and tug's log there.

**A rhythm that works:** 10% for a day or two, 50% for a day, then 100%. A hotfix can go straight
to 100%. If anything looks wrong, `set … 0` and ship the fix forward.

**A manual run** (Actions › release › Run workflow) takes the version (as in `package.json`) and a
**dry run** switch, on by default. A dry run runs only `build`: it builds and checks everything and
uploads the unsigned files as a workflow artifact for 14 days, but has no secrets, signs nothing
and creates no release. Pull requests that change the release workflow or its scripts run the same
dry run automatically. A manual run with dry run off makes a draft exactly like a tag push, but only
when it's started on the tag (`release` refuses anything else):
`gh workflow run release.yml --repo Davenee12/tug --ref v0.5.19 -f version=0.5.19 -f dry_run=false`.

**The first release with the public key** can't update the copies people already have, because
those versions don't have the updater yet. Say so in that release's notes: install it by hand once,
and every release after it arrives automatically.

## Checking a release

- **Checksums:** `Get-FileHash .\tug-0.5.19-setup.exe` (PowerShell) must equal the line in the
  release notes and `SHA256SUMS.txt`.
- **What installed copies see:** after publishing,
  <https://github.com/Davenee12/tug/releases/latest/download/latest.json> shows the new version and
  the installer's address; that address downloads the same file as the release page.
- **Code signature** (with SignPath): `Get-AuthenticodeSignature .\tug-0.5.19-setup.exe` says
  `Valid`; so does the portable exe.
- **The update itself**, on a PC with the previous version installed: Settings › About › Check for
  updates goes Checking… → Downloading… → Ready — Restart to update, and the Update button appears
  beside "tug". Restart now shows Windows' small installer progress, then tug comes back on the same
  screen and About shows the new version. tug's log (`%LOCALAPPDATA%\dev.davejames.tug\logs`) has
  `update:` lines for each step, and the reason if one failed.
- **The portable exe** shows "Update available — Download", which opens the release page.

## Rolling back

Installed copies never downgrade, so a bad release is fixed by stopping it spreading and then
shipping a fixed, newer version.

- **A draft you haven't published:** delete the draft on the Releases page, then the tag:
  `git push --delete origin v0.5.19` and `git tag -d v0.5.19`. Fix, and tag again.
- **A published release that's bad:**
  1. Stop new downloads at once: `node scripts/rollout.mjs set 0.5.22 0` pauses it for every copy
     that understands rollouts. Older copies don't, so also (or instead) edit the release and tick
     **Set as a pre-release** (GitHub's
     "latest" then points back at the previous release, which installed copies see as "not newer"),
     or delete its `latest.json` asset (checks then fail quietly and retry later). Copies that already
     downloaded it but haven't restarted can still install it when someone clicks Update.
  2. Ship the fix forward: revert or fix on `main`, bump to the next version (0.5.20), add the
     CHANGELOG entry, tag and publish. Copies that installed the bad version update to it.
  3. For anyone who can't wait, the previous version's installer is still on its release page;
     installing it by hand over the bad one works (it's a normal per-user install).
- **The updater key is lost or leaked:** make a new key (step 1), put its public key in tug (step
  2), replace both environment secrets (step 3c) and release. Installed copies can't verify the new key, so say
  in the notes that this one version must be installed by hand. If the key leaked, replace the
  secrets right away, before anything else.

## Troubleshooting the workflow

- **"version 0.5.19 doesn't match …":** a version file wasn't bumped, or the tag has a typo.
- **"Releases are built only from commits on main":** tag the merge commit on `main`, not a branch.
- **"… still contains local path(s)":** the path remapping in `scripts/remap-paths.mjs` didn't
  reach part of the build; nothing is published.
- **"mismatch: the updater signature wasn't made with the key …":** the `TAURI_SIGNING_PRIVATE_KEY`
  secret and the public key in `tauri.conf.json` aren't a pair. Fix whichever is wrong; publishing
  this build would leave every installed copy refusing the update.
- **"A release for v0.5.19 already exists":** delete the old draft first; the workflow never
  overwrites a release.
- **The run stops at "Waiting for review":** that's the `release` environment asking you to
  approve `sign` (step 3a). Approve it from the run's page.
- **"Branch … is not allowed to deploy to release" / "Tag … is not allowed":** the run wasn't on a
  `v*` tag (a manual run from `main`, say). Start it on the tag (see A manual run above).
- **The tag push is refused ("Cannot create ref due to creations being restricted"):** the tag
  ruleset (step 3d) allows only admins; push it as the owner.
