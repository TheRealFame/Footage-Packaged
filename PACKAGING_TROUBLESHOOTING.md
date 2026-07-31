# Footage Packaging Troubleshooting

Common issues and fixes for the Footage packaging builds.

## Build fails: `blueprint-compiler` not found

The project requires the Blueprint compiler to generate GTK UI files.
- On Ubuntu 24.04, the version might be too old.
- The CI uses `ubuntu-24.10` as a workaround to provide a compatible `blueprint-compiler`.

## GSettings errors at runtime

Ensure the GSchema is compiled and installed to `/usr/share/glib-2.0/schemas/`.
In an AppImage, this usually requires `GSETTINGS_SCHEMA_DIR` to be set in the AppRun script.

## GStreamer plugin issues

If video playback or encoding fails, ensure the following plugins are installed:
- `gstreamer1.0-plugins-good`
- `gstreamer1.0-plugins-bad`
- `gstreamer1.0-plugins-ugly`
- `gstreamer1.0-libav`

## RPM build fails on Ubuntu

We use the `rpm` package on `ubuntu-24.04` to build the RPM. Ensure you have `rpm` installed:
```bash
sudo apt-get install rpm
```

_Generated with AI assistance_
