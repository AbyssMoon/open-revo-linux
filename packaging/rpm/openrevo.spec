# Собирается из готовой переносной сборки: scripts/make-rpm.sh передаёт пути
# через --define (bundle, pkgsrc, drvsrc, icon, licfile) и версии (or_version, drv_version).

# Qt и ICU лежат внутри /opt/openrevo: не объявлять их как Provides и не
# требовать системные libQt6*. Остальные зависимости (libGL, xcb, wayland…)
# rpmbuild находит сам.
%global __provides_exclude_from ^/opt/openrevo/.*$
%global __requires_exclude ^lib(Qt6|icu).*$
%global debug_package %{nil}
%global _build_id_links none

Name:           openrevo
Version:        %{or_version}
Release:        1%{?dist}
Summary:        Performance modes and charging profile for MECHREVO (Uniwill) laptops
License:        GPL-3.0-or-later AND GPL-2.0-only
URL:            https://github.com/AbyssMoon/open-revo-linux
ExclusiveArch:  x86_64

Requires:       dkms
Requires:       kernel-devel
Conflicts:      tuxedo-drivers
%{?systemd_requires}

%description
Performance modes (office / balanced / turbo), battery charging profile and
Fn Lock on MECHREVO laptops built on the Uniwill / TongFang platform.
Includes the openrevod daemon (systemd), the tray GUI started at login, the
openrevo CLI and tuxedo-drivers with MECHREVO patches built by DKMS.

%prep
%build

%install
install -d %{buildroot}/opt/openrevo
cp -a %{bundle}/{bin,lib,plugins,qml} %{buildroot}/opt/openrevo/
install -Dm755 %{pkgsrc}/dkms-setup.sh %{buildroot}/opt/openrevo/libexec/dkms-setup.sh

install -Dm755 %{pkgsrc}/openrevo-gui %{buildroot}%{_bindir}/openrevo-gui
ln -s ../../opt/openrevo/bin/openrevo %{buildroot}%{_bindir}/openrevo

install -Dm644 %{pkgsrc}/openrevod.service %{buildroot}%{_unitdir}/openrevod.service
install -Dm644 %{pkgsrc}/modules-load.conf %{buildroot}%{_modulesloaddir}/openrevo.conf
install -Dm644 %{drvsrc}/blacklist.conf %{buildroot}%{_modprobedir}/openrevo.conf

install -Dm644 %{pkgsrc}/openrevo.desktop %{buildroot}%{_datadir}/applications/openrevo.desktop
install -Dm644 %{pkgsrc}/openrevo-autostart.desktop %{buildroot}%{_sysconfdir}/xdg/autostart/openrevo.desktop
install -Dm644 %{icon} %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/openrevo.svg

S=%{buildroot}%{_usrsrc}/openrevo-drivers-%{drv_version}
install -d "$S"
cp -a %{drvsrc}/src/. "$S/"
sed 's/@VERSION@/%{drv_version}/' %{pkgsrc}/dkms.conf > "$S/dkms.conf"

install -d %{buildroot}%{_sharedstatedir}/openrevo
install -Dm644 %{licfile} %{buildroot}%{_defaultlicensedir}/openrevo/LICENSE
install -Dm644 %{drvsrc}/LICENSE %{buildroot}%{_defaultlicensedir}/openrevo/LICENSE.tuxedo-drivers

%post
/opt/openrevo/libexec/dkms-setup.sh install %{drv_version}
%systemd_post openrevod.service
if [ $1 -eq 1 ]; then
    systemctl enable --now openrevod.service || :
fi

%preun
%systemd_preun openrevod.service
if [ $1 -eq 0 ]; then
    /opt/openrevo/libexec/dkms-setup.sh remove %{drv_version}
fi

%postun
%systemd_postun_with_restart openrevod.service

%files
%license %{_defaultlicensedir}/openrevo/LICENSE
%license %{_defaultlicensedir}/openrevo/LICENSE.tuxedo-drivers
/opt/openrevo
%{_bindir}/openrevo
%{_bindir}/openrevo-gui
%{_unitdir}/openrevod.service
%{_modulesloaddir}/openrevo.conf
%{_modprobedir}/openrevo.conf
%{_datadir}/applications/openrevo.desktop
%config(noreplace) %{_sysconfdir}/xdg/autostart/openrevo.desktop
%{_datadir}/icons/hicolor/scalable/apps/openrevo.svg
%{_usrsrc}/openrevo-drivers-%{drv_version}
%dir %{_sharedstatedir}/openrevo

%changelog
* Thu Oct 29 2026 AbyssMoon <7919805+AbyssMoon@users.noreply.github.com> - 0.2.0-1
- Профили заряда останавливают зарядку на 100 / 80 / 60%, свой лимит — openrevo limit
- Вентиляторы на максимум, подсветка клавиатуры (яркость и цвет)
- USB при выключенном ноутбуке, включение при подключении зарядки
- При обновлении демон останавливается на время перезагрузки модулей
- tuxedo-drivers 4.25.0 с патчем лимита заряда (4.25.0.2)

* Wed Oct 28 2026 AbyssMoon <7919805+AbyssMoon@users.noreply.github.com> - 0.1.0-1
- Первый выпуск: режимы производительности, профиль заряда, Fn Lock
- tuxedo-drivers 4.25.0 с патчами для MECHREVO WUJIE 15X через DKMS
