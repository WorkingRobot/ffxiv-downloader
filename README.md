<h1 align="center">
  FFXIV Downloader
</h1>

<h3 align="center">Download and keep track of any files from FFXIV</h3>

<div align="center">
  <a href="https://github.com/WorkingRobot/ffxiv-downloader/actions/workflows/exd-files.yml"><img src="https://github.com/WorkingRobot/ffxiv-downloader/actions/workflows/exd-files.yml/badge.svg" alt="EXD Files"></a>
  <a href="https://github.com/WorkingRobot/ffxiv-downloader/actions/workflows/movies.yml"><img src="https://github.com/WorkingRobot/ffxiv-downloader/actions/workflows/movies.yml/badge.svg" alt="Movies"></a>
  <a href="https://github.com/WorkingRobot/ffxiv-downloader/actions/workflows/stress-data.yml"><img src="https://github.com/WorkingRobot/ffxiv-downloader/actions/workflows/stress-data.yml/badge.svg" alt="Sqpack-040000"></a>
</div>

## Usage

Here's an example build step:
```yaml
- name: Download EXD Files
  uses: WorkingRobot/ffxiv-downloader@v2
  with:
    output-path: exd-data
    regex: '^sqpack\/ffxiv\/0a0000\..+$'
```

`output-path` specifies exactly where the downloaded files get stored, and `regex` is a regex expression that matches which files must be downloaded. Here, we only want EXD/Excel related files, so we'll make sure to filter out anything that doesn't begin with `sqpack/ffxiv/0a0000`.
**Important Note:** Do not use the `${{ github.workspace }}` variable in `output-path`. `github.workspace` maps to `/github/workspace/` in Docker actions. [This is intended behavior.](https://docs.github.com/en/actions/sharing-automations/creating-actions/creating-a-docker-container-action#accessing-files-created-by-a-container-action)

## Caching

When running this downloaded in a CI/CD environment, it's important to be aware of the volume of data you're downloading. FFXIV does not update often, so it's best to rely on GitHub's built-in action cache whenever possible to speed up your build times. Luckily, this is extremely easy. This action already takes care of only downloading data when it's necessary by storing the currently downloaded version in a file called `.cachemeta.json` inside your `output-path` (unless explicitly disabled by `skip-cache`.) All you need to do is cache its data. You can do so by prepending the following step before your downloader step:
```yaml
- name: Retrieve cache
  uses: actions/cache@v4
  with:
    path: exd-data
    key: exd-files-data # Can be anything, as long as it doesn't conflict with any other cache key
```

## Inputs

### `slug`
Slug of the repository to download from. [Thaliak](https://thaliak.xiv.dev) holds a list of all repositories and their slugs. Defaults to [`4e9a232b`](https://thaliak.xiv.dev/repository/4e9a232b), which is the Win32 Global/JP base game.

### `version`
Version of the game to download. If omitted, the latest version will automatically be selected.

### `output-path`
Path to download files to. Defaults to `.` or the current directory.

**Important Note:** Do not use the `${{ github.workspace }}` variable in `output-path`. `github.workspace` maps to `/github/workspace/` in Docker actions. [This is intended behavior.](https://docs.github.com/en/actions/sharing-automations/creating-actions/creating-a-docker-container-action#accessing-files-created-by-a-container-action)

### `regex`
Regex to match files to download. Defaults to `.*`, which matches all files. Retroactively changing this does not invalidate any cached files. **If this value is changed to include new, undownloaded files, you must invalidate the cache manually.**

### `parallelism`
Number of concurrent threads to use when updating. Defaults to `4`.

### `clut-path`
Best to leave default. Automatically grabs clut (index-style) files to speed up downloads dramatically.

### `skip-cache`
If true, a `.cachemeta.json` file will not be created to track versioning. As such, files will be redownloaded every time this runs.

### `debug`
Toggles debug logging. Disabled by default.

### `verbose`
Toggles verbose logging. Enabled by default.

## Outputs

### `updated`
Whether or not a version update occurred.

### `version`
Currently installed version.
