Name:           footage
Version:        1.4.0
Release:        1%{?dist}
Summary:        Video editor for GNOME

%define debug_package %{nil}

License:        GPL-3.0-or-later
URL:            https://gitlab.com/adhami3310/Footage
Source0:        https://gitlab.com/adhami3310/Footage/-/archive/v%{version}/Footage-v%{version}.tar.gz

BuildRequires:  meson
BuildRequires:  ninja-build
BuildRequires:  pkgconfig
BuildRequires:  libgtk-4-dev
BuildRequires:  libadwaita-1-dev
BuildRequires:  blueprint-compiler
BuildRequires:  libgstreamer1.0-dev
BuildRequires:  libgstreamer-plugins-base1.0-dev
BuildRequires:  libges-1.0-dev
BuildRequires:  libglib2.0-dev

Requires:       gtk4
Requires:       libadwaita-1
Requires:       gstreamer1.0
Requires:       gstreamer1.0-plugins-base

%description
Footage is a video editor for GNOME, written in Rust using GTK4 and libadwaita.
It uses GStreamer for media processing and supports modern video editing workflows.

%prep
%autosetup -n Footage-v%{version}

%build
meson setup builddir --prefix=/usr
ninja -C builddir

%install
DESTDIR=%{buildroot} ninja -C builddir install

%files
%{_bindir}/footage
%{_datadir}/applications/io.gitlab.adhami3310.Footage.desktop
%{_datadir}/metainfo/io.gitlab.adhami3310.Footage.metainfo.xml
%{_datadir}/glib-2.0/schemas/io.gitlab.adhami3310.Footage.gschema.xml
%{_datadir}/dbus-1/services/io.gitlab.adhami3310.Footage.service
%{_datadir}/icons/hicolor/*/apps/io.gitlab.adhami3310.Footage.svg
%{_datadir}/icons/hicolor/scalable/actions/*.svg
%{_datadir}/footage/
%{_datadir}/locale/*/LC_MESSAGES/footage.mo
%license COPYING

%changelog
* Wed Jul 29 2026 Fame <fame@famelinuxpc> - 1.4.0-1
- Initial RPM package