<div align="center">
<h1>Footage</h1>

Polish your videos.

<img src="data/resources/icons/hicolor/scalable/apps/io.gitlab.adhami3310.Footage.svg" width="128" height="128" alt="Footage icon">

[![Installs](https://img.shields.io/flathub/downloads/io.gitlab.adhami3310.Footage?style=for-the-badge)](https://flathub.org/apps/details/io.gitlab.adhami3310.Footage)
[![Latest Tag](https://img.shields.io/gitlab/v/tag/adhami3310/Footage?sort=date&style=for-the-badge)](https://gitlab.com/adhami3310/Footage/-/tags)
[![License](https://img.shields.io/gitlab/license/adhami3310/Footage?style=for-the-badge)](https://gitlab.com/adhami3310/Footage/-/raw/main/COPYING)

</div>


## Installation
<a href='https://flathub.org/apps/details/io.gitlab.adhami3310.Footage'><img width='240' alt='Download on Flathub' src='https://flathub.org/assets/badges/flathub-badge-en.png'/></a>


### Third Party Packages
You may also be able to obtain Footage from your distribution's package manager. Note these packages are maintained independently and thus may differ from the official version on Flathub. Please report any issues experienced to the package maintainer.

[![Packaging status](https://repology.org/badge/vertical-allrepos/footage.svg)](https://repology.org/project/footage/versions)


## About

Trim, flip, rotate and crop individual clips. Footage is a useful tool for quickly editing short videos and screencasts. It's also capable of exporting any video into a format of your choice. See [Press](PRESS.md) for content mentioning Footage from various writers, content creators, etc.

<img src="data/resources/screenshots/0.png" alt="Main screen with a chosen ISO and one USB memory">

## Contributing
Issues and merge requests are more than welcome. However, please take the following into consideration:

- This project follows the [GNOME Code of Conduct](https://wiki.gnome.org/Foundation/CodeOfConduct)
- Only Flatpak is supported

## Development

### GNOME Builder
The recommended method is to use GNOME Builder:

1. Install [GNOME Builder](https://apps.gnome.org/app/org.gnome.Builder/) from Flathub
1. Open Builder and select "Clone Repository..."
1. Clone `https://gitlab.com/adhami3310/Footage.git` (or your fork)
1. Press "Run Project" (▶) at the top, or `Ctrl`+`Shift`+`[Spacebar]`.

### Flatpak
You can install Footage from the latest commit:

1. Install [`org.flatpak.Builder`](https://github.com/flathub/org.flatpak.Builder) from Flathub
1. Clone `https://gitlab.com/adhami3310/Footage.git` (or your fork)
1. `cd flatpatk`
1. Run `flatpak run org.flatpak.Builder --install --user --force-clean build-dir io.gitlab.adhami3310.Footage.json` in the terminal from the root of the repository.

### Meson
You can build and install on your host system by directly using the Meson buildsystem:

1. Install `blueprint-compiler` and other relevant codecs.
1. Run the following commands (with `/usr` prefix):
```
meson --prefix=/usr build
ninja -C build
sudo ninja -C build install
```

## Credits

Actively developed by Khaleel Al-Adhami.

Logo desgined by kramo.

Huge thanks to all of the translators who brought Footage to many other languages!

## Installation via Linux Packages

Linux packages are built automatically via GitHub Actions for each release.

### Debian / Ubuntu

```bash
sudo dpkg -i footage_*_amd64.deb
sudo apt-get install -f
```

### Fedora / RHEL / openSUSE

```bash
sudo rpm -i footage-*.rpm
```

### AppImage (universal)

```bash
chmod +x footage-x86_64.AppImage
./footage-x86_64.AppImage
```

### Arch / Manjaro

```bash
sudo pacman -U footage-*.pkg.tar.zst
```
