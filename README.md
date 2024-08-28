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
  uses: WorkingRobot/ffxiv-downloader@v1
  with:
    output-path: ${{ github.workspace }}/exd-data
    file-regex: '^sqpack\/ffxiv\/0a0000\..+$'
```

`output-path` specifies exactly where the downloaded files get stored, and `file-regex` is a regex expression that matches which files must be downloaded. Here, we only want EXD/Excel related files, so we'll make sure to filter out anything that doesn't begin with `sqpack/ffxiv/0a0000`.

## Caching

When running this downloaded in a CI/CD environment, it's important to be aware of the volume of data you're downloading. FFXIV does not update often, so it's best to rely on GitHub's built-in action cache whenever possible to speed up your build times. Luckily, this is extremely easy. This action already takes care of only downloading data when it's necessary by storing the currently downloaded version in a file called `cached-ver.txt` inside your `output-path`. All you need to do is cache its data. You can do so by prepending the following step before your downloader step:
```yaml
- name: Retrieve cache
  uses: actions/cache@v4
  with:
    path: ${{ github.workspace }}/exd-data
    key: exd-files-data # Can be anything, as long as it doesn't conflict with any other cache key
```

## Inputs

### `repository-slug`
Slug of the repository to download from. [Thaliak](https://thaliak.xiv.dev) holds a list of all repositories and their slugs. Defaults to `4e9a232b`, which is the Win32 Global/JP base game.

### `output-path`
Path to download files to. Defaults to `.` or the current directory.

### `file-regex`
Regex to match files to download. Defaults to `.*`, which matches all files. Retroactively changing this should invalidate any cached files.

### `current-version`
Version of the currently downloaded files. Will be cached and retrieved from `{output-path}/cached-ver.txt` if omitted.

## Outputs

### `updated-files`
Semicolon separated list of files updated.

### `latest-version`
Version of the repository after the update.
